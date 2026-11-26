//! The syntax tree produced by the parser (SPEC.md section 4).

use std::rc::Rc;

use crate::lexer::RedirectKind;
use crate::source::{FileId, Span};

/// A parsed file.
#[derive(Debug)]
pub struct Program {
    pub file: FileId,
    pub stmts: Vec<Stmt>,
    /// The file's leading comment block (after a shebang), used as the help
    /// text of scripts without `main`.
    pub doc: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum StmtKind {
    Let {
        pattern: Pattern,
        value: Expr,
        mutable: bool,
    },
    Fn(Rc<FnDecl>),
    Task(Rc<TaskDecl>),
    Import {
        path: String,
        path_span: Span,
        alias: Option<(String, Span)>,
    },
    Requires {
        version: String,
    },
    EnvFile {
        path: String,
    },
    Assign {
        target: Place,
        op: AssignOp,
        value: Expr,
    },
    For {
        first: Pattern,
        second: Option<Pattern>,
        iter: Expr,
        body: Block,
    },
    While {
        cond: Expr,
        body: Block,
    },
    Break,
    Continue,
    Return(Option<Expr>),
    Expr(Expr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

/// The left side of an assignment: a `var` name followed by fields and
/// indexes, as in `cfg.database.port` or `xs[0]`.
#[derive(Debug, Clone)]
pub struct Place {
    pub root: String,
    pub root_span: Span,
    pub path: Vec<PlaceSeg>,
}

#[derive(Debug, Clone)]
pub enum PlaceSeg {
    Field(String, Span),
    Index(Expr),
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
    /// The block's [`Expr::height`]: 1 plus the tallest statement.
    pub height: u32,
}

impl Block {
    pub fn new(stmts: Vec<Stmt>, span: Span) -> Block {
        let height = 1 + stmts.iter().map(stmt_height).max().unwrap_or(0);
        Block {
            stmts,
            span,
            height,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Name(String, Span),
    List {
        items: Vec<Pattern>,
        rest: Option<(String, Span)>,
        span: Span,
    },
    Record {
        fields: Vec<(String, Span, Pattern)>,
        span: Span,
    },
}

impl Pattern {
    pub fn span(&self) -> Span {
        match self {
            Pattern::Name(_, s) => *s,
            Pattern::List { span, .. } | Pattern::Record { span, .. } => *span,
        }
    }

    /// Every name the pattern binds, in order.
    pub fn names(&self, out: &mut Vec<(String, Span)>) {
        match self {
            Pattern::Name(n, s) => out.push((n.clone(), *s)),
            Pattern::List { items, rest, .. } => {
                for p in items {
                    p.names(out);
                }
                if let Some((n, s)) = rest {
                    out.push((n.clone(), *s));
                }
            }
            Pattern::Record { fields, .. } => {
                for (_, _, p) in fields {
                    p.names(out);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeAnn {
    Any,
    Null,
    Bool,
    Int,
    Float,
    String,
    Bytes,
    Path,
    Duration,
    Time,
    Record,
    Fn,
    List(Option<Box<TypeAnn>>),
    Optional(Box<TypeAnn>),
    /// `"admin" | "user"`: a string equal to one of these (SPEC.md 5.11).
    Choice(Vec<String>),
}

impl TypeAnn {
    /// How the type is written in source, for messages and help.
    pub fn describe(&self) -> String {
        match self {
            TypeAnn::Any => "any".into(),
            TypeAnn::Null => "null".into(),
            TypeAnn::Bool => "bool".into(),
            TypeAnn::Int => "int".into(),
            TypeAnn::Float => "float".into(),
            TypeAnn::String => "string".into(),
            TypeAnn::Bytes => "bytes".into(),
            TypeAnn::Path => "path".into(),
            TypeAnn::Duration => "duration".into(),
            TypeAnn::Time => "time".into(),
            TypeAnn::Record => "record".into(),
            TypeAnn::Fn => "fn".into(),
            TypeAnn::List(None) => "list".into(),
            TypeAnn::List(Some(t)) => format!("list[{}]", t.describe()),
            TypeAnn::Optional(t) if matches!(**t, TypeAnn::Choice(_)) => {
                format!("{} | null", t.describe())
            }
            TypeAnn::Optional(t) => format!("{}?", t.describe()),
            TypeAnn::Choice(cs) => cs
                .iter()
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
                .join(" | "),
        }
    }

    /// The choices of a choice type, also when it is optional.
    pub fn choices(&self) -> Option<&[String]> {
        match self {
            TypeAnn::Choice(cs) => Some(cs),
            TypeAnn::Optional(t) => t.choices(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    /// The name bound in the body (`--dry-run` binds `dry_run`).
    pub name: String,
    /// The name as written, without `--` (`dry-run`); used on the command line.
    pub cli_name: String,
    pub flag: bool,
    /// The one-letter short form on the command line: `--verbose, -v`.
    pub short: Option<char>,
    pub rest: bool,
    pub ty: Option<TypeAnn>,
    pub default: Option<Expr>,
    /// The comment at the end of the parameter's line, for `--help`.
    pub help: Option<String>,
    pub span: Span,
}

#[derive(Debug)]
pub struct FnDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub body: Block,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug)]
pub struct TaskDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub needs: Vec<Need>,
    pub sources: Option<Expr>,
    pub outputs: Option<Expr>,
    pub confirm: Option<String>,
    pub hidden: bool,
    pub body: Block,
    pub doc: Option<String>,
    pub span: Span,
}

/// One entry of a task's `needs [...]` list (SPEC.md 15.5):
/// `gen`, `db.migrate` or `build(release: true)`.
#[derive(Debug, Clone)]
pub struct Need {
    /// The task name as written, `ns.task` for another namespace.
    pub name: String,
    /// The name only.
    pub name_span: Span,
    /// The whole entry, arguments included.
    pub span: Span,
    /// Named arguments: (parameter name, its span, constant value).
    pub args: Vec<(String, Span, Expr)>,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// The height of the tree rooted here: 1 for a leaf, otherwise 1 plus
    /// the tallest child (blocks and closure bodies included). The
    /// evaluator, the checker and the formatter recurse once (or a few
    /// times) per level, so the parser rejects programs whose trees are
    /// taller than its limit; computing it at construction keeps that check
    /// O(1) per node, also for long chains like `a + b + c ...`.
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    NotIn,
    And,
    Or,
    Coalesce,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::In => "in",
            BinOp::NotIn => "not in",
            BinOp::And => "and",
            BinOp::Or => "or",
            BinOp::Coalesce => "??",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone)]
pub enum StrPiece {
    Lit(String),
    /// `{expr}` or `{expr:spec}`; `span` covers the braces.
    Interp {
        expr: Expr,
        spec: Option<String>,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum Elem {
    Item(Expr),
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub enum Entry {
    Field {
        key: String,
        key_span: Span,
        value: Expr,
    },
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub enum Arg {
    Pos(Expr),
    Named {
        name: String,
        name_span: Span,
        value: Expr,
    },
    Spread(Expr),
}

/// A pattern of a `match` arm (SPEC.md 6.6).
#[derive(Debug, Clone)]
pub enum MatchPat {
    /// A literal, compared with `==`.
    Lit(Expr),
    /// `_`
    Wildcard(Span),
    /// A name: matches anything and binds it.
    Bind(String, Span),
    /// `[p1, p2]`, `[p1, ...rest]`, `[p1, ..._]`.
    List {
        items: Vec<MatchPat>,
        rest: Option<ListRest>,
        span: Span,
    },
    /// `{key: p, other}`: the keys must exist; extra keys are allowed.
    Record {
        fields: Vec<(String, MatchPat)>,
        span: Span,
    },
    /// `p1 | p2` nested inside a list or record pattern.
    Alt(Vec<MatchPat>, Span),
}

/// The `...` part of a list pattern.
#[derive(Debug, Clone)]
pub enum ListRest {
    /// `..._`: any further elements, not bound.
    Ignore,
    /// `...name`: further elements bound to `name` as a list.
    Bind(String),
}

impl MatchPat {
    pub fn span(&self) -> Span {
        match self {
            MatchPat::Lit(e) => e.span,
            MatchPat::Wildcard(s)
            | MatchPat::Bind(_, s)
            | MatchPat::List { span: s, .. }
            | MatchPat::Record { span: s, .. }
            | MatchPat::Alt(_, s) => *s,
        }
    }

    /// The names the pattern binds, in order, with their spans. For
    /// alternatives, the names of the first one (all bind the same names).
    pub fn names(&self, out: &mut Vec<(String, Span)>) {
        match self {
            MatchPat::Lit(_) | MatchPat::Wildcard(_) => {}
            MatchPat::Bind(n, s) => out.push((n.clone(), *s)),
            MatchPat::List { items, rest, span } => {
                for p in items {
                    p.names(out);
                }
                if let Some(ListRest::Bind(n)) = rest {
                    out.push((n.clone(), *span));
                }
            }
            MatchPat::Record { fields, .. } => {
                for (_, p) in fields {
                    p.names(out);
                }
            }
            MatchPat::Alt(alts, _) => {
                if let Some(first) = alts.first() {
                    first.names(out);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub pats: Vec<MatchPat>,
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

/// A piece of a command word after parsing its interpolations.
#[derive(Debug, Clone)]
pub enum CmdPart {
    Lit(String),
    Interp { expr: Expr, spec: Option<String> },
    Spread(Expr),
}

#[derive(Debug, Clone)]
pub struct CmdWord {
    pub parts: Vec<CmdPart>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Command {
    pub bang: bool,
    pub words: Vec<CmdWord>,
    pub redirects: Vec<(RedirectKind, CmdWord)>,
    pub options: Option<Box<Expr>>,
}

/// The right side of `|` (SPEC.md 8.2).
#[derive(Debug, Clone)]
pub enum Stage {
    /// `name(args)`, `module.name(args)`, `name`, or `name arg, arg`: the
    /// callee is evaluated and called with the left value prepended.
    Call { callee: Box<Expr>, args: Vec<Arg> },
    /// `recv.name(args)` parsed as a method call.
    Method {
        recv: Box<Expr>,
        name: String,
        args: Vec<Arg>,
    },
    /// Any other expression that evaluates to a function.
    Value(Box<Expr>),
    /// A command; the left value becomes its stdin.
    Run(Box<Expr>),
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Duration(i64),
    Str(Vec<StrPiece>),
    Ident(String),
    /// The implicit argument of a `.field` shorthand closure (SPEC.md 6.9).
    It,
    List(Vec<Elem>),
    Record(Vec<Entry>),
    /// Parentheses, kept so that `a ?? (b or c)` can be told apart from
    /// `a ?? b or c`.
    Group(Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Arg>,
    },
    MethodCall {
        recv: Box<Expr>,
        name: String,
        name_span: Span,
        args: Vec<Arg>,
    },
    Field {
        expr: Box<Expr>,
        name: String,
        name_span: Span,
    },
    Index {
        expr: Box<Expr>,
        index: Box<Expr>,
    },
    /// `expr?`
    Catch(Box<Expr>),
    If {
        cond: Box<Expr>,
        then: Block,
        otherwise: Option<Box<Expr>>,
    },
    Block(Block),
    Match {
        subject: Box<Expr>,
        arms: Vec<Arm>,
    },
    /// `try { } catch e { } finally { }`; at least one of `handler` and
    /// `finally` is present.
    Try {
        body: Block,
        binding: Option<String>,
        handler: Option<Block>,
        finally: Option<Block>,
    },
    Lambda(Rc<FnDecl>),
    Pipe {
        lhs: Box<Expr>,
        stage: Stage,
    },
    Run(Box<Command>),
    RunFn {
        bang: bool,
        args: Vec<Arg>,
    },
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Expr {
        let height = kind_height(&kind).saturating_add(1);
        Expr { kind, span, height }
    }
}

/// The height of the tallest direct child of an expression (0 for none).
fn kind_height(kind: &ExprKind) -> u32 {
    fn args(a: &[Arg]) -> u32 {
        a.iter()
            .map(|a| match a {
                Arg::Pos(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => e.height,
            })
            .max()
            .unwrap_or(0)
    }
    fn pieces(p: &[StrPiece]) -> u32 {
        p.iter()
            .map(|p| match p {
                StrPiece::Interp { expr, .. } => expr.height,
                StrPiece::Lit(_) => 0,
            })
            .max()
            .unwrap_or(0)
    }
    fn pat(p: &MatchPat) -> u32 {
        match p {
            MatchPat::Lit(e) => e.height,
            MatchPat::Wildcard(_) | MatchPat::Bind(..) => 0,
            MatchPat::List { items, .. } | MatchPat::Alt(items, _) => {
                items.iter().map(pat).max().unwrap_or(0) + 1
            }
            MatchPat::Record { fields, .. } => {
                fields.iter().map(|(_, p)| pat(p)).max().unwrap_or(0) + 1
            }
        }
    }
    let opt = |e: &Option<Box<Expr>>| e.as_ref().map_or(0, |e| e.height);
    let block = |b: &Option<Block>| b.as_ref().map_or(0, |b| b.height);
    match kind {
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Duration(_)
        | ExprKind::Ident(_)
        | ExprKind::It => 0,
        ExprKind::Str(p) => pieces(p),
        ExprKind::List(elems) => elems
            .iter()
            .map(|e| match e {
                Elem::Item(e) | Elem::Spread(e) => e.height,
            })
            .max()
            .unwrap_or(0),
        ExprKind::Record(entries) => entries
            .iter()
            .map(|e| match e {
                Entry::Field { value: e, .. } | Entry::Spread(e) => e.height,
            })
            .max()
            .unwrap_or(0),
        ExprKind::Group(e) | ExprKind::Unary(_, e) | ExprKind::Catch(e) => e.height,
        ExprKind::Binary(_, a, b) => a.height.max(b.height),
        ExprKind::Call { callee, args: a } => callee.height.max(args(a)),
        ExprKind::MethodCall { recv, args: a, .. } => recv.height.max(args(a)),
        ExprKind::Field { expr, .. } => expr.height,
        ExprKind::Index { expr, index } => expr.height.max(index.height),
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => cond.height.max(then.height).max(opt(otherwise)),
        ExprKind::Block(b) => b.height,
        ExprKind::Match { subject, arms } => arms
            .iter()
            .map(|a| {
                let pats = a.pats.iter().map(pat).max().unwrap_or(0);
                let guard = a.guard.as_ref().map_or(0, |g| g.height);
                pats.max(guard).max(a.body.height)
            })
            .max()
            .unwrap_or(0)
            .max(subject.height),
        ExprKind::Try {
            body,
            handler,
            finally,
            ..
        } => body.height.max(block(handler)).max(block(finally)),
        ExprKind::Lambda(decl) => fn_height(decl),
        ExprKind::Pipe { lhs, stage } => lhs.height.max(match stage {
            Stage::Call { callee, args: a } => callee.height.max(args(a)),
            Stage::Method { recv, args: a, .. } => recv.height.max(args(a)),
            Stage::Value(e) | Stage::Run(e) => e.height,
        }),
        ExprKind::Run(cmd) => {
            let word = |w: &CmdWord| {
                w.parts
                    .iter()
                    .map(|p| match p {
                        CmdPart::Lit(_) => 0,
                        CmdPart::Interp { expr, .. } | CmdPart::Spread(expr) => expr.height,
                    })
                    .max()
                    .unwrap_or(0)
            };
            let words = cmd.words.iter().map(word).max().unwrap_or(0);
            let redirects = cmd
                .redirects
                .iter()
                .map(|(_, w)| word(w))
                .max()
                .unwrap_or(0);
            words.max(redirects).max(opt(&cmd.options))
        }
        ExprKind::RunFn { args: a, .. } => args(a),
    }
}

fn params_height(params: &[Param]) -> u32 {
    params
        .iter()
        .filter_map(|p| p.default.as_ref().map(|d| d.height))
        .max()
        .unwrap_or(0)
}

fn fn_height(decl: &FnDecl) -> u32 {
    decl.body.height.max(params_height(&decl.params))
}

/// The height of a statement: that of its tallest expression or block.
pub fn stmt_height(s: &Stmt) -> u32 {
    match &s.kind {
        StmtKind::Let { value, .. } => value.height,
        StmtKind::Fn(decl) => fn_height(decl),
        StmtKind::Task(t) => {
            let opt = |e: &Option<Expr>| e.as_ref().map_or(0, |e| e.height);
            let needs = t
                .needs
                .iter()
                .flat_map(|n| n.args.iter().map(|(_, _, e)| e.height))
                .max()
                .unwrap_or(0);
            t.body
                .height
                .max(params_height(&t.params))
                .max(opt(&t.sources))
                .max(opt(&t.outputs))
                .max(needs)
        }
        StmtKind::Import { .. }
        | StmtKind::Requires { .. }
        | StmtKind::EnvFile { .. }
        | StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Return(None) => 0,
        StmtKind::Assign { target, value, .. } => target
            .path
            .iter()
            .map(|seg| match seg {
                PlaceSeg::Index(e) => e.height,
                PlaceSeg::Field(..) => 0,
            })
            .max()
            .unwrap_or(0)
            .max(value.height),
        StmtKind::For { iter, body, .. } => iter.height.max(body.height),
        StmtKind::While { cond, body } => cond.height.max(body.height),
        StmtKind::Return(Some(e)) | StmtKind::Expr(e) => e.height,
    }
}

/// The internal parameter name of implicit `.field` closures. It contains a
/// character no identifier can, so it never clashes with a user name.
pub const IT: &str = "·it";
