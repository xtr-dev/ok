//! The parser: tokens to [`ast`](crate::ast) (SPEC.md section 4).
//!
//! A hand-written recursive-descent parser with one precedence function per
//! level of SPEC.md 4.4. Interpolations and command words arrive from the
//! lexer as nested token lists and are parsed by sub-parsers over those
//! lists. The parser never panics; nesting depth is limited so that hostile
//! input produces an error rather than a stack overflow.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ast::*;
use crate::error::{ErrKind, OkError, suggest};
use crate::lexer::{self, CommandTok, Comment, Kw, StrPart, Tok, Token, WordPart};
use crate::source::{FileId, Span};

/// Maximum nesting of expressions and blocks.
const MAX_DEPTH: usize = 150;

/// Parse a whole file.
pub fn parse(file: FileId, src: &str) -> Result<Program, OkError> {
    let lexed = lexer::lex(file, src)?;
    let mut line_starts = vec![0];
    for (i, b) in src.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    let ctx = Rc::new(Ctx::new(src, lexed.comments, line_starts));
    let mut p = Parser::new(file, lexed.tokens, ctx.clone(), 0);
    let stmts = p.program()?;
    Ok(Program {
        file,
        stmts,
        doc: ctx.file_doc(),
    })
}

/// Shared, read-only information for the parser and its sub-parsers.
struct Ctx {
    src_starts_with_shebang: bool,
    /// Own-line comments by 1-based line number.
    own_line: HashMap<usize, String>,
    /// Comments that follow code on their line, by 1-based line number.
    trailing: HashMap<usize, String>,
    line_starts: Vec<usize>,
    /// Line of the first token, to find the file's leading comment block.
    first_code_line: std::cell::Cell<usize>,
}

impl Ctx {
    fn new(src: &str, comments: Vec<Comment>, line_starts: Vec<usize>) -> Ctx {
        let mut ctx = Ctx {
            src_starts_with_shebang: src.trim_start_matches('\u{feff}').starts_with("#!"),
            own_line: HashMap::new(),
            trailing: HashMap::new(),
            line_starts,
            first_code_line: std::cell::Cell::new(usize::MAX),
        };
        for c in comments {
            let line = ctx.line_of(c.span.start as usize);
            if c.own_line {
                ctx.own_line.insert(line, c.text);
            } else {
                ctx.trailing.insert(line, c.text);
            }
        }
        ctx
    }

    fn line_of(&self, offset: usize) -> usize {
        match self.line_starts.binary_search(&offset) {
            Ok(i) => i + 1,
            Err(i) => i,
        }
    }

    /// The doc comment directly above `line` (SPEC.md 3.2).
    fn doc_above(&self, line: usize) -> Option<String> {
        let mut lines = Vec::new();
        let mut l = line;
        while l > 1 {
            l -= 1;
            if l == 1 && self.src_starts_with_shebang {
                break;
            }
            match self.own_line.get(&l) {
                Some(text) => lines.push(text.clone()),
                None => break,
            }
        }
        if lines.is_empty() {
            return None;
        }
        lines.reverse();
        Some(lines.join("\n"))
    }

    /// The first comment block of the file, after a shebang, if it comes
    /// before any code.
    fn file_doc(&self) -> Option<String> {
        let mut l = if self.src_starts_with_shebang { 2 } else { 1 };
        let mut lines = Vec::new();
        while l < self.first_code_line.get() {
            match self.own_line.get(&l) {
                Some(text) => lines.push(text.clone()),
                None if lines.is_empty() => {}
                None => break,
            }
            l += 1;
            if l > self.line_starts.len() {
                break;
            }
        }
        // Drop a trailing blank comment line and keep the block as written.
        while lines.last().is_some_and(|s| s.is_empty()) {
            lines.pop();
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\n"))
        }
    }
}

struct Parser {
    file: FileId,
    toks: Vec<Token>,
    pos: usize,
    ctx: Rc<Ctx>,
    depth: usize,
    /// When set, `{` at the outermost level of an expression starts a block,
    /// not a record (the heads of `if`, `while`, `for` and `match`).
    nostruct: bool,
    loop_depth: usize,
    fn_depth: usize,
}

type PResult<T> = Result<T, OkError>;

fn syntax(span: Span, msg: impl Into<String>) -> OkError {
    OkError::at(ErrKind::Syntax, span, msg)
}

impl Parser {
    fn new(file: FileId, toks: Vec<Token>, ctx: Rc<Ctx>, depth: usize) -> Parser {
        Parser {
            file,
            toks,
            pos: 0,
            ctx,
            depth,
            nostruct: false,
            loop_depth: 0,
            fn_depth: 0,
        }
    }

    /// A parser over a nested token list (an interpolation or command word).
    fn sub(&self, mut tokens: Vec<Token>, end: Span) -> Parser {
        tokens.push(Token {
            tok: Tok::Eof,
            span: Span {
                file: self.file,
                start: end.end,
                end: end.end,
            },
            space_before: true,
        });
        let mut p = Parser::new(self.file, tokens, self.ctx.clone(), self.depth);
        p.loop_depth = self.loop_depth;
        p.fn_depth = self.fn_depth;
        p
    }

    // ----- token access -------------------------------------------------------

    fn peek(&self) -> &Tok {
        &self.toks[self.pos.min(self.toks.len() - 1)].tok
    }

    fn peek_token(&self) -> &Token {
        &self.toks[self.pos.min(self.toks.len() - 1)]
    }

    fn peek_at(&self, n: usize) -> &Token {
        &self.toks[(self.pos + n).min(self.toks.len() - 1)]
    }

    fn span(&self) -> Span {
        self.peek_token().span
    }

    fn prev_span(&self) -> Span {
        if self.pos == 0 {
            self.span()
        } else {
            self.toks[self.pos - 1].span
        }
    }

    fn bump(&mut self) -> Token {
        let t = self.peek_token().clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }

    fn at(&self, tok: &Tok) -> bool {
        self.peek() == tok
    }

    fn eat(&mut self, tok: &Tok) -> bool {
        if self.at(tok) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, tok: &Tok, what: &str) -> PResult<Token> {
        if self.at(tok) {
            Ok(self.bump())
        } else {
            Err(self.unexpected(what))
        }
    }

    fn unexpected(&self, what: &str) -> OkError {
        let t = self.peek_token();
        syntax(
            t.span,
            format!("expected {what}, found {}", t.tok.describe()),
        )
    }

    fn skip_newlines(&mut self) {
        while self.at(&Tok::Newline) {
            self.bump();
        }
    }

    fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(syntax(self.span(), "expression is nested too deeply"));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth -= 1;
    }

    fn with_nostruct<T>(
        &mut self,
        on: bool,
        f: impl FnOnce(&mut Self) -> PResult<T>,
    ) -> PResult<T> {
        let saved = self.nostruct;
        self.nostruct = on;
        let r = f(self);
        self.nostruct = saved;
        r
    }

    // ----- statements ---------------------------------------------------------

    fn program(&mut self) -> PResult<Vec<Stmt>> {
        self.skip_newlines();
        let first_line = self.ctx.line_of(self.span().start as usize);
        if !self.at(&Tok::Eof) {
            self.ctx.first_code_line.set(first_line);
        }
        let mut stmts = Vec::new();
        while !self.at(&Tok::Eof) {
            let stmt = self.statement(true)?;
            stmts.push(stmt);
            self.end_of_statement(stmts.last())?;
        }
        Ok(stmts)
    }

    /// After a statement: a newline, or the `}` / end of file that closes it.
    fn end_of_statement(&mut self, last: Option<&Stmt>) -> PResult<()> {
        match self.peek() {
            Tok::Newline => {
                self.skip_newlines();
                Ok(())
            }
            Tok::Eof | Tok::RBrace => Ok(()),
            _ => {
                let mut e = self.unexpected("the end of the statement (a new line)");
                if let Some(Stmt {
                    kind:
                        StmtKind::Expr(Expr {
                            kind: ExprKind::Ident(name),
                            ..
                        }),
                    ..
                }) = last
                    && matches!(
                        self.peek(),
                        Tok::Str(_) | Tok::Int(_) | Tok::Float(_) | Tok::Ident(_)
                    )
                {
                    e = e.with_help(format!("calls need parentheses: {name}(...)"));
                }
                Err(e)
            }
        }
    }

    fn statement(&mut self, top: bool) -> PResult<Stmt> {
        let start = self.span();
        let kind = match self.peek().clone() {
            Tok::Kw(Kw::Let) | Tok::Kw(Kw::Var) => {
                let mutable = self.at(&Tok::Kw(Kw::Var));
                self.bump();
                let pattern = self.pattern()?;
                self.expect(&Tok::Assign, "`=`")?;
                let value = self.arg_expr()?;
                StmtKind::Let {
                    pattern,
                    value,
                    mutable,
                }
            }
            Tok::Kw(Kw::Fn) if matches!(self.peek_at(1).tok, Tok::Ident(_)) => {
                StmtKind::Fn(Rc::new(self.fn_decl()?))
            }
            Tok::Kw(Kw::Task) => {
                let decl = self.task_decl()?;
                if !top {
                    return Err(syntax(
                        start,
                        "tasks can only be declared at the top level of a project file",
                    ));
                }
                StmtKind::Task(Rc::new(decl))
            }
            Tok::Kw(Kw::Import) => {
                self.bump();
                let (path, path_span) =
                    self.plain_string("the path of the file to import, in quotes")?;
                let alias = if self.eat(&Tok::Kw(Kw::As)) {
                    let t = self.bump();
                    match t.tok {
                        Tok::Ident(name) => Some((name, t.span)),
                        other => {
                            return Err(syntax(
                                t.span,
                                format!("expected a name after `as`, found {}", other.describe()),
                            ));
                        }
                    }
                } else {
                    None
                };
                if !top {
                    return Err(syntax(
                        start,
                        "`import` is only allowed at the top level of a file",
                    ));
                }
                StmtKind::Import {
                    path,
                    path_span,
                    alias,
                }
            }
            Tok::Kw(Kw::Requires) => {
                self.bump();
                let (version, _) = self.plain_string("a version in quotes, like \"0.3\"")?;
                if !top {
                    return Err(syntax(
                        start,
                        "`requires` is only allowed at the top level of a project file",
                    ));
                }
                StmtKind::Requires { version }
            }
            Tok::Ident(ref w)
                if w == "env"
                    && top
                    && matches!(&self.peek_at(1).tok, Tok::Ident(f) if f == "file") =>
            {
                self.bump();
                self.bump();
                let (path, _) = self.plain_string("the path of the .env file, in quotes")?;
                StmtKind::EnvFile { path }
            }
            Tok::Kw(Kw::For) => self.for_stmt()?,
            Tok::Kw(Kw::While) => {
                self.bump();
                let cond = self.with_nostruct(true, |p| p.expr())?;
                self.check_no_it(&cond)?;
                self.loop_depth += 1;
                let body = self.block();
                self.loop_depth -= 1;
                StmtKind::While { cond, body: body? }
            }
            Tok::Kw(Kw::Break) | Tok::Kw(Kw::Continue) => {
                let is_break = self.at(&Tok::Kw(Kw::Break));
                let t = self.bump();
                if self.loop_depth == 0 {
                    return Err(syntax(
                        t.span,
                        format!(
                            "`{}` outside of a loop",
                            if is_break { "break" } else { "continue" }
                        ),
                    ));
                }
                if is_break {
                    StmtKind::Break
                } else {
                    StmtKind::Continue
                }
            }
            Tok::Kw(Kw::Return) => {
                let t = self.bump();
                if self.fn_depth == 0 {
                    return Err(syntax(t.span, "`return` outside of a function")
                        .with_help("to stop a script early, use exit(code)"));
                }
                let value = if matches!(self.peek(), Tok::Newline | Tok::Eof | Tok::RBrace) {
                    None
                } else {
                    let e = self.expr()?;
                    self.check_no_it(&e)?;
                    Some(e)
                };
                StmtKind::Return(value)
            }
            Tok::Ident(ref w)
                if (w == "function" || w == "def" || w == "func")
                    && matches!(self.peek_at(1).tok, Tok::Ident(_)) =>
            {
                return Err(syntax(start, format!("`{w}` is not an ok keyword"))
                    .with_help("declare functions with fn: fn name(x) { ... }"));
            }
            Tok::Ident(ref w) if w == "elif" || w == "elsif" || w == "elseif" => {
                return Err(syntax(start, format!("`{w}` is not an ok keyword"))
                    .with_help("write `else if`"));
            }
            Tok::Ident(ref w) if w == "const" && matches!(self.peek_at(1).tok, Tok::Ident(_)) => {
                return Err(syntax(start, "`const` is not an ok keyword")
                    .with_help("use `let` for bindings that never change"));
            }
            _ => {
                let e = self.expr()?;
                if matches!(self.peek(), Tok::Assign | Tok::PlusEq | Tok::MinusEq) {
                    let op = match self.bump().tok {
                        Tok::Assign => AssignOp::Set,
                        Tok::PlusEq => AssignOp::Add,
                        _ => AssignOp::Sub,
                    };
                    let target = self.place(e)?;
                    let value = self.arg_expr()?;
                    StmtKind::Assign { target, op, value }
                } else {
                    self.check_no_it(&e)?;
                    StmtKind::Expr(e)
                }
            }
        };
        Ok(Stmt {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    fn plain_string(&mut self, what: &str) -> PResult<(String, Span)> {
        let t = self.peek_token().clone();
        if let Tok::Str(parts) = &t.tok {
            if let [StrPart::Lit(s)] = parts.as_slice() {
                self.bump();
                return Ok((s.clone(), t.span));
            }
            return Err(syntax(t.span, "this string cannot contain interpolation"));
        }
        Err(self.unexpected(what))
    }

    fn place(&self, e: Expr) -> PResult<Place> {
        let mut path = Vec::new();
        let mut cur = e;
        loop {
            match cur.kind {
                ExprKind::Ident(name) => {
                    path.reverse();
                    return Ok(Place {
                        root: name,
                        root_span: cur.span,
                        path,
                    });
                }
                ExprKind::Field {
                    expr,
                    name,
                    name_span,
                } => {
                    path.push(PlaceSeg::Field(name, name_span));
                    cur = *expr;
                }
                ExprKind::Index { expr, index } => {
                    path.push(PlaceSeg::Index(*index));
                    cur = *expr;
                }
                _ => {
                    return Err(
                        syntax(cur.span, "cannot assign to this expression").with_help(
                            "assign to a `var` name, a field (cfg.port) or an element (xs[0])",
                        ),
                    );
                }
            }
        }
    }

    fn for_stmt(&mut self) -> PResult<StmtKind> {
        self.bump();
        let first = self.pattern()?;
        let second = if self.eat(&Tok::Comma) {
            Some(self.pattern()?)
        } else {
            None
        };
        self.expect(&Tok::Kw(Kw::In), "`in`")?;
        let iter = self.with_nostruct(true, |p| p.expr())?;
        self.check_no_it(&iter)?;
        self.loop_depth += 1;
        let body = self.block();
        self.loop_depth -= 1;
        Ok(StmtKind::For {
            first,
            second,
            iter,
            body: body?,
        })
    }

    fn pattern(&mut self) -> PResult<Pattern> {
        let t = self.peek_token().clone();
        match t.tok {
            Tok::Ident(name) => {
                self.bump();
                Ok(Pattern::Name(name, t.span))
            }
            Tok::LBracket => {
                self.bump();
                let mut items = Vec::new();
                let mut rest = None;
                while !self.at(&Tok::RBracket) {
                    if self.eat(&Tok::Ellipsis) {
                        let n = self.bump();
                        let Tok::Ident(name) = n.tok else {
                            return Err(syntax(n.span, "expected a name after `...`"));
                        };
                        rest = Some((name, n.span));
                        self.eat(&Tok::Comma);
                        if !self.at(&Tok::RBracket) {
                            return Err(syntax(
                                self.span(),
                                "`...rest` must be the last item of a list pattern",
                            ));
                        }
                        break;
                    }
                    items.push(self.pattern()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let end = self.expect(&Tok::RBracket, "`]`")?;
                Ok(Pattern::List {
                    items,
                    rest,
                    span: t.span.to(end.span),
                })
            }
            Tok::LBrace => {
                self.bump();
                self.skip_newlines();
                let mut fields = Vec::new();
                while !self.at(&Tok::RBrace) {
                    let n = self.bump();
                    let name = match n.tok {
                        Tok::Ident(name) => name,
                        Tok::Kw(k) => k.as_str().to_string(),
                        other => {
                            return Err(syntax(
                                n.span,
                                format!("expected a field name, found {}", other.describe()),
                            ));
                        }
                    };
                    let pat = if self.eat(&Tok::Colon) {
                        self.pattern()?
                    } else {
                        Pattern::Name(name.clone(), n.span)
                    };
                    fields.push((name, n.span, pat));
                    self.skip_newlines();
                    if !self.eat(&Tok::Comma) {
                        self.skip_newlines();
                        break;
                    }
                    self.skip_newlines();
                }
                let end = self.expect(&Tok::RBrace, "`}`")?;
                Ok(Pattern::Record {
                    fields,
                    span: t.span.to(end.span),
                })
            }
            _ => Err(self.unexpected("a name or a [list] / {record} pattern")),
        }
    }

    fn block(&mut self) -> PResult<Block> {
        self.enter()?;
        let open = self.expect(&Tok::LBrace, "`{`")?;
        self.skip_newlines();
        let mut stmts = Vec::new();
        while !self.at(&Tok::RBrace) {
            if self.at(&Tok::Eof) {
                self.leave();
                return Err(syntax(open.span, "this block is never closed with `}`"));
            }
            let s = self.statement(false)?;
            stmts.push(s);
            self.end_of_statement(stmts.last())?;
        }
        let close = self.bump();
        self.leave();
        Ok(Block {
            stmts,
            span: open.span.to(close.span),
        })
    }

    // ----- declarations -------------------------------------------------------

    fn fn_decl(&mut self) -> PResult<FnDecl> {
        let start = self.bump().span;
        let name_tok = self.bump();
        let Tok::Ident(name) = name_tok.tok else {
            return Err(internal());
        };
        let doc = self.ctx.doc_above(self.ctx.line_of(start.start as usize));
        self.expect(&Tok::LParen, "`(` after the function name")?;
        let params = self.params(&Tok::RParen)?;
        let body = self.fn_body()?;
        Ok(FnDecl {
            name,
            name_span: name_tok.span,
            params,
            span: start.to(body.span),
            body,
            doc,
        })
    }

    fn fn_body(&mut self) -> PResult<Block> {
        let saved_loop = self.loop_depth;
        self.loop_depth = 0;
        self.fn_depth += 1;
        let body = self.block();
        self.fn_depth -= 1;
        self.loop_depth = saved_loop;
        body
    }

    /// Parameters up to and including `close` (SPEC.md 4.2).
    fn params(&mut self, close: &Tok) -> PResult<Vec<Param>> {
        let mut params: Vec<Param> = Vec::new();
        loop {
            self.skip_newlines();
            if self.eat(close) {
                break;
            }
            let start = self.span();
            let mut flag = false;
            let mut rest = false;
            let cli_name;
            if self.eat(&Tok::Ellipsis) {
                rest = true;
                let t = self.bump();
                let Tok::Ident(n) = t.tok else {
                    return Err(syntax(t.span, "expected a parameter name after `...`"));
                };
                cli_name = n;
            } else if self.at(&Tok::Minus)
                && self.peek_at(1).tok == Tok::Minus
                && !self.peek_at(1).space_before
            {
                self.bump();
                self.bump();
                flag = true;
                cli_name = self.dashed_name("a flag name after `--`")?;
            } else {
                let t = self.bump();
                match t.tok {
                    Tok::Ident(n) => cli_name = n,
                    other => {
                        return Err(syntax(
                            t.span,
                            format!("expected a parameter name, found {}", other.describe()),
                        ));
                    }
                }
            }
            let ty = if self.eat(&Tok::Colon) {
                Some(self.type_ann()?)
            } else {
                None
            };
            let default = if self.eat(&Tok::Assign) {
                if rest {
                    return Err(syntax(
                        self.prev_span(),
                        "a rest parameter cannot have a default",
                    ));
                }
                let e = self.with_nostruct(false, |p| p.coalesce())?;
                self.check_no_it(&e)?;
                Some(e)
            } else {
                None
            };
            let end_span = self.prev_span();
            let name = cli_name.replace('-', "_");
            if let Some(prev) = params.iter().find(|p| p.name == name) {
                return Err(syntax(
                    start.to(end_span),
                    format!("parameter `{}` is declared twice", prev.cli_name),
                ));
            }
            if rest && params.iter().any(|p| p.rest) {
                return Err(syntax(start, "a function can have only one rest parameter"));
            }
            let mut param = Param {
                name,
                cli_name,
                flag,
                rest,
                ty,
                default,
                help: None,
                span: start.to(end_span),
            };
            let more = self.eat(&Tok::Comma);
            let line = self.ctx.line_of(self.prev_span().end as usize);
            param.help = self.ctx.trailing.get(&line).cloned();
            params.push(param);
            if !more {
                self.skip_newlines();
                self.expect(close, &format!("`,` or {}", close.describe()))?;
                break;
            }
        }
        // A help comment belongs to one parameter only: if two parameters
        // share a line, neither gets it.
        for i in 1..params.len() {
            let a = self.ctx.line_of(params[i - 1].span.end as usize);
            let b = self.ctx.line_of(params[i].span.end as usize);
            if a == b {
                params[i - 1].help = None;
                params[i].help = None;
            }
        }
        Ok(params)
    }

    /// A name made of words joined by `-` with no spaces: `dry-run`,
    /// `reset-db`, `step-2`. Keywords are allowed as words.
    fn dashed_name(&mut self, what: &str) -> PResult<String> {
        let t = self.bump();
        let mut name = match t.tok {
            Tok::Ident(n) => n,
            Tok::Kw(k) => k.as_str().to_string(),
            other => {
                return Err(syntax(
                    t.span,
                    format!("expected {what}, found {}", other.describe()),
                ));
            }
        };
        while self.at(&Tok::Minus)
            && !self.peek_token().space_before
            && !self.peek_at(1).space_before
        {
            let word = match &self.peek_at(1).tok {
                Tok::Ident(n) => n.clone(),
                Tok::Kw(k) => k.as_str().to_string(),
                Tok::Int(i) if *i >= 0 => i.to_string(),
                _ => break,
            };
            self.bump();
            self.bump();
            name.push('-');
            name.push_str(&word);
        }
        Ok(name)
    }

    fn type_ann(&mut self) -> PResult<TypeAnn> {
        let t = self.bump();
        let base = match &t.tok {
            Tok::Kw(Kw::Fn) => TypeAnn::Fn,
            Tok::Kw(Kw::Null) => TypeAnn::Null,
            Tok::Ident(n) => match n.as_str() {
                "any" => TypeAnn::Any,
                "bool" => TypeAnn::Bool,
                "int" => TypeAnn::Int,
                "float" => TypeAnn::Float,
                "string" => TypeAnn::String,
                "bytes" => TypeAnn::Bytes,
                "path" => TypeAnn::Path,
                "duration" => TypeAnn::Duration,
                "time" => TypeAnn::Time,
                "record" => TypeAnn::Record,
                "list" => {
                    if self.at(&Tok::LBracket) && !self.peek_token().space_before {
                        self.bump();
                        let inner = self.type_ann()?;
                        self.expect(&Tok::RBracket, "`]`")?;
                        TypeAnn::List(Some(Box::new(inner)))
                    } else {
                        TypeAnn::List(None)
                    }
                }
                other => {
                    let names = [
                        "any", "bool", "int", "float", "string", "bytes", "path", "duration",
                        "time", "record", "list", "fn", "null",
                    ];
                    let help = match other {
                        "str" | "String" | "text" => Some("use `string`".to_string()),
                        "number" | "num" => Some("use `int` or `float`".to_string()),
                        "boolean" => Some("use `bool`".to_string()),
                        "dict" | "map" | "object" => Some("use `record`".to_string()),
                        _ => suggest(other, names).map(|s| format!("did you mean `{s}`?")),
                    };
                    return Err(syntax(t.span, format!("unknown type `{other}`")).maybe_help(help));
                }
            },
            other => {
                return Err(syntax(
                    t.span,
                    format!("expected a type, found {}", other.describe()),
                ));
            }
        };
        if self.eat(&Tok::Question) {
            return Ok(TypeAnn::Optional(Box::new(base)));
        }
        Ok(base)
    }

    fn task_decl(&mut self) -> PResult<TaskDecl> {
        let start = self.bump().span;
        let name_span = self.span();
        let name = self.dashed_name("a task name")?;
        let name_span = name_span.to(self.prev_span());
        let doc = self.ctx.doc_above(self.ctx.line_of(start.start as usize));
        let params = if self.eat(&Tok::LParen) {
            self.params(&Tok::RParen)?
        } else {
            Vec::new()
        };
        let mut needs = None;
        let mut sources = None;
        let mut outputs = None;
        let mut confirm = None;
        let mut hidden = false;
        loop {
            let t = self.peek_token().clone();
            let word = match &t.tok {
                Tok::LBrace => break,
                Tok::Ident(w) => w.clone(),
                _ => {
                    return Err(self.unexpected(
                        "a task modifier (needs, sources, outputs, confirm, hidden) or `{`",
                    ));
                }
            };
            self.bump();
            let dup = || syntax(t.span, format!("`{word}` is given twice"));
            match word.as_str() {
                "needs" => {
                    if needs.is_some() {
                        return Err(dup());
                    }
                    self.expect(&Tok::LBracket, "`[` after needs")?;
                    let mut list = Vec::new();
                    while !self.at(&Tok::RBracket) {
                        let s = self.span();
                        let mut n = self.dashed_name("a task name")?;
                        if self.at(&Tok::Dot) && !self.peek_token().space_before {
                            self.bump();
                            n.push('.');
                            n.push_str(&self.dashed_name("a task name after `.`")?);
                        }
                        list.push((n, s.to(self.prev_span())));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RBracket, "`]`")?;
                    needs = Some(list);
                }
                "sources" | "outputs" => {
                    let slot = if word == "sources" {
                        &sources
                    } else {
                        &outputs
                    };
                    if slot.is_some() {
                        return Err(dup());
                    }
                    if !self.at(&Tok::LBracket) {
                        return Err(
                            self.unexpected(&format!("a list of glob patterns after {word}"))
                        );
                    }
                    let e = self.with_nostruct(false, |p| p.primary())?;
                    if word == "sources" {
                        sources = Some(e);
                    } else {
                        outputs = Some(e);
                    }
                }
                "confirm" => {
                    if confirm.is_some() {
                        return Err(dup());
                    }
                    confirm = Some(self.plain_string("the confirmation text in quotes")?.0);
                }
                "hidden" => {
                    if hidden {
                        return Err(dup());
                    }
                    hidden = true;
                }
                "allow" => {
                    return Err(OkError::at(
                        ErrKind::Syntax,
                        t.span,
                        "task permissions (allow) are not implemented yet",
                    )
                    .with_note("the syntax is reserved for a later version of ok"));
                }
                other => {
                    let help = suggest(other, ["needs", "sources", "outputs", "confirm", "hidden"])
                        .map(|s| format!("did you mean `{s}`?"));
                    return Err(
                        syntax(t.span, format!("unknown task modifier `{other}`")).maybe_help(help)
                    );
                }
            }
        }
        let body = self.fn_body()?;
        Ok(TaskDecl {
            name,
            name_span,
            params,
            needs: needs.unwrap_or_default(),
            sources,
            outputs,
            confirm,
            hidden,
            span: start.to(body.span),
            body,
            doc,
        })
    }

    // ----- implicit closures ----------------------------------------------------

    /// An expression at an argument boundary (SPEC.md 6.9): if it uses `.x`
    /// shorthand, the whole expression becomes a one-argument closure.
    fn arg_expr(&mut self) -> PResult<Expr> {
        let e = self.with_nostruct(false, |p| p.expr())?;
        Ok(wrap_it(e))
    }

    fn arg_coalesce(&mut self) -> PResult<Expr> {
        let e = self.with_nostruct(false, |p| p.coalesce())?;
        Ok(wrap_it(e))
    }

    fn check_no_it(&self, e: &Expr) -> PResult<()> {
        if let Some(span) = find_it(e) {
            return Err(syntax(span, "`.name` shorthand needs an argument to belong to")
                .with_help("use it as an argument, like where(.active) or | sort .name, or write a closure: x => x.name"));
        }
        Ok(())
    }

    // ----- expressions ----------------------------------------------------------

    fn expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let r = self.pipeline();
        self.leave();
        r
    }

    fn pipeline(&mut self) -> PResult<Expr> {
        let mut lhs = self.coalesce()?;
        while self.at(&Tok::Pipe) {
            self.bump();
            let stage = self.stage()?;
            let span = lhs.span.to(self.prev_span());
            lhs = Expr::new(
                ExprKind::Pipe {
                    lhs: Box::new(lhs),
                    stage,
                },
                span,
            );
        }
        Ok(lhs)
    }

    fn stage_ends(&self) -> bool {
        match self.peek() {
            Tok::Pipe
            | Tok::Newline
            | Tok::Eof
            | Tok::RParen
            | Tok::RBracket
            | Tok::RBrace
            | Tok::Comma => true,
            Tok::LBrace => self.nostruct,
            _ => false,
        }
    }

    /// Whether the next token can begin a juxtaposed stage argument
    /// (SPEC.md 8.2 rule 3).
    fn juxt_starts(&self) -> bool {
        let t = self.peek_token();
        if !t.space_before {
            return false;
        }
        match &t.tok {
            Tok::Ident(_)
            | Tok::Int(_)
            | Tok::Float(_)
            | Tok::Duration(_)
            | Tok::Str(_)
            | Tok::LBracket
            | Tok::LParen => true,
            Tok::Kw(
                Kw::True | Kw::False | Kw::Null | Kw::Not | Kw::Fn | Kw::If | Kw::Match | Kw::Try,
            ) => true,
            Tok::LBrace => !self.nostruct,
            Tok::Dot => matches!(
                self.peek_at(1),
                Token {
                    tok: Tok::Ident(_) | Tok::Kw(_),
                    space_before: false,
                    ..
                }
            ),
            Tok::Minus => matches!(
                self.peek_at(1),
                Token {
                    tok: Tok::Int(_) | Tok::Float(_) | Tok::Duration(_),
                    space_before: false,
                    ..
                }
            ),
            Tok::Ellipsis => true,
            _ => false,
        }
    }

    fn stage(&mut self) -> PResult<Stage> {
        let start = self.pos;
        if matches!(self.peek(), Tok::Command(_) | Tok::RunFn { .. }) {
            let e = self.postfix()?;
            return Ok(Stage::Run(Box::new(e)));
        }
        if let Tok::Ident(name) = self.peek().clone()
            && self.peek_at(1).tok != Tok::FatArrow
        {
            let t = self.bump();
            let mut callee = Expr::new(ExprKind::Ident(name), t.span);
            while self.at(&Tok::Dot)
                && !self.peek_token().space_before
                && matches!(
                    self.peek_at(1),
                    Token {
                        tok: Tok::Ident(_),
                        space_before: false,
                        ..
                    }
                )
            {
                self.bump();
                let n = self.bump();
                let Tok::Ident(field) = n.tok else {
                    return Err(internal());
                };
                let span = callee.span.to(n.span);
                callee = Expr::new(
                    ExprKind::Field {
                        expr: Box::new(callee),
                        name: field,
                        name_span: n.span,
                    },
                    span,
                );
            }
            let mut args = Vec::new();
            if self.at(&Tok::LParen) && !self.peek_token().space_before {
                self.bump();
                args = self.call_args()?;
            }
            // A juxtaposed stage takes exactly one argument, so a following
            // comma belongs to the enclosing call or list (SPEC.md 8.2).
            if self.juxt_starts() {
                args.push(self.one_arg(true)?);
            }
            if self.stage_ends() {
                return Ok(Stage::Call {
                    callee: Box::new(callee),
                    args,
                });
            }
            self.pos = start;
        }
        let e = self.arg_coalesce()?;
        let span = e.span;
        match e.kind {
            ExprKind::Call { callee, args } => Ok(Stage::Call { callee, args }),
            ExprKind::MethodCall {
                recv, name, args, ..
            } => Ok(Stage::Method { recv, name, args }),
            ExprKind::Lambda(_)
            | ExprKind::Group(_)
            | ExprKind::Ident(_)
            | ExprKind::Field { .. } => Ok(Stage::Value(Box::new(e))),
            ExprKind::Binary(op, ..) => Err(syntax(span, "a pipeline stage must be a call")
                .with_help(format!(
                    "to use `{}` on the result, wrap the pipeline in parentheses: (a | f) {} b",
                    op.symbol(),
                    op.symbol()
                ))),
            _ => Err(syntax(
                span,
                "a pipeline stage must be a call, like `| count` or `| where .active`",
            )),
        }
    }

    /// One call argument: `expr`, `name: expr` or `...expr`.
    fn one_arg(&mut self, juxt: bool) -> PResult<Arg> {
        if self.eat(&Tok::Ellipsis) {
            let e = if juxt {
                self.arg_coalesce()?
            } else {
                self.arg_expr()?
            };
            return Ok(Arg::Spread(e));
        }
        if let Tok::Ident(name) = self.peek().clone()
            && self.peek_at(1).tok == Tok::Colon
        {
            let t = self.bump();
            self.bump();
            let value = if juxt {
                self.arg_coalesce()?
            } else {
                self.arg_expr()?
            };
            return Ok(Arg::Named {
                name,
                name_span: t.span,
                value,
            });
        }
        let e = if juxt {
            self.arg_coalesce()?
        } else {
            self.arg_expr()?
        };
        Ok(Arg::Pos(e))
    }

    /// Arguments after `(`, up to and including `)`.
    fn call_args(&mut self) -> PResult<Vec<Arg>> {
        let mut args = Vec::new();
        let mut seen_named = false;
        while !self.at(&Tok::RParen) {
            let arg = self.with_nostruct(false, |p| p.one_arg(false))?;
            match &arg {
                Arg::Named { .. } => seen_named = true,
                Arg::Pos(e) | Arg::Spread(e) if seen_named => {
                    return Err(syntax(
                        e.span,
                        "positional arguments must come before named arguments",
                    ));
                }
                _ => {}
            }
            args.push(arg);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen, "`,` or `)`")?;
        Ok(args)
    }

    fn coalesce(&mut self) -> PResult<Expr> {
        let mut left = self.or_expr()?;
        while self.at(&Tok::Coalesce) {
            self.bump();
            let right = self.or_expr()?;
            for side in [&left, &right] {
                if let ExprKind::Binary(BinOp::And | BinOp::Or, ..) = side.kind {
                    return Err(syntax(
                        side.span,
                        "`??` cannot be mixed with `and`/`or` without parentheses",
                    )
                    .with_help("add parentheses to say which part goes first"));
                }
            }
            let span = left.span.to(right.span);
            left = Expr::new(
                ExprKind::Binary(BinOp::Coalesce, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut left = self.and_expr()?;
        while self.eat(&Tok::Kw(Kw::Or)) {
            let right = self.and_expr()?;
            let span = left.span.to(right.span);
            left = Expr::new(
                ExprKind::Binary(BinOp::Or, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let mut left = self.not_expr()?;
        while self.eat(&Tok::Kw(Kw::And)) {
            let right = self.not_expr()?;
            let span = left.span.to(right.span);
            left = Expr::new(
                ExprKind::Binary(BinOp::And, Box::new(left), Box::new(right)),
                span,
            );
        }
        Ok(left)
    }

    fn not_expr(&mut self) -> PResult<Expr> {
        if self.at(&Tok::Kw(Kw::Not)) {
            let t = self.bump();
            self.enter()?;
            let inner = self.not_expr();
            self.leave();
            let inner = inner?;
            let span = t.span.to(inner.span);
            return Ok(Expr::new(ExprKind::Unary(UnOp::Not, Box::new(inner)), span));
        }
        self.comparison()
    }

    fn comp_op(&self) -> Option<(BinOp, usize)> {
        Some(match self.peek() {
            Tok::EqEq => (BinOp::Eq, 1),
            Tok::NotEq => (BinOp::Ne, 1),
            Tok::Lt => (BinOp::Lt, 1),
            Tok::Le => (BinOp::Le, 1),
            Tok::Gt => (BinOp::Gt, 1),
            Tok::Ge => (BinOp::Ge, 1),
            Tok::Kw(Kw::In) => (BinOp::In, 1),
            Tok::Kw(Kw::Not) if self.peek_at(1).tok == Tok::Kw(Kw::In) => (BinOp::NotIn, 2),
            _ => return None,
        })
    }

    fn comparison(&mut self) -> PResult<Expr> {
        let left = self.additive()?;
        let Some((op, n)) = self.comp_op() else {
            return Ok(left);
        };
        for _ in 0..n {
            self.bump();
        }
        let right = self.additive()?;
        let span = left.span.to(right.span);
        if self.comp_op().is_some() {
            return Err(syntax(self.span(), "comparisons cannot be chained")
                .with_help("combine them with `and`: a < b and b < c"));
        }
        Ok(Expr::new(
            ExprKind::Binary(op, Box::new(left), Box::new(right)),
            span,
        ))
    }

    fn additive(&mut self) -> PResult<Expr> {
        let mut left = self.term()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let right = self.term()?;
            let span = left.span.to(right.span);
            left = Expr::new(ExprKind::Binary(op, Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn term(&mut self) -> PResult<Expr> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Rem,
                _ => break,
            };
            self.bump();
            let right = self.unary()?;
            let span = left.span.to(right.span);
            left = Expr::new(ExprKind::Binary(op, Box::new(left), Box::new(right)), span);
        }
        Ok(left)
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.at(&Tok::Minus) {
            let t = self.bump();
            self.enter()?;
            let inner = self.unary();
            self.leave();
            let inner = inner?;
            let span = t.span.to(inner.span);
            return Ok(Expr::new(ExprKind::Unary(UnOp::Neg, Box::new(inner)), span));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    let dot = self.bump();
                    let n = self.bump();
                    let name = match n.tok {
                        Tok::Ident(name) => name,
                        Tok::Kw(k) => k.as_str().to_string(),
                        other => {
                            let mut err = syntax(
                                n.span,
                                format!(
                                    "expected a field name after `.`, found {}",
                                    other.describe()
                                ),
                            );
                            if matches!(other, Tok::Int(_)) {
                                err = err.with_help("index lists with brackets: xs[0]");
                            }
                            return Err(err);
                        }
                    };
                    let _ = dot;
                    if self.at(&Tok::LParen) && !self.peek_token().space_before {
                        self.bump();
                        let args = self.call_args()?;
                        let span = e.span.to(self.prev_span());
                        e = Expr::new(
                            ExprKind::MethodCall {
                                recv: Box::new(e),
                                name,
                                name_span: n.span,
                                args,
                            },
                            span,
                        );
                    } else {
                        let span = e.span.to(n.span);
                        e = Expr::new(
                            ExprKind::Field {
                                expr: Box::new(e),
                                name,
                                name_span: n.span,
                            },
                            span,
                        );
                    }
                }
                Tok::LParen if !self.peek_token().space_before => {
                    self.bump();
                    let args = self.call_args()?;
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(
                        ExprKind::Call {
                            callee: Box::new(e),
                            args,
                        },
                        span,
                    );
                }
                Tok::LBracket if !self.peek_token().space_before => {
                    self.bump();
                    let index = self.with_nostruct(false, |p| p.expr())?;
                    self.expect(&Tok::RBracket, "`]`")?;
                    let span = e.span.to(self.prev_span());
                    e = Expr::new(
                        ExprKind::Index {
                            expr: Box::new(e),
                            index: Box::new(index),
                        },
                        span,
                    );
                }
                Tok::Question => {
                    let q = self.bump();
                    let span = e.span.to(q.span);
                    e = Expr::new(ExprKind::Catch(Box::new(e)), span);
                    let continues = match self.peek() {
                        Tok::Dot => matches!(self.peek_at(1).tok, Tok::Ident(_)),
                        Tok::LParen | Tok::LBracket => !self.peek_token().space_before,
                        _ => false,
                    };
                    if continues {
                        return Err(syntax(q.span, "`?` must end the expression it applies to")
                            .with_help("ok has no optional chaining: put `?` at the end (a.b?) or wrap it: (a?).b"));
                    }
                    break;
                }
                _ => break,
            }
        }
        Ok(e)
    }

    /// Whether the tokens at `pos` are `( name, name ) =>`.
    fn arrow_params_ahead(&self) -> bool {
        let mut i = self.pos + 1;
        let mut expect_name = true;
        loop {
            let t = &self.toks[i.min(self.toks.len() - 1)].tok;
            match t {
                Tok::RParen => return self.toks.get(i + 1).is_some_and(|t| t.tok == Tok::FatArrow),
                Tok::Ident(_) if expect_name => expect_name = false,
                Tok::Comma if !expect_name => expect_name = true,
                _ => return false,
            }
            i += 1;
        }
    }

    fn lambda(&mut self, params: Vec<(String, Span)>, start: Span) -> PResult<Expr> {
        self.expect(&Tok::FatArrow, "`=>`")?;
        let saved_loop = self.loop_depth;
        self.loop_depth = 0;
        self.fn_depth += 1;
        let body = self.arg_coalesce();
        self.fn_depth -= 1;
        self.loop_depth = saved_loop;
        let body = body?;
        let span = start.to(body.span);
        let params = params
            .into_iter()
            .map(|(name, s)| Param {
                cli_name: name.clone(),
                name,
                flag: false,
                rest: false,
                ty: None,
                default: None,
                help: None,
                span: s,
            })
            .collect();
        let decl = FnDecl {
            name: "<closure>".into(),
            name_span: start,
            params,
            body: Block {
                span: body.span,
                stmts: vec![Stmt {
                    span: body.span,
                    kind: StmtKind::Expr(body),
                }],
            },
            doc: None,
            span,
        };
        Ok(Expr::new(ExprKind::Lambda(Rc::new(decl)), span))
    }

    fn primary(&mut self) -> PResult<Expr> {
        self.enter()?;
        let r = self.primary_inner();
        self.leave();
        r
    }

    fn primary_inner(&mut self) -> PResult<Expr> {
        let t = self.peek_token().clone();
        let span = t.span;
        let simple = |k| Ok(Expr::new(k, span));
        match t.tok {
            Tok::Int(v) => {
                self.bump();
                simple(ExprKind::Int(v))
            }
            Tok::Float(v) => {
                self.bump();
                simple(ExprKind::Float(v))
            }
            Tok::Duration(v) => {
                self.bump();
                simple(ExprKind::Duration(v))
            }
            Tok::Kw(Kw::True) => {
                self.bump();
                simple(ExprKind::Bool(true))
            }
            Tok::Kw(Kw::False) => {
                self.bump();
                simple(ExprKind::Bool(false))
            }
            Tok::Kw(Kw::Null) => {
                self.bump();
                simple(ExprKind::Null)
            }
            Tok::Str(parts) => {
                self.bump();
                let pieces = self.string_pieces(parts)?;
                simple(ExprKind::Str(pieces))
            }
            Tok::Ident(name) => {
                self.bump();
                if self.at(&Tok::FatArrow) {
                    return self.lambda(vec![(name, span)], span);
                }
                simple(ExprKind::Ident(name))
            }
            Tok::Dot if matches!(self.peek_at(1).tok, Tok::Ident(_) | Tok::Kw(_)) => {
                // The receiver of `.name` shorthand; `postfix` adds the field.
                Ok(Expr::new(ExprKind::It, span))
            }
            Tok::LParen => {
                if self.arrow_params_ahead() {
                    self.bump();
                    let mut params = Vec::new();
                    while let Tok::Ident(n) = self.peek().clone() {
                        let s = self.bump().span;
                        params.push((n, s));
                        if !self.eat(&Tok::Comma) {
                            break;
                        }
                    }
                    self.expect(&Tok::RParen, "`)`")?;
                    return self.lambda(params, span);
                }
                self.bump();
                let inner = self.with_nostruct(false, |p| p.expr())?;
                let close = self.expect(&Tok::RParen, "`)`")?;
                Ok(Expr::new(
                    ExprKind::Group(Box::new(inner)),
                    span.to(close.span),
                ))
            }
            Tok::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                while !self.at(&Tok::RBracket) {
                    if self.eat(&Tok::Ellipsis) {
                        elems.push(Elem::Spread(self.arg_expr()?));
                    } else {
                        elems.push(Elem::Item(self.arg_expr()?));
                    }
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let close = self.expect(&Tok::RBracket, "`,` or `]`")?;
                Ok(Expr::new(ExprKind::List(elems), span.to(close.span)))
            }
            Tok::LBrace if !self.nostruct => self.record(),
            Tok::Kw(Kw::If) => self.if_expr(),
            Tok::Kw(Kw::Match) => self.match_expr(),
            Tok::Kw(Kw::Try) => {
                self.bump();
                let body = self.block()?;
                self.skip_newlines();
                self.expect(&Tok::Kw(Kw::Catch), "`catch` after the try block")?;
                let binding = match self.peek().clone() {
                    Tok::Ident(n) => {
                        self.bump();
                        Some(n)
                    }
                    _ => None,
                };
                let handler = self.block()?;
                let s = span.to(handler.span);
                Ok(Expr::new(
                    ExprKind::Try {
                        body,
                        binding,
                        handler,
                    },
                    s,
                ))
            }
            Tok::Kw(Kw::Fn) if self.peek_at(1).tok == Tok::LParen => {
                self.bump();
                self.bump();
                let params = self.params(&Tok::RParen)?;
                let body = self.fn_body()?;
                let s = span.to(body.span);
                let decl = FnDecl {
                    name: "<closure>".into(),
                    name_span: span,
                    params,
                    body,
                    doc: None,
                    span: s,
                };
                Ok(Expr::new(ExprKind::Lambda(Rc::new(decl)), s))
            }
            Tok::Command(cmd) => {
                self.bump();
                let command = self.command(*cmd, span)?;
                Ok(Expr::new(ExprKind::Run(Box::new(command)), span))
            }
            Tok::RunFn { bang } => {
                self.bump();
                self.expect(&Tok::LParen, "`(`")?;
                let args = self.call_args()?;
                Ok(Expr::new(
                    ExprKind::RunFn { bang, args },
                    span.to(self.prev_span()),
                ))
            }
            Tok::Kw(Kw::Not) => self.not_expr(),
            Tok::Minus => self.unary(),
            _ => Err(self.unexpected("an expression")),
        }
    }

    fn string_pieces(&self, parts: Vec<StrPart>) -> PResult<Vec<StrPiece>> {
        let mut pieces = Vec::new();
        for part in parts {
            match part {
                StrPart::Lit(s) => pieces.push(StrPiece::Lit(s)),
                StrPart::Interp { tokens, spec, span } => {
                    let expr = self.sub_expr(tokens, span)?;
                    pieces.push(StrPiece::Interp { expr, spec });
                }
            }
        }
        Ok(pieces)
    }

    /// Parse a nested token list as one complete expression.
    fn sub_expr(&self, tokens: Vec<Token>, span: Span) -> PResult<Expr> {
        let mut p = self.sub(tokens, span);
        let e = p.expr()?;
        if !p.at(&Tok::Eof) {
            return Err(p.unexpected("`}` to end the interpolation"));
        }
        p.check_no_it(&e)?;
        Ok(e)
    }

    fn command(&self, cmd: CommandTok, span: Span) -> PResult<Command> {
        let word = |w: lexer::Word| -> PResult<CmdWord> {
            let mut parts = Vec::new();
            for part in w.parts {
                parts.push(match part {
                    WordPart::Lit { text, .. } => CmdPart::Lit(text),
                    WordPart::Interp { tokens, spec, span } => CmdPart::Interp {
                        expr: self.sub_expr(tokens, span)?,
                        spec,
                    },
                    WordPart::Spread { tokens, span } => {
                        CmdPart::Spread(self.sub_expr(tokens, span)?)
                    }
                });
            }
            Ok(CmdWord {
                parts,
                span: w.span,
            })
        };
        let mut words = Vec::new();
        for w in cmd.words {
            words.push(word(w)?);
        }
        let mut redirects = Vec::new();
        for r in cmd.redirects {
            redirects.push((r.kind, word(r.target)?));
        }
        let options = match cmd.with {
            Some(tokens) => {
                let mut p = self.sub(tokens, span);
                let e = p.record()?;
                if !p.at(&Tok::Eof) {
                    return Err(p.unexpected("the end of the command"));
                }
                Some(Box::new(e))
            }
            None => None,
        };
        Ok(Command {
            bang: cmd.bang,
            words,
            redirects,
            options,
        })
    }

    fn record(&mut self) -> PResult<Expr> {
        let open = self.expect(&Tok::LBrace, "`{`")?;
        let mut entries = Vec::new();
        self.skip_newlines();
        while !self.at(&Tok::RBrace) {
            if self.eat(&Tok::Ellipsis) {
                entries.push(Entry::Spread(self.arg_expr()?));
            } else {
                let t = self.bump();
                let key = match &t.tok {
                    Tok::Ident(k) => k.clone(),
                    Tok::Kw(k) => k.as_str().to_string(),
                    Tok::Str(parts) => match parts.as_slice() {
                        [StrPart::Lit(s)] => s.clone(),
                        _ => return Err(syntax(t.span, "record keys cannot use interpolation")),
                    },
                    other => {
                        return Err(syntax(
                            t.span,
                            format!("expected a record key, found {}", other.describe()),
                        ));
                    }
                };
                if self.eat(&Tok::Colon) {
                    let value = self.arg_expr()?;
                    entries.push(Entry::Field {
                        key,
                        key_span: t.span,
                        value,
                    });
                } else if matches!(t.tok, Tok::Ident(_)) {
                    let value = Expr::new(ExprKind::Ident(key.clone()), t.span);
                    entries.push(Entry::Field {
                        key,
                        key_span: t.span,
                        value,
                    });
                } else {
                    return Err(self.unexpected("`:` after the record key"));
                }
            }
            let had_comma = self.eat(&Tok::Comma);
            let had_newline = self.at(&Tok::Newline);
            self.skip_newlines();
            if !had_comma && !had_newline {
                break;
            }
        }
        let close = self.expect(&Tok::RBrace, "`,` or `}`")?;
        Ok(Expr::new(
            ExprKind::Record(entries),
            open.span.to(close.span),
        ))
    }

    fn if_expr(&mut self) -> PResult<Expr> {
        let start = self.bump().span;
        let cond = self.with_nostruct(true, |p| p.expr())?;
        let then = self.block()?;
        let mut end = then.span;
        let otherwise = if self.at(&Tok::Kw(Kw::Else)) {
            self.bump();
            let e = if self.at(&Tok::Kw(Kw::If)) {
                self.if_expr()?
            } else {
                let b = self.block()?;
                let s = b.span;
                Expr::new(ExprKind::Block(b), s)
            };
            end = e.span;
            Some(Box::new(e))
        } else {
            None
        };
        Ok(Expr::new(
            ExprKind::If {
                cond: Box::new(cond),
                then,
                otherwise,
            },
            start.to(end),
        ))
    }

    fn match_expr(&mut self) -> PResult<Expr> {
        let start = self.bump().span;
        let subject = self.with_nostruct(true, |p| p.expr())?;
        self.expect(&Tok::LBrace, "`{` after the match value")?;
        self.skip_newlines();
        let mut arms = Vec::new();
        while !self.at(&Tok::RBrace) {
            let arm_start = self.span();
            let mut pats = vec![self.match_pat()?];
            while self.eat(&Tok::Pipe) {
                pats.push(self.match_pat()?);
            }
            let guard = if self.eat(&Tok::Kw(Kw::If)) {
                Some(self.with_nostruct(false, |p| p.expr())?)
            } else {
                None
            };
            self.expect(&Tok::FatArrow, "`=>` after the pattern")?;
            let body = if self.at(&Tok::LBrace) {
                let b = self.block()?;
                let s = b.span;
                Expr::new(ExprKind::Block(b), s)
            } else {
                self.with_nostruct(false, |p| p.expr())?
            };
            arms.push(Arm {
                pats,
                guard,
                span: arm_start.to(body.span),
                body,
            });
            if !self.eat(&Tok::Comma) && !self.at(&Tok::Newline) && !self.at(&Tok::RBrace) {
                return Err(self.unexpected("a new line between match arms"));
            }
            self.skip_newlines();
        }
        let close = self.bump();
        if arms.is_empty() {
            return Err(syntax(start, "a match needs at least one arm"));
        }
        Ok(Expr::new(
            ExprKind::Match {
                subject: Box::new(subject),
                arms,
            },
            start.to(close.span),
        ))
    }

    fn match_pat(&mut self) -> PResult<MatchPat> {
        let t = self.peek_token().clone();
        let lit = |k| MatchPat::Lit(Expr::new(k, t.span));
        let pat = match t.tok {
            Tok::Int(v) => lit(ExprKind::Int(v)),
            Tok::Float(v) => lit(ExprKind::Float(v)),
            Tok::Duration(v) => lit(ExprKind::Duration(v)),
            Tok::Kw(Kw::True) => lit(ExprKind::Bool(true)),
            Tok::Kw(Kw::False) => lit(ExprKind::Bool(false)),
            Tok::Kw(Kw::Null) => lit(ExprKind::Null),
            Tok::Str(parts) => match parts.as_slice() {
                [StrPart::Lit(s)] => lit(ExprKind::Str(vec![StrPiece::Lit(s.clone())])),
                _ => {
                    return Err(syntax(t.span, "match patterns cannot use interpolation")
                        .with_help("bind the value and use a guard: x if x == \"{a}\" =>"));
                }
            },
            Tok::Minus => {
                self.bump();
                let n = self.peek_token().clone();
                let kind = match n.tok {
                    Tok::Int(v) => ExprKind::Int(-v),
                    Tok::Float(v) => ExprKind::Float(-v),
                    Tok::Duration(v) => ExprKind::Duration(-v),
                    _ => return Err(self.unexpected("a number after `-`")),
                };
                self.bump();
                return Ok(MatchPat::Lit(Expr::new(kind, t.span.to(n.span))));
            }
            Tok::Ident(ref n) if n == "_" => MatchPat::Wildcard,
            Tok::Ident(ref n) => MatchPat::Bind(n.clone()),
            _ => return Err(self.unexpected("a pattern (a literal, `_` or a name)")),
        };
        self.bump();
        Ok(pat)
    }
}

fn internal() -> OkError {
    OkError::new(ErrKind::Syntax, "internal parser error: expected a name")
}

/// Wrap an expression that uses `.name` shorthand into a closure over its
/// implicit argument.
fn wrap_it(e: Expr) -> Expr {
    if find_it(&e).is_none() {
        return e;
    }
    let span = e.span;
    let param = Param {
        name: IT.into(),
        cli_name: IT.into(),
        flag: false,
        rest: false,
        ty: None,
        default: None,
        help: None,
        span,
    };
    let decl = FnDecl {
        name: "<shorthand>".into(),
        name_span: span,
        params: vec![param],
        body: Block {
            span,
            stmts: vec![Stmt {
                span,
                kind: StmtKind::Expr(e),
            }],
        },
        doc: None,
        span,
    };
    Expr::new(ExprKind::Lambda(Rc::new(decl)), span)
}

/// The span of the first `.name` shorthand in `e` that is not already inside
/// a closure.
pub fn find_it(e: &Expr) -> Option<Span> {
    let args = |args: &[Arg]| {
        args.iter().find_map(|a| match a {
            Arg::Pos(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => find_it(e),
        })
    };
    match &e.kind {
        ExprKind::It => Some(e.span),
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Duration(_)
        | ExprKind::Ident(_)
        | ExprKind::Lambda(_)
        | ExprKind::Block(_)
        | ExprKind::Try { .. }
        | ExprKind::Run(_) => None,
        ExprKind::Str(pieces) => pieces.iter().find_map(|p| match p {
            StrPiece::Interp { expr, .. } => find_it(expr),
            StrPiece::Lit(_) => None,
        }),
        ExprKind::List(elems) => elems.iter().find_map(|el| match el {
            Elem::Item(e) | Elem::Spread(e) => find_it(e),
        }),
        ExprKind::Record(entries) => entries.iter().find_map(|en| match en {
            Entry::Field { value, .. } => find_it(value),
            Entry::Spread(e) => find_it(e),
        }),
        ExprKind::Group(e) | ExprKind::Unary(_, e) | ExprKind::Catch(e) => find_it(e),
        ExprKind::Binary(_, a, b) => find_it(a).or_else(|| find_it(b)),
        ExprKind::Call { callee, args: a } => find_it(callee).or_else(|| args(a)),
        ExprKind::MethodCall { recv, args: a, .. } => find_it(recv).or_else(|| args(a)),
        ExprKind::Field { expr, .. } => find_it(expr),
        ExprKind::Index { expr, index } => find_it(expr).or_else(|| find_it(index)),
        ExprKind::If {
            cond, otherwise, ..
        } => find_it(cond).or_else(|| otherwise.as_deref().and_then(find_it)),
        ExprKind::Match { subject, arms } => find_it(subject).or_else(|| {
            arms.iter().find_map(|a| {
                a.guard
                    .as_ref()
                    .and_then(find_it)
                    .or_else(|| find_it(&a.body))
            })
        }),
        ExprKind::Pipe { lhs, stage } => find_it(lhs).or_else(|| match stage {
            Stage::Call { callee, args: a } => find_it(callee).or_else(|| args(a)),
            Stage::Method { recv, args: a, .. } => find_it(recv).or_else(|| args(a)),
            Stage::Value(e) | Stage::Run(e) => find_it(e),
        }),
        ExprKind::RunFn { args: a, .. } => args(a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Program {
        match parse(0, src) {
            Ok(p) => p,
            Err(e) => panic!("parse failed for {src:?}: {} {:?}", e.message, e.help),
        }
    }

    fn fails(src: &str) -> OkError {
        parse(0, src)
            .err()
            .unwrap_or_else(|| panic!("expected a parse error for {src:?}"))
    }

    fn first_expr(src: &str) -> ExprKind {
        match ok(src).stmts.into_iter().next().map(|s| s.kind) {
            Some(StmtKind::Expr(e)) => e.kind,
            Some(StmtKind::Let { value, .. }) => value.kind,
            other => panic!("not an expression: {other:?}"),
        }
    }

    #[test]
    fn precedence() {
        let ExprKind::Binary(BinOp::Or, _, r) = first_expr("a or b and not c == d + e * f") else {
            panic!()
        };
        let ExprKind::Binary(BinOp::And, _, r) = r.kind else {
            panic!()
        };
        let ExprKind::Unary(UnOp::Not, r) = r.kind else {
            panic!()
        };
        let ExprKind::Binary(BinOp::Eq, _, r) = r.kind else {
            panic!()
        };
        let ExprKind::Binary(BinOp::Add, _, r) = r.kind else {
            panic!()
        };
        assert!(matches!(r.kind, ExprKind::Binary(BinOp::Mul, ..)));
    }

    #[test]
    fn comparisons_do_not_chain_and_coalesce_needs_parens() {
        assert!(fails("a < b < c").message.contains("chained"));
        assert!(fails("a ?? b or c").message.contains("??"));
        ok("a ?? (b or c)");
        assert!(matches!(
            first_expr("x not in xs"),
            ExprKind::Binary(BinOp::NotIn, ..)
        ));
    }

    #[test]
    fn pipeline_stages() {
        let ExprKind::Pipe { stage, .. } = first_expr("xs | where .active") else {
            panic!()
        };
        let Stage::Call { args, .. } = stage else {
            panic!()
        };
        assert!(matches!(
            &args[0],
            Arg::Pos(Expr {
                kind: ExprKind::Lambda(_),
                ..
            })
        ));
        let ExprKind::Pipe { stage, .. } = first_expr("xs | take 5") else {
            panic!()
        };
        assert!(matches!(stage, Stage::Call { .. }));
        let ExprKind::Pipe { stage, .. } = first_expr("x | .items") else {
            panic!()
        };
        assert!(matches!(stage, Stage::Value(_)));
        let ExprKind::Pipe { stage, .. } = first_expr("xs | sort(.size, desc: true)") else {
            panic!()
        };
        let Stage::Call { args, .. } = stage else {
            panic!()
        };
        assert_eq!(args.len(), 2);
        assert!(
            fails("xs | count > 3")
                .help
                .unwrap()
                .contains("parentheses")
        );
        let ExprKind::Pipe { lhs, .. } = first_expr("xs | map x => x * 2 | sum") else {
            panic!()
        };
        assert!(matches!(lhs.kind, ExprKind::Pipe { .. }));
    }

    #[test]
    fn implicit_closure_covers_whole_argument() {
        let ExprKind::Pipe {
            stage: Stage::Call { args, .. },
            ..
        } = first_expr("u | where .age >= 18 and .active")
        else {
            panic!()
        };
        let Arg::Pos(Expr {
            kind: ExprKind::Lambda(decl),
            ..
        }) = &args[0]
        else {
            panic!()
        };
        let StmtKind::Expr(body) = &decl.body.stmts[0].kind else {
            panic!()
        };
        assert!(matches!(body.kind, ExprKind::Binary(BinOp::And, ..)));
        assert!(fails(".x == 1").message.contains("shorthand"));
        ok("let f = .name");
    }

    #[test]
    fn records_versus_blocks() {
        assert!(matches!(first_expr("{a: 1, b}"), ExprKind::Record(_)));
        let ExprKind::If { cond, .. } = first_expr("if x == y { 1 } else { 2 }") else {
            panic!()
        };
        assert!(matches!(cond.kind, ExprKind::Binary(BinOp::Eq, ..)));
        ok("for k, v in r {\n print(k)\n}");
        ok("let r = {\n  a: 1\n  b: 2,\n}");
    }

    #[test]
    fn calls_need_adjacent_parens() {
        assert!(matches!(first_expr("f(1)"), ExprKind::Call { .. }));
        assert!(fails("print \"hi\"").help.unwrap().contains("print(...)"));
    }

    #[test]
    fn catch_operator_ends_the_chain() {
        assert!(matches!(first_expr("a.b?"), ExprKind::Catch(_)));
        assert!(fails("a?.b").help.unwrap().contains("optional chaining"));
    }

    #[test]
    fn declarations() {
        let p = ok(
            "# Convert things\nfn main(\n  input: path,   # the input\n  --out: path = \"o.json\",\n  --dry-run: bool,\n  ...rest: string,\n) { }\n",
        );
        let StmtKind::Fn(f) = &p.stmts[0].kind else {
            panic!()
        };
        assert_eq!(f.doc.as_deref(), Some("Convert things"));
        assert_eq!(f.params.len(), 4);
        assert_eq!(f.params[0].help.as_deref(), Some("the input"));
        assert_eq!(f.params[2].name, "dry_run");
        assert_eq!(f.params[2].cli_name, "dry-run");
        assert!(f.params[3].rest);
    }

    #[test]
    fn tasks() {
        let p = ok(
            "# Wipe it\ntask reset-db(--force: bool) needs [gen, db.migrate] confirm \"sure?\" hidden {\n}\n",
        );
        let StmtKind::Task(t) = &p.stmts[0].kind else {
            panic!()
        };
        assert_eq!(t.name, "reset-db");
        assert_eq!(
            t.needs.iter().map(|n| n.0.as_str()).collect::<Vec<_>>(),
            ["gen", "db.migrate"]
        );
        assert!(t.hidden);
        assert_eq!(t.doc.as_deref(), Some("Wipe it"));
        assert!(
            fails("task x allow [net] {}")
                .message
                .contains("not implemented")
        );
    }

    #[test]
    fn match_and_try() {
        ok(
            "let x = match os.name {\n  \"windows\" => 1\n  \"linux\" | \"macos\" => 2\n  n if n == \"x\" => 3\n  _ => fail(\"no\")\n}",
        );
        ok("let c = try {\n read(p)\n} catch e {\n {}\n}");
    }

    #[test]
    fn break_and_return_placement() {
        assert!(fails("break").message.contains("outside"));
        assert!(fails("return 1").help.unwrap().contains("exit"));
        ok("fn f() { for x in xs { if x { break } } \n return 1 }");
    }

    #[test]
    fn habits() {
        assert!(fails("function f() {}").help.unwrap().contains("fn"));
        assert!(
            fails("if a { } elif b { }").message.contains("elif")
                || fails("if a {\n}\nelif b {\n}").message.contains("elif")
        );
    }

    #[test]
    fn commands_parse() {
        let ExprKind::Run(cmd) =
            first_expr("run cargo build {...flags} --target={t} with {cwd: \"x\"}")
        else {
            panic!()
        };
        assert_eq!(cmd.words.len(), 4);
        assert!(cmd.options.is_some());
        let ExprKind::Field { expr, .. } = first_expr("(run git rev-parse HEAD).out") else {
            panic!()
        };
        assert!(matches!(expr.kind, ExprKind::Group(_)));
    }

    #[test]
    fn deep_nesting_is_an_error_not_a_crash() {
        let src = "[".repeat(5000);
        assert!(parse(0, &src).is_err());
        let src = format!("{}1", "-".repeat(5000));
        assert!(parse(0, &src).is_err());
    }
}
