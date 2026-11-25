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
            TypeAnn::Optional(t) => format!("{}?", t.describe()),
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
    pub needs: Vec<(String, Span)>,
    pub sources: Option<Expr>,
    pub outputs: Option<Expr>,
    pub confirm: Option<String>,
    pub hidden: bool,
    pub body: Block,
    pub doc: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
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
    Interp { expr: Expr, spec: Option<String> },
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

#[derive(Debug, Clone)]
pub enum MatchPat {
    Lit(Expr),
    Wildcard,
    Bind(String),
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
        Expr { kind, span }
    }
}

/// The internal parameter name of implicit `.field` closures. It contains a
/// character no identifier can, so it never clashes with a user name.
pub const IT: &str = "·it";
