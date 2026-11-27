//! `ok self fmt`: the canonical layout (SPEC.md 17).
//!
//! The formatter works on the token stream of a file, guided by its syntax
//! tree, and keeps the line structure the author chose:
//!
//! * **Indentation** is two spaces per open bracket *line*: the lines inside
//!   a bracket are indented one level deeper than the line that opened it,
//!   however many brackets that line opened. A line that continues the
//!   previous one (a leading `|` or `.name`, or a previous line ending in an
//!   operator) gets one more level.
//! * **Spacing inside a line** is normalised: one space around binary
//!   operators, after `,` and `:`, inside one-line blocks (`{ x }`); none
//!   inside `( )`, `[ ]` and records (`{a: 1}`). Where whitespace changes
//!   meaning (before `(`, `[` and `.`, and around a `-` that is not an
//!   operator, as in `--dry-run` or `take -1`), the presence or absence of a
//!   space is kept as written.
//! * **Long lines** (more than [`WIDTH`] columns of code) are broken: first
//!   before each `|` of a pipeline, otherwise the largest bracket group on
//!   the line is split one element per line with a trailing comma, or a
//!   one-line block is opened up.
//! * Comments stay where they are: own-line comments take the indentation of
//!   the code they precede, end-of-line comments keep their column (or
//!   follow two spaces after code that grew into it). String literals and
//!   commands (`run ...`) are copied verbatim.
//!
//! Formatting must never change what a program means. [`format_source`]
//! therefore parses its own output and compares the syntax tree (ignoring
//! positions) and the comments with the original; if they differ, it
//! refuses instead of returning the text.

use std::collections::{HashSet, VecDeque};

use crate::ast::*;
use crate::error::{ErrKind, OkError};
use crate::lexer::{self, Kw, Lexed, Tok};
use crate::source::FileId;

/// The maximum width of a line's code, in characters. End-of-line comments
/// are not counted.
pub const WIDTH: usize = 100;

const INDENT: &str = "  ";

/// Format source text. Fails if the text does not parse.
pub fn format_source(src: &str) -> Result<String, OkError> {
    format_with_width(src, WIDTH)
}

/// The text the formatter works on: `\n` line endings, no byte order mark.
pub fn normalize(src: &str) -> String {
    let norm = src.replace("\r\n", "\n");
    match norm.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_string(),
        None => norm,
    }
}

/// [`format_source`] for a file in the source map, so that a syntax error
/// points into it. `src` must be [`normalize`]d.
pub fn format_file(file: FileId, src: &str) -> Result<String, OkError> {
    format_impl(file, src, WIDTH)
}

/// [`format_source`] with another line width. Tests use narrow widths to
/// exercise line breaking on every file of the repository.
pub fn format_with_width(src: &str, width: usize) -> Result<String, OkError> {
    format_impl(0, &normalize(src), width)
}

fn format_impl(file: FileId, text: &str, width: usize) -> Result<String, OkError> {
    let prog = crate::parser::parse(file, text)?;
    let lexed = lexer::lex(file, text)?;
    let mut f = Formatter::new(text, &lexed, &prog);
    f.width = width;
    let out = f.run();
    verify(&prog, &lexed, &out)?;
    Ok(out)
}

/// Check that `out` means exactly what the original meant: the same syntax
/// tree, doc comments and parameter help included, and the same comments in
/// the same order.
fn verify(prog: &Program, lexed: &Lexed, out: &str) -> Result<(), OkError> {
    let refuse = || {
        OkError::new(
            ErrKind::Value,
            "internal formatter error: formatting would change the meaning of the file, so it was left unchanged",
        )
        .with_help("please report this as a bug in ok, with the file")
    };
    let prog2 = crate::parser::parse(0, out).map_err(|_| refuse())?;
    let lexed2 = lexer::lex(0, out).map_err(|_| refuse())?;
    let comments = |l: &Lexed| -> Vec<(String, bool)> {
        l.comments
            .iter()
            .map(|c| (c.text.clone(), c.own_line))
            .collect()
    };
    if tree_key(prog) != tree_key(&prog2) || comments(lexed) != comments(&lexed2) {
        return Err(refuse());
    }
    Ok(())
}

/// The syntax tree as text with every source position removed, so two
/// programs that differ only in layout give the same key.
pub fn tree_key(prog: &Program) -> String {
    let dump = format!("{:?}\n{:?}", prog.stmts, prog.doc);
    let mut out = String::with_capacity(dump.len());
    let mut rest = dump.as_str();
    while let Some(i) = rest.find("Span {") {
        out.push_str(&rest[..i]);
        out.push_str("Span");
        rest = &rest[i..];
        match rest.find('}') {
            Some(j) => rest = &rest[j + 1..],
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// One token of the file with its source text.
struct FTok<'a> {
    tok: &'a Tok,
    start: usize,
    end: usize,
    /// The source text, copied verbatim. Commands lose trailing blanks.
    text: &'a str,
    /// A comment lexed as part of the token (a command's end-of-line comment).
    has_comment: bool,
}

impl FTok<'_> {
    fn multiline(&self) -> bool {
        self.text.contains('\n')
    }
}

/// Something on its own line between two lines of code.
#[derive(Clone)]
enum Item {
    Blank,
    Comment(String),
}

/// What the syntax tree says about tokens, by source offset.
#[derive(Default)]
struct Info {
    /// `{` of record literals.
    record_exprs: HashSet<usize>,
    /// `{` of record patterns (`let {a, b} = ...`, match patterns).
    record_pats: HashSet<usize>,
    /// `{` of blocks.
    blocks: HashSet<usize>,
    /// `(` and `[` whose contents are a comma list that may end with a
    /// trailing comma: call arguments, parameters, list literals, `needs`.
    comma_lists: HashSet<usize>,
    /// `|` that are pipeline operators (not match alternatives or choices).
    pipes: HashSet<usize>,
    /// `-` that are binary operators.
    minus_binary: HashSet<usize>,
    /// `-` that are prefix operators.
    minus_unary: HashSet<usize>,
}

/// Positions the syntax tree only knows indirectly ("the token after this
/// expression"); resolved against the tokens once the walk is done.
#[derive(Default)]
struct Requests {
    pipe_after: Vec<usize>,
    minus_after: Vec<usize>,
    /// A `(` exactly at this offset starts call arguments.
    paren_at: Vec<usize>,
    /// The first token at or after this offset, if it is `(`, starts
    /// parameters.
    paren_after: Vec<usize>,
    /// `recv.name(`: a `.`, a name and an adjacent `(` after this offset.
    method_after: Vec<usize>,
    /// The `needs [` of a task header between these offsets.
    needs_between: Vec<(usize, usize)>,
}

struct Walk<'a> {
    src: &'a str,
    info: Info,
    req: Requests,
}

impl Walk<'_> {
    fn starts_with(&self, off: usize, s: &str) -> bool {
        self.src.get(off..).is_some_and(|t| t.starts_with(s))
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let { pattern, value, .. } => {
                self.pattern(pattern);
                self.expr(value);
            }
            StmtKind::Fn(d) => {
                self.req.paren_after.push(d.name_span.end as usize);
                self.fn_decl(d);
            }
            StmtKind::Task(t) => {
                self.req.paren_after.push(t.name_span.end as usize);
                self.req
                    .needs_between
                    .push((t.name_span.end as usize, t.body.span.start as usize));
                self.params(&t.params);
                for n in &t.needs {
                    for (_, _, v) in &n.args {
                        self.expr(v);
                    }
                }
                if let Some(e) = &t.sources {
                    self.expr(e);
                }
                if let Some(e) = &t.outputs {
                    self.expr(e);
                }
                self.block(&t.body);
            }
            StmtKind::Import { .. } | StmtKind::Requires { .. } | StmtKind::EnvFile { .. } => {}
            StmtKind::Assign { target, value, .. } => {
                for seg in &target.path {
                    if let PlaceSeg::Index(e) = seg {
                        self.expr(e);
                    }
                }
                self.expr(value);
            }
            StmtKind::For {
                first,
                second,
                iter,
                body,
            } => {
                self.pattern(first);
                if let Some(p) = second {
                    self.pattern(p);
                }
                self.expr(iter);
                self.block(body);
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Return(Some(e)) | StmtKind::Expr(e) => self.expr(e),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn block(&mut self, b: &Block) {
        // Closures written `x => expr` get a block that is not in the source.
        if self.starts_with(b.span.start as usize, "{") {
            self.info.blocks.insert(b.span.start as usize);
        }
        self.stmts(&b.stmts);
    }

    fn params(&mut self, params: &[Param]) {
        for p in params {
            if let Some(d) = &p.default {
                self.expr(d);
            }
        }
    }

    fn fn_decl(&mut self, d: &FnDecl) {
        self.params(&d.params);
        self.block(&d.body);
    }

    fn pattern(&mut self, p: &Pattern) {
        match p {
            Pattern::Name(..) => {}
            Pattern::List { items, .. } => {
                for i in items {
                    self.pattern(i);
                }
            }
            Pattern::Record { fields, span } => {
                self.info.record_pats.insert(span.start as usize);
                for (_, _, p) in fields {
                    self.pattern(p);
                }
            }
        }
    }

    fn match_pat(&mut self, p: &MatchPat) {
        match p {
            MatchPat::Lit(e) => {
                if self.starts_with(e.span.start as usize, "-") {
                    self.info.minus_unary.insert(e.span.start as usize);
                }
            }
            MatchPat::Wildcard(_) | MatchPat::Bind(..) => {}
            MatchPat::List { items, .. } => {
                for i in items {
                    self.match_pat(i);
                }
            }
            MatchPat::Record { fields, span } => {
                self.info.record_pats.insert(span.start as usize);
                for (_, p) in fields {
                    self.match_pat(p);
                }
            }
            MatchPat::Alt(alts, _) => {
                for a in alts {
                    self.match_pat(a);
                }
            }
        }
    }

    fn args(&mut self, args: &[Arg]) {
        for a in args {
            match a {
                Arg::Pos(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => self.expr(e),
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        let start = e.span.start as usize;
        match &e.kind {
            ExprKind::Null
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Duration(_)
            | ExprKind::Str(_)
            | ExprKind::Ident(_)
            | ExprKind::It
            | ExprKind::Run(_) => {}
            ExprKind::List(elems) => {
                self.info.comma_lists.insert(start);
                for el in elems {
                    match el {
                        Elem::Item(x) | Elem::Spread(x) => self.expr(x),
                    }
                }
            }
            ExprKind::Record(entries) => {
                self.info.record_exprs.insert(start);
                for en in entries {
                    match en {
                        Entry::Field { value, .. } => self.expr(value),
                        Entry::Spread(x) => self.expr(x),
                    }
                }
            }
            ExprKind::Group(x) | ExprKind::Catch(x) => self.expr(x),
            ExprKind::Unary(op, x) => {
                if *op == UnOp::Neg {
                    self.info.minus_unary.insert(start);
                }
                self.expr(x);
            }
            ExprKind::Binary(op, l, r) => {
                if *op == BinOp::Sub {
                    self.req.minus_after.push(l.span.end as usize);
                }
                self.expr(l);
                self.expr(r);
            }
            ExprKind::Call { callee, args } => {
                self.req.paren_at.push(callee.span.end as usize);
                self.expr(callee);
                self.args(args);
            }
            ExprKind::MethodCall {
                recv,
                name_span,
                args,
                ..
            } => {
                self.req.paren_at.push(name_span.end as usize);
                self.expr(recv);
                self.args(args);
            }
            ExprKind::Field { expr, .. } => self.expr(expr),
            ExprKind::Index { expr, index } => {
                self.expr(expr);
                self.expr(index);
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.expr(o);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    for p in &arm.pats {
                        self.match_pat(p);
                    }
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.expr(&arm.body);
                }
            }
            ExprKind::Try {
                body,
                handler,
                finally,
                ..
            } => {
                self.block(body);
                if let Some(h) = handler {
                    self.block(h);
                }
                if let Some(f) = finally {
                    self.block(f);
                }
            }
            ExprKind::Lambda(d) => {
                if self.starts_with(d.name_span.start as usize, "fn") {
                    self.req.paren_after.push(d.name_span.end as usize);
                }
                self.fn_decl(d);
            }
            ExprKind::Pipe { lhs, stage } => {
                self.req.pipe_after.push(lhs.span.end as usize);
                self.expr(lhs);
                match stage {
                    Stage::Call { callee, args } => {
                        self.req.paren_at.push(callee.span.end as usize);
                        self.expr(callee);
                        self.args(args);
                    }
                    Stage::Method { recv, args, .. } => {
                        self.req.method_after.push(recv.span.end as usize);
                        self.expr(recv);
                        self.args(args);
                    }
                    Stage::Value(x) | Stage::Run(x) => self.expr(x),
                }
            }
            ExprKind::RunFn { args, .. } => self.args(args),
        }
    }
}

fn is_opener(t: &Tok) -> bool {
    matches!(t, Tok::LParen | Tok::LBracket | Tok::LBrace)
}

fn is_closer(t: &Tok) -> bool {
    matches!(t, Tok::RParen | Tok::RBracket | Tok::RBrace)
}

/// Binary operators that always get one space on each side.
fn is_spaced_op(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Plus
            | Tok::Star
            | Tok::Slash
            | Tok::Percent
            | Tok::EqEq
            | Tok::NotEq
            | Tok::Lt
            | Tok::Le
            | Tok::Gt
            | Tok::Ge
            | Tok::Assign
            | Tok::PlusEq
            | Tok::MinusEq
            | Tok::FatArrow
            | Tok::Coalesce
            | Tok::Pipe
            | Tok::Kw(Kw::And)
            | Tok::Kw(Kw::Or)
            | Tok::Kw(Kw::In)
    )
}

/// Whether a token at the end of a line means the statement continues.
fn continues(t: &Tok) -> bool {
    matches!(
        t,
        Tok::Plus
            | Tok::Minus
            | Tok::Star
            | Tok::Slash
            | Tok::Percent
            | Tok::EqEq
            | Tok::NotEq
            | Tok::Lt
            | Tok::Le
            | Tok::Gt
            | Tok::Ge
            | Tok::Kw(Kw::And)
            | Tok::Kw(Kw::Or)
            | Tok::Coalesce
            | Tok::Pipe
            | Tok::Assign
            | Tok::PlusEq
            | Tok::MinusEq
            | Tok::FatArrow
    )
}

/// What the indentation of a line depends on: the indentation of each
/// open bracket's line, whether the previous line ended in an operator, and
/// the previous line's last token.
#[derive(Clone, Default)]
struct IndentState {
    stack: Vec<usize>,
    prev_continues: bool,
    prev_last: Option<usize>,
}

/// A line of code: a run of tokens, and what precedes it.
struct Line {
    first: usize,
    last: usize,
}

struct Formatter<'a> {
    toks: Vec<FTok<'a>>,
    /// The matching bracket of each bracket token.
    mate: Vec<Option<usize>>,
    /// Whether the source has whitespace between token `i - 1` and `i`.
    space_before: Vec<bool>,
    /// Whether the source has a line break between token `i - 1` and `i`.
    newline_before: Vec<bool>,
    /// Blank lines and own-line comments before token `i`'s line.
    items_before: Vec<Vec<Item>>,
    /// The end-of-line comment after token `i`: the column it starts at
    /// (in characters) and its text.
    trailing: Vec<Option<(usize, String)>>,
    /// Blank lines and comments after the last token.
    items_end: Vec<Item>,
    info: Info,
    /// Breaks added for long lines: a new line starts at these tokens.
    forced: HashSet<usize>,
    /// Trailing commas added after these tokens.
    commas: HashSet<usize>,
    /// The maximum width of a line's code.
    width: usize,
    /// Whether the file starts with a shebang line (after an optional BOM),
    /// which is the only `#!` comment that may stay on the first line.
    shebang: bool,
}

impl<'a> Formatter<'a> {
    fn new(src: &'a str, lexed: &'a Lexed, prog: &Program) -> Formatter<'a> {
        let comment_spans: Vec<(usize, usize)> = lexed
            .comments
            .iter()
            .map(|c| (c.span.start as usize, c.span.end as usize))
            .collect();
        let toks: Vec<FTok> = lexed
            .tokens
            .iter()
            .filter(|t| !matches!(t.tok, Tok::Newline | Tok::Eof))
            .map(|t| {
                let (start, end) = (t.span.start as usize, t.span.end as usize);
                let mut text = &src[start..end];
                if matches!(t.tok, Tok::Command(_)) {
                    text = text.trim_end_matches([' ', '\t']);
                }
                FTok {
                    tok: &t.tok,
                    start,
                    end,
                    text,
                    has_comment: comment_spans.iter().any(|&(s, _)| s >= start && s < end),
                }
            })
            .collect();
        let n = toks.len();
        let mut mate = vec![None; n];
        let mut stack = Vec::new();
        for (i, t) in toks.iter().enumerate() {
            if is_opener(t.tok) {
                stack.push(i);
            } else if is_closer(t.tok)
                && let Some(o) = stack.pop()
            {
                mate[o] = Some(i);
                mate[i] = Some(o);
            }
        }
        let mut f = Formatter {
            toks,
            mate,
            space_before: vec![false; n],
            newline_before: vec![false; n],
            items_before: vec![Vec::new(); n],
            trailing: vec![None; n],
            items_end: Vec::new(),
            info: Info::default(),
            forced: HashSet::new(),
            commas: HashSet::new(),
            width: WIDTH,
            shebang: src.trim_start_matches('\u{feff}').starts_with("#!"),
        };
        f.scan_gaps(src);
        let mut walk = Walk {
            src,
            info: Info::default(),
            req: Requests::default(),
        };
        walk.stmts(&prog.stmts);
        f.info = walk.info;
        f.resolve(&walk.req);
        f
    }

    /// Read the whitespace and comments between tokens.
    fn scan_gaps(&mut self, src: &str) {
        let n = self.toks.len();
        for i in 0..=n {
            let from = if i == 0 { 0 } else { self.toks[i - 1].end };
            let to = if i == n {
                src.len()
            } else {
                self.toks[i].start
            };
            let gap = &src[from..to];
            let segs: Vec<&str> = gap.split('\n').collect();
            let mut items = Vec::new();
            let own_start = if i == 0 {
                0
            } else {
                // The first segment is the rest of the previous token's line.
                if let Some(h) = segs[0].find('#') {
                    let at = from + h;
                    let line_start = src[..at].rfind('\n').map_or(0, |j| j + 1);
                    let column = src[line_start..at].chars().count();
                    let text = segs[0][h..].trim_end().to_string();
                    self.trailing[i - 1] = Some((column, text));
                }
                1
            };
            // The last segment holds the indentation of token `i`, unless
            // this is the end of the file.
            let own_end = if i == n { segs.len() } else { segs.len() - 1 };
            for seg in segs.iter().take(own_end).skip(own_start) {
                let t = seg.trim();
                items.push(if t.is_empty() {
                    Item::Blank
                } else {
                    Item::Comment(t.to_string())
                });
            }
            if i == n {
                self.items_end = items;
            } else {
                self.items_before[i] = items;
                self.newline_before[i] = segs.len() > 1;
                self.space_before[i] = !gap.is_empty();
            }
        }
    }

    fn first_at_or_after(&self, off: usize) -> Option<usize> {
        let i = self.toks.partition_point(|t| t.start < off);
        (i < self.toks.len()).then_some(i)
    }

    fn resolve(&mut self, req: &Requests) {
        for &off in &req.pipe_after {
            if let Some(i) = self.first_at_or_after(off)
                && matches!(self.toks[i].tok, Tok::Pipe)
            {
                self.info.pipes.insert(self.toks[i].start);
            }
        }
        for &off in &req.minus_after {
            if let Some(i) = self.first_at_or_after(off)
                && matches!(self.toks[i].tok, Tok::Minus)
            {
                self.info.minus_binary.insert(self.toks[i].start);
            }
        }
        for &off in &req.paren_at {
            if let Some(i) = self.first_at_or_after(off)
                && self.toks[i].start == off
                && matches!(self.toks[i].tok, Tok::LParen)
            {
                self.info.comma_lists.insert(off);
            }
        }
        for &off in &req.paren_after {
            if let Some(i) = self.first_at_or_after(off)
                && matches!(self.toks[i].tok, Tok::LParen)
            {
                self.info.comma_lists.insert(self.toks[i].start);
            }
        }
        for &off in &req.method_after {
            if let Some(i) = self.first_at_or_after(off)
                && i + 2 < self.toks.len()
                && matches!(self.toks[i].tok, Tok::Dot)
                && matches!(self.toks[i + 1].tok, Tok::Ident(_))
                && matches!(self.toks[i + 2].tok, Tok::LParen)
                && self.toks[i + 2].start == self.toks[i + 1].end
            {
                self.info.comma_lists.insert(self.toks[i + 2].start);
            }
        }
        for &(from, to) in &req.needs_between {
            let mut i = self.first_at_or_after(from).unwrap_or(self.toks.len());
            while i + 1 < self.toks.len() && self.toks[i].start < to {
                if matches!(self.toks[i].tok, Tok::Ident(w) if w == "needs")
                    && matches!(self.toks[i + 1].tok, Tok::LBracket)
                {
                    self.info.comma_lists.insert(self.toks[i + 1].start);
                }
                i += 1;
            }
        }
        // The function form of `run`: `run(` and `run!(`.
        for i in 1..self.toks.len() {
            if matches!(self.toks[i - 1].tok, Tok::RunFn { .. })
                && matches!(self.toks[i].tok, Tok::LParen)
            {
                self.info.comma_lists.insert(self.toks[i].start);
            }
        }
    }

    /// Whether the `{` at token `i` is a record or record pattern.
    fn recordish(&self, i: usize) -> bool {
        let s = self.toks[i].start;
        self.info.record_exprs.contains(&s) || self.info.record_pats.contains(&s)
    }

    /// Whether there is a space between tokens `b - 1` and `b` on one line.
    fn space(&self, b: usize) -> bool {
        let a = b - 1;
        let (ta, tb) = (self.toks[a].tok, self.toks[b].tok);
        let keep = self.space_before[b];
        let minus_unary = |i: usize| self.info.minus_unary.contains(&self.toks[i].start);
        let minus_binary = |i: usize| self.info.minus_binary.contains(&self.toks[i].start);
        // A name `run` starts a command when a space follows it, unless it
        // is glued to a `.` or `-` before it (SPEC.md 3.4). So the spacing
        // right after `run`, and around a `-` glued to it, stays as written.
        let is_run = |i: usize| matches!(self.toks[i].tok, Tok::Ident(w) if w == "run");
        if is_run(a) {
            return keep;
        }
        if is_run(b) && !keep {
            return false;
        }
        if matches!(tb, Tok::Minus)
            && b + 1 < self.toks.len()
            && is_run(b + 1)
            && !self.space_before[b + 1]
        {
            return keep;
        }
        // The other way round: a command or `run(` after a unary `-`
        // (`- run a`) was only lexed as one because of the space, and
        // `-run` would be a name.
        if matches!(ta, Tok::Minus) && matches!(tb, Tok::Command(_) | Tok::RunFn { .. }) {
            return keep;
        }
        if matches!(tb, Tok::Comma) {
            return false;
        }
        if matches!(ta, Tok::Comma) {
            return true;
        }
        // `.` decides between a field (`a.b`) and an implicit closure
        // argument (`sort .name`); keep it as written.
        if matches!(ta, Tok::Dot) || matches!(tb, Tok::Dot) {
            return keep;
        }
        if matches!(ta, Tok::LParen | Tok::LBracket) {
            return false;
        }
        if matches!(tb, Tok::RParen | Tok::RBracket) {
            return false;
        }
        if is_spaced_op(ta) || (matches!(ta, Tok::Minus) && minus_binary(a)) {
            return true;
        }
        // `f(x)` is a call and `f (x)` is not; `xs[0]` indexes.
        if matches!(tb, Tok::LParen | Tok::LBracket) {
            return keep;
        }
        if matches!(ta, Tok::Ellipsis) {
            return false;
        }
        if matches!(tb, Tok::Colon) {
            return false;
        }
        if matches!(ta, Tok::Colon) {
            return true;
        }
        if matches!(tb, Tok::Question) {
            return keep;
        }
        if matches!(ta, Tok::LBrace) {
            return !matches!(tb, Tok::RBrace) && !self.recordish(a);
        }
        if matches!(tb, Tok::RBrace) {
            return match self.mate[b] {
                Some(o) => !self.recordish(o),
                None => keep,
            };
        }
        if matches!(tb, Tok::LBrace) {
            return true;
        }
        if is_spaced_op(tb) || (matches!(tb, Tok::Minus) && minus_binary(b)) {
            return true;
        }
        if matches!(ta, Tok::Minus) && minus_unary(a) {
            return false;
        }
        keep
    }

    fn lines(&self) -> Vec<Line> {
        let mut lines: Vec<Line> = Vec::new();
        for i in 0..self.toks.len() {
            if i == 0 || self.newline_before[i] || self.forced.contains(&i) {
                lines.push(Line { first: i, last: i });
            } else if let Some(l) = lines.last_mut() {
                l.last = i;
            }
        }
        lines
    }

    /// The indentation of `line` (SPEC.md 17), given the state after the lines before
    /// it, which is updated to include `line`.
    fn indent_step(&self, state: &mut IndentState, line: &Line) -> usize {
        let first = self.toks[line.first].tok;
        let closes_first = is_closer(first);
        let base = if closes_first {
            state.stack.last().copied().unwrap_or(0)
        } else {
            state.stack.last().map_or(0, |i| i + 1)
        };
        let after_list = matches!(
            state.prev_last.map(|i| self.toks[i].tok),
            Some(Tok::Comma | Tok::LParen | Tok::LBracket | Tok::LBrace)
        );
        let method_chain = matches!(first, Tok::Dot)
            && line.first < line.last
            && matches!(self.toks[line.first + 1].tok, Tok::Ident(_) | Tok::Kw(_))
            && !after_list;
        let cont =
            !closes_first && (matches!(first, Tok::Pipe) || method_chain || state.prev_continues);
        let indent = base + usize::from(cont);
        let depth_before = state.stack.len();
        for t in &self.toks[line.first..=line.last] {
            if is_opener(t.tok) {
                state.stack.push(indent);
            } else if is_closer(t.tok) {
                state.stack.pop();
            }
        }
        let last = self.toks[line.last].tok;
        // A trailing operator continues the statement unless the line
        // opened a bracket (the bracket's contents carry the indentation).
        state.prev_continues = continues(last) && state.stack.len() <= depth_before;
        state.prev_last = Some(line.last);
        indent
    }

    /// The code of a line, without indentation and end-of-line comment.
    fn code(&self, line: &Line) -> String {
        let mut s = String::new();
        for i in line.first..=line.last {
            if i > line.first && self.space(i) {
                s.push(' ');
            }
            s.push_str(self.toks[i].text);
            if self.commas.contains(&i) {
                s.push(',');
            }
        }
        s
    }

    /// Whether a line is too long and may be broken.
    fn breakable_long_line(&self, line: &Line, indent: usize) -> bool {
        if self.trailing[line.last].is_some() {
            return false;
        }
        let toks = &self.toks[line.first..=line.last];
        if toks.iter().any(|t| t.multiline() || t.has_comment) {
            return false;
        }
        indent * INDENT.len() + self.code(line).chars().count() > self.width
    }

    /// The breaks that shorten a long line, or `None` if it cannot be broken.
    fn breaks_for(&self, line: &Line) -> Option<(Vec<usize>, Option<usize>)> {
        let mut open: Vec<usize> = Vec::new();
        let mut pipes = Vec::new();
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for i in line.first..=line.last {
            let t = &self.toks[i];
            if open.is_empty()
                && i != line.first
                && matches!(t.tok, Tok::Pipe)
                && self.info.pipes.contains(&t.start)
            {
                pipes.push(i);
            }
            if is_opener(t.tok) {
                if open.is_empty()
                    && let Some(m) = self.mate[i]
                    && m <= line.last
                    && m > i + 1
                    && let Some(kind) = self.group_kind(i)
                    // A list holding a single token (a long string) gains
                    // nothing from being split.
                    && (kind == GroupKind::Block || m > i + 2)
                {
                    groups.push((i, m));
                }
                open.push(i);
            } else if is_closer(t.tok) {
                open.pop();
            }
        }
        if !pipes.is_empty() {
            return Some((pipes, None));
        }
        let mut best: Option<(usize, usize)> = None;
        for &(o, m) in &groups {
            if best.is_none_or(|(bo, bm)| m - o > bm - bo) {
                best = Some((o, m));
            }
        }
        let (o, m) = best?;
        let mut breaks = vec![o + 1];
        let mut comma = None;
        if self.group_kind(o) == Some(GroupKind::Commas) {
            let mut j = o + 1;
            while j < m {
                if matches!(self.toks[j].tok, Tok::Comma) && j + 1 < m {
                    breaks.push(j + 1);
                }
                j = match self.mate[j] {
                    Some(c) if is_opener(self.toks[j].tok) => c + 1,
                    _ => j + 1,
                };
            }
            // No comma after a command: it would become part of the
            // command's last word (`run a 0` would become `run a 0,`).
            if !matches!(self.toks[m - 1].tok, Tok::Comma | Tok::Command(_)) {
                comma = Some(m - 1);
            }
        }
        breaks.push(m);
        Some((breaks, comma))
    }

    fn group_kind(&self, i: usize) -> Option<GroupKind> {
        let s = self.toks[i].start;
        match self.toks[i].tok {
            Tok::LParen | Tok::LBracket if self.info.comma_lists.contains(&s) => {
                Some(GroupKind::Commas)
            }
            Tok::LBrace if self.info.record_exprs.contains(&s) => Some(GroupKind::Commas),
            Tok::LBrace if self.info.blocks.contains(&s) => Some(GroupKind::Block),
            _ => None,
        }
    }

    fn run(mut self) -> String {
        // Lines are visited in order. A long line that can be broken is
        // split, and its pieces are checked right away with their own
        // indentation (which only depends on the lines before), so a line
        // that needs many breaks (a long `else if` chain written on one
        // line) is handled in one pass instead of one pass per break.
        let mut state = IndentState::default();
        let mut lines = Vec::new();
        let mut indents = Vec::new();
        for line in self.lines() {
            let mut pending = VecDeque::from([line]);
            while let Some(piece) = pending.pop_front() {
                let before = state.clone();
                let indent = self.indent_step(&mut state, &piece);
                if self.breakable_long_line(&piece, indent)
                    && let Some((breaks, comma)) = self.breaks_for(&piece)
                {
                    self.forced.extend(breaks);
                    self.commas.extend(comma);
                    state = before;
                    let mut first = piece.first;
                    let mut parts = Vec::new();
                    for i in piece.first + 1..=piece.last {
                        if self.forced.contains(&i) {
                            parts.push(Line { first, last: i - 1 });
                            first = i;
                        }
                    }
                    parts.push(Line {
                        first,
                        last: piece.last,
                    });
                    for p in parts.into_iter().rev() {
                        pending.push_front(p);
                    }
                    continue;
                }
                lines.push(piece);
                indents.push(indent);
            }
        }
        self.render(&lines, &indents)
    }

    fn render(&self, lines: &[Line], indents: &[usize]) -> String {
        let mut out = String::new();
        // Whether a blank line is waiting to be written before the next line.
        let mut blank = false;
        // Whether nothing has been written yet, or the last line ended with
        // an opening bracket: no blank line may follow either.
        let mut no_blank = true;
        for (line, &indent) in lines.iter().zip(indents) {
            let closes_first = is_closer(self.toks[line.first].tok);
            let comment_indent = indent + usize::from(closes_first);
            for item in &self.items_before[line.first] {
                match item {
                    Item::Blank => blank = true,
                    Item::Comment(c) => {
                        let shebang = out.is_empty() && c.starts_with("#!") && self.shebang;
                        let pad = if shebang {
                            String::new()
                        } else {
                            INDENT.repeat(comment_indent)
                        };
                        self.keep_off_first_line(&mut out, c);
                        emit(&mut out, &format!("{pad}{c}"), &mut blank, &mut no_blank);
                    }
                }
            }
            let mut text = INDENT.repeat(indent);
            text.push_str(&self.code(line));
            if let Some((column, c)) = &self.trailing[line.last] {
                // The comment stays in its column, so aligned comments stay
                // aligned; if the code now reaches it, two spaces separate.
                let used = text.rsplit('\n').next().map_or(0, |l| l.chars().count());
                // After a name `run`, a space would start a command, so a
                // comment there stays glued to it as written.
                let after_run = matches!(self.toks[line.last].tok, Tok::Ident(w) if w == "run");
                let pad = if after_run {
                    0
                } else if *column > used {
                    column - used
                } else {
                    2
                };
                text.push_str(&" ".repeat(pad));
                text.push_str(c);
            }
            emit(&mut out, &text, &mut blank, &mut no_blank);
            let last = &self.toks[line.last];
            no_blank = is_opener(last.tok) && self.trailing[line.last].is_none();
        }
        for item in &self.items_end {
            match item {
                Item::Blank => blank = true,
                Item::Comment(c) => {
                    self.keep_off_first_line(&mut out, c);
                    emit(&mut out, c, &mut blank, &mut no_blank);
                }
            }
        }
        if out.is_empty() {
            return out;
        }
        let trimmed = out.trim_end_matches('\n');
        format!("{trimmed}\n")
    }
}

impl Formatter<'_> {
    /// A comment that starts with `#!` but was not the file's shebang (it
    /// was indented, or below a blank line) would become a shebang on the
    /// first line, and stop being a comment; it keeps one blank line above
    /// it instead.
    fn keep_off_first_line(&self, out: &mut String, comment: &str) {
        if out.is_empty() && comment.starts_with("#!") && !self.shebang {
            out.push('\n');
        }
    }
}

/// Write one line, preceded by the pending blank line if one may go here.
fn emit(out: &mut String, text: &str, blank: &mut bool, no_blank: &mut bool) {
    if *blank && !*no_blank {
        out.push('\n');
    }
    *blank = false;
    out.push_str(text);
    out.push('\n');
    *no_blank = false;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum GroupKind {
    /// Elements separated by commas; one per line with a trailing comma.
    Commas,
    /// A one-line block; its statement moves to a line of its own.
    Block,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(src: &str) -> String {
        let once = format_source(src).unwrap();
        assert_eq!(format_source(&once).unwrap(), once, "not idempotent");
        once
    }

    #[test]
    fn spacing_never_turns_a_run_name_into_a_command() {
        // Found by fuzzing: `run--'\t'` became `run - -'\t'`, where the
        // space after `run` starts command mode.
        for src in [
            "run--'\t'\n",
            "a-run+1\n",
            "run+1\n",
            "run==1\n",
            "-run*2\n",
            "x.run+1\n",
        ] {
            let out = fmt(src);
            assert!(!out.contains("run "), "{src:?} became {out:?}");
        }
    }

    #[test]
    fn hashbang_comment_does_not_become_a_shebang() {
        // Found by fuzzing: removing the leading blank line turned the
        // comment into a shebang, which the self-check rejected.
        assert_eq!(fmt("\n#!"), "\n#!\n");
        assert_eq!(fmt("  #! x\nprint(1)\n"), "\n#! x\nprint(1)\n");
        assert_eq!(
            fmt("#!/usr/bin/env ok\n\n\nprint(1)\n"),
            "#!/usr/bin/env ok\n\nprint(1)\n"
        );
        assert_eq!(
            fmt("\u{feff}#!/usr/bin/env ok\nprint(1)\n").trim_start_matches('\u{feff}'),
            "#!/usr/bin/env ok\nprint(1)\n"
        );
    }

    #[test]
    fn keeps_the_space_between_a_minus_and_a_command() {
        // Found by fuzzing: `- run a` lost its space and became `-run a`,
        // where `run` glued to a `-` is a name.
        for src in [
            "- run a | f\n",
            "- run! a\n",
            "- run(\"a\")\n",
            "x - run a\n",
        ] {
            let out = fmt(src);
            assert!(out.contains("- run"), "{src:?} became {out:?}");
            assert_eq!(out, fmt(&out));
        }
    }

    #[test]
    fn no_trailing_comma_after_a_command() {
        // Found by fuzzing: breaking the arguments of a call added a comma
        // after the last one, which joined the command ending it.
        let src = "sumpub(run1 + run / 0 in 2 0)(ysum | z | y)\n";
        let out = super::format_with_width(src, 20).expect("formats");
        assert!(!out.contains("0,"), "{out:?}");
        assert_eq!(out, super::format_with_width(&out, 20).unwrap());
    }

    #[test]
    fn backslash_at_end_of_file_is_a_continuation() {
        // Found by fuzzing: without a final newline the `\` was an argument,
        // and the newline the formatter adds made it a continuation.
        let out = fmt("run  4      \\");
        assert_eq!(out, fmt(&out));
        assert_eq!(fmt("run a \\"), fmt("run a \\\n"));
    }

    #[test]
    fn reindents_blocks_and_pipelines() {
        let src = "fn f(x) {\nif x {\n        print(1)\n}\nlet y = [1, 2]\n| map(v => v)\n\n\n\nprint(y)   \n}\n\n";
        let want = "fn f(x) {\n  if x {\n    print(1)\n  }\n  let y = [1, 2]\n    | map(v => v)\n\n  print(y)\n}\n";
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn keeps_multiline_strings_verbatim() {
        let src = "let s = \"\"\"\n      keep   this  \n  \"\"\"\nprint(s)\n";
        assert_eq!(fmt(src), src);
    }

    #[test]
    fn two_brackets_on_one_line_indent_once() {
        let src = "print(match x {\n    1 => \"a\"\n    _ => \"b\"\n  })\n";
        let want = "print(match x {\n  1 => \"a\"\n  _ => \"b\"\n})\n";
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn normalises_spacing_inside_lines() {
        assert_eq!(fmt("let x=[ 1,2 ]+[3]\n"), "let x = [1, 2] + [3]\n");
        assert_eq!(fmt("let r = { a:1, b : 2 }\n"), "let r = {a: 1, b: 2}\n");
        assert_eq!(fmt("if x {print(1)}\n"), "if x { print(1) }\n");
        assert_eq!(fmt("let y = a-b*-c\n"), "let y = a - b * -c\n");
        assert_eq!(fmt("let y = f( x )?\n"), "let y = f(x)?\n");
        assert_eq!(
            fmt("fn f(  x:int=1 ,--dry-run: bool) {}\n"),
            "fn f(x: int = 1, --dry-run: bool) {}\n"
        );
    }

    #[test]
    fn keeps_significant_whitespace() {
        // Juxtaposed stage arguments and implicit closures.
        assert_eq!(
            fmt("let y = xs | take -1 | sort .name\n"),
            "let y = xs | take -1 | sort .name\n"
        );
        assert_eq!(
            fmt("task reset-db needs [a-b] {\n}\n"),
            "task reset-db needs [a-b] {\n}\n"
        );
    }

    #[test]
    fn keeps_commands_and_comments() {
        let src = "task a {\n  run  cargo   build   # two spaces\n  # own line\n    run x {y}\n}\n";
        let want = "task a {\n  run  cargo   build   # two spaces\n  # own line\n  run x {y}\n}\n";
        assert_eq!(fmt(src), want);
    }

    #[test]
    fn breaks_long_calls_one_argument_per_line() {
        let src = format!(
            "let files = glob(\"{}\", root: root, hidden: true)\n",
            "x".repeat(80)
        );
        let want = format!(
            "let files = glob(\n  \"{}\",\n  root: root,\n  hidden: true,\n)\n",
            "x".repeat(80)
        );
        assert_eq!(fmt(&src), want);
    }

    #[test]
    fn breaks_long_pipelines_before_each_stage() {
        let src = format!(
            "let y = xs | where .{} | map(.name) | sort\n",
            "a".repeat(80)
        );
        let want = format!(
            "let y = xs\n  | where .{}\n  | map(.name)\n  | sort\n",
            "a".repeat(80)
        );
        assert_eq!(fmt(&src), want);
    }

    #[test]
    fn opens_long_one_line_blocks() {
        let src = format!("if x {{ print(\"{}\") }}\n", "a".repeat(90));
        let want = format!("if x {{\n  print(\"{}\")\n}}\n", "a".repeat(90));
        assert_eq!(fmt(&src), want);
        // A block holding only a command is opened up too.
        let src = format!(
            "let r = try {{ run make {} }} catch e {{ null }}\n",
            "a".repeat(80)
        );
        let want = format!(
            "let r = try {{\n  run make {}\n}} catch e {{ null }}\n",
            "a".repeat(80)
        );
        assert_eq!(fmt(&src), want);
        // A single long string is not split out of its call.
        let src = format!("print(\"{}\")\n", "a".repeat(100));
        assert_eq!(fmt(&src), src);
    }

    #[test]
    fn match_alternatives_are_not_pipelines() {
        let src = format!(
            "let k = match x {{ \"{}\" | \"b\" => 1, _ => 2 }}\n",
            "a".repeat(90)
        );
        // The match body is neither a block nor a list, so it stays.
        assert_eq!(fmt(&src), src);
    }

    /// The example of SPEC.md 17.2.
    #[test]
    fn spec_example() {
        let before = "let files = glob(\"**/*\", root: root, hidden: true, exclude: [\"**/.git\", \"**/node_modules\", \"**/target\"])\nlet r = {name:x , size:stat(x).size}\nif r.size>limit {print( \"big\" )}\n";
        let after = "let files = glob(\n  \"**/*\",\n  root: root,\n  hidden: true,\n  exclude: [\"**/.git\", \"**/node_modules\", \"**/target\"],\n)\nlet r = {name: x, size: stat(x).size}\nif r.size > limit { print(\"big\") }\n";
        assert_eq!(fmt(before), after);
    }

    #[test]
    fn refuses_files_that_do_not_parse() {
        assert!(format_source("let = 1\n").is_err());
    }
}
