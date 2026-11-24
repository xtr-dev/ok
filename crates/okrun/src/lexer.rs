//! The lexer (SPEC.md section 3 and 9.2).
//!
//! It turns source text into tokens. Two things make it more than a simple
//! scanner:
//!
//! * **Nested token streams.** String interpolations `{expr}` and command
//!   words are lexed recursively, so a string token carries the tokens of its
//!   interpolated expressions and a command token carries its words. The
//!   parser then never has to re-scan text.
//! * **Command mode.** After `run` or `run!` followed by whitespace, the rest
//!   of the command is lexed by different rules (words, quotes,
//!   interpolation, redirects, `with {...}`) into one [`Tok::Command`].
//!
//! Newlines are significant; [`fix_newlines`] applies the continuation rules
//! of SPEC.md 3.3 after lexing.

use crate::error::{ErrKind, OkError};
use crate::source::{FileId, Span};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kw {
    And,
    As,
    Break,
    Catch,
    Continue,
    Else,
    False,
    Fn,
    For,
    If,
    Import,
    In,
    Let,
    Match,
    Not,
    Null,
    Or,
    Requires,
    Return,
    Task,
    True,
    Try,
    Var,
    While,
}

impl Kw {
    pub fn from_word(s: &str) -> Option<Kw> {
        Some(match s {
            "and" => Kw::And,
            "as" => Kw::As,
            "break" => Kw::Break,
            "catch" => Kw::Catch,
            "continue" => Kw::Continue,
            "else" => Kw::Else,
            "false" => Kw::False,
            "fn" => Kw::Fn,
            "for" => Kw::For,
            "if" => Kw::If,
            "import" => Kw::Import,
            "in" => Kw::In,
            "let" => Kw::Let,
            "match" => Kw::Match,
            "not" => Kw::Not,
            "null" => Kw::Null,
            "or" => Kw::Or,
            "requires" => Kw::Requires,
            "return" => Kw::Return,
            "task" => Kw::Task,
            "true" => Kw::True,
            "try" => Kw::Try,
            "var" => Kw::Var,
            "while" => Kw::While,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kw::And => "and",
            Kw::As => "as",
            Kw::Break => "break",
            Kw::Catch => "catch",
            Kw::Continue => "continue",
            Kw::Else => "else",
            Kw::False => "false",
            Kw::Fn => "fn",
            Kw::For => "for",
            Kw::If => "if",
            Kw::Import => "import",
            Kw::In => "in",
            Kw::Let => "let",
            Kw::Match => "match",
            Kw::Not => "not",
            Kw::Null => "null",
            Kw::Or => "or",
            Kw::Requires => "requires",
            Kw::Return => "return",
            Kw::Task => "task",
            Kw::True => "true",
            Kw::Try => "try",
            Kw::Var => "var",
            Kw::While => "while",
        }
    }
}

/// A piece of a string literal.
#[derive(Clone, Debug, PartialEq)]
pub enum StrPart {
    Lit(String),
    /// `{expr}` or `{expr:spec}`; `span` covers the braces.
    Interp {
        tokens: Vec<Token>,
        spec: Option<String>,
        span: Span,
    },
}

/// A piece of a command-mode word (SPEC.md 9.2).
#[derive(Clone, Debug, PartialEq)]
pub enum WordPart {
    /// Literal text. `quoted` is false for bare characters, which is what the
    /// shell-habit checks look at.
    Lit { text: String, quoted: bool },
    Interp {
        tokens: Vec<Token>,
        spec: Option<String>,
        span: Span,
    },
    /// `{...expr}`; always the only part of its word.
    Spread { tokens: Vec<Token>, span: Span },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub parts: Vec<WordPart>,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirectKind {
    /// `>`
    Out,
    /// `>>`
    Append,
    /// `<`
    In,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Redirect {
    pub kind: RedirectKind,
    pub target: Word,
    pub span: Span,
}

/// Everything after `run` / `run!` in command mode.
#[derive(Clone, Debug, PartialEq)]
pub struct CommandTok {
    pub bang: bool,
    pub words: Vec<Word>,
    pub redirects: Vec<Redirect>,
    /// Tokens of the `with {...}` record literal, braces included.
    pub with: Option<Vec<Token>>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Kw(Kw),
    Int(i64),
    Float(f64),
    /// Nanoseconds.
    Duration(i64),
    Str(Vec<StrPart>),
    /// `run ...` / `run! ...` in command mode.
    Command(Box<CommandTok>),
    /// `run(` / `run!(`: the function form. The `(` follows as its own token.
    RunFn {
        bang: bool,
    },
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Assign,
    PlusEq,
    MinusEq,
    FatArrow,
    Coalesce,
    Question,
    Pipe,
    Dot,
    Ellipsis,
    Comma,
    Colon,
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Newline,
    Eof,
}

impl Tok {
    /// How the token is described in "expected X, found Y" messages.
    pub fn describe(&self) -> String {
        match self {
            Tok::Ident(s) => format!("`{s}`"),
            Tok::Kw(k) => format!("`{}`", k.as_str()),
            Tok::Int(_) | Tok::Float(_) => "a number".into(),
            Tok::Duration(_) => "a duration".into(),
            Tok::Str(_) => "a string".into(),
            Tok::Command(_) => "a command".into(),
            Tok::RunFn { .. } => "`run(`".into(),
            Tok::Newline => "end of line".into(),
            Tok::Eof => "end of file".into(),
            other => format!("`{}`", other.symbol()),
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            Tok::Plus => "+",
            Tok::Minus => "-",
            Tok::Star => "*",
            Tok::Slash => "/",
            Tok::Percent => "%",
            Tok::EqEq => "==",
            Tok::NotEq => "!=",
            Tok::Lt => "<",
            Tok::Le => "<=",
            Tok::Gt => ">",
            Tok::Ge => ">=",
            Tok::Assign => "=",
            Tok::PlusEq => "+=",
            Tok::MinusEq => "-=",
            Tok::FatArrow => "=>",
            Tok::Coalesce => "??",
            Tok::Question => "?",
            Tok::Pipe => "|",
            Tok::Dot => ".",
            Tok::Ellipsis => "...",
            Tok::Comma => ",",
            Tok::Colon => ":",
            Tok::LParen => "(",
            Tok::RParen => ")",
            Tok::LBracket => "[",
            Tok::RBracket => "]",
            Tok::LBrace => "{",
            Tok::RBrace => "}",
            _ => "?",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
    /// True when whitespace, a newline or a comment separates this token from
    /// the previous one. Calls (`f(x)`), indexing (`xs[0]`) and pipeline
    /// juxtaposition depend on it.
    pub space_before: bool,
}

/// A `#` comment, kept for doc comments and parameter help.
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    /// Text after `#` with one leading space removed.
    pub text: String,
    pub span: Span,
    /// True when nothing but whitespace precedes the comment on its line.
    pub own_line: bool,
}

pub struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
}

/// Lex a whole file.
pub fn lex(file: FileId, src: &str) -> Result<Lexed, OkError> {
    let mut lx = Lexer {
        src,
        bytes: src.as_bytes(),
        pos: 0,
        file,
        comments: Vec::new(),
        depth: 0,
    };
    if src.starts_with('\u{feff}') {
        lx.pos = 3;
    }
    let (tokens, _) = lx.tokens(Mode::Top)?;
    Ok(Lexed {
        tokens: fix_newlines(tokens),
        comments: lx.comments,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A whole file, until end of input.
    Top,
    /// Inside `{...}` of a string or command interpolation: stop at the `}`
    /// that closes it. `spec` allows `:spec` at depth 0 (strings only).
    Interp { spec: bool },
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    file: FileId,
    comments: Vec<Comment>,
    /// Nesting of interpolations inside strings inside interpolations.
    depth: usize,
}

/// Maximum nesting of interpolations, so hostile input cannot overflow the stack.
const MAX_INTERP_DEPTH: usize = 32;

fn is_ident_start(c: u8) -> bool {
    c.is_ascii_alphabetic() || c == b'_'
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

const NS_PER_MS: i128 = 1_000_000;
const NS_PER_S: i128 = 1_000_000_000;

impl<'a> Lexer<'a> {
    fn span(&self, start: usize, end: usize) -> Span {
        Span::new(self.file, start, end)
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, n: usize) -> Option<u8> {
        self.bytes.get(self.pos + n).copied()
    }

    fn err(&self, start: usize, end: usize, msg: impl Into<String>) -> OkError {
        OkError::at(ErrKind::Syntax, self.span(start, end), msg)
    }

    fn current_char(&self) -> char {
        self.src[self.pos..].chars().next().unwrap_or('\0')
    }

    /// Whether only spaces and tabs precede `pos` on its line.
    fn at_line_start(&self, pos: usize) -> bool {
        self.src[..pos]
            .bytes()
            .rev()
            .take_while(|&b| b != b'\n')
            .all(|b| b == b' ' || b == b'\t')
    }

    fn comment(&mut self) {
        let start = self.pos;
        let own_line = self.at_line_start(start);
        while let Some(c) = self.peek() {
            if c == b'\n' || (c == b'\r' && self.peek_at(1) == Some(b'\n')) {
                break;
            }
            self.pos += 1;
        }
        let raw = &self.src[start + 1..self.pos];
        let text = raw.strip_prefix(' ').unwrap_or(raw).trim_end().to_string();
        self.comments.push(Comment {
            text,
            span: self.span(start, self.pos),
            own_line,
        });
    }

    /// Lex tokens until the end of the mode. Returns the tokens and, for
    /// interpolations, the format spec after `:`.
    fn tokens(&mut self, mode: Mode) -> Result<(Vec<Token>, Option<String>), OkError> {
        if self.depth >= MAX_INTERP_DEPTH {
            return Err(self.err(
                self.pos,
                self.pos + 1,
                "interpolations are nested too deeply",
            ));
        }
        self.depth += 1;
        let r = self.tokens_inner(mode);
        self.depth -= 1;
        r
    }

    fn tokens_inner(&mut self, mode: Mode) -> Result<(Vec<Token>, Option<String>), OkError> {
        let mut out: Vec<Token> = Vec::new();
        // Open brackets, to decide whether newlines matter and to find the
        // end of an interpolation.
        let mut stack: Vec<(u8, usize)> = Vec::new();
        let mut prev_end: Option<usize> = if mode == Mode::Top {
            None
        } else {
            Some(self.pos)
        };

        loop {
            let Some(c) = self.peek() else {
                if let Some(&(open, at)) = stack.last() {
                    return Err(self.err(
                        at,
                        at + 1,
                        format!("`{}` is never closed", open as char),
                    ));
                }
                if mode != Mode::Top {
                    return Err(self.err(
                        self.pos,
                        self.pos,
                        "unterminated interpolation: missing `}`",
                    ));
                }
                out.push(Token {
                    tok: Tok::Eof,
                    span: self.span(self.pos, self.pos),
                    space_before: true,
                });
                return Ok((out, None));
            };
            let start = self.pos;

            match c {
                b' ' | b'\t' => {
                    self.pos += 1;
                    continue;
                }
                b'\r' if self.peek_at(1) != Some(b'\n') => {
                    return Err(self.err(
                        start,
                        start + 1,
                        "a carriage return must be followed by a newline",
                    ));
                }
                b'\r' | b'\n' => {
                    self.pos += if c == b'\r' { 2 } else { 1 };
                    let in_brackets = matches!(stack.last(), Some((b'(', _)) | Some((b'[', _)));
                    if mode == Mode::Top && !in_brackets {
                        if !matches!(
                            out.last(),
                            Some(Token {
                                tok: Tok::Newline,
                                ..
                            }) | None
                        ) {
                            out.push(Token {
                                tok: Tok::Newline,
                                span: self.span(start, start),
                                space_before: true,
                            });
                        }
                        prev_end = Some(start);
                    }
                    continue;
                }
                b'#' => {
                    self.comment();
                    continue;
                }
                _ => {}
            }

            if mode != Mode::Top && stack.is_empty() {
                if c == b'}' {
                    self.pos += 1;
                    return Ok((out, None));
                }
                if c == b':' && mode == (Mode::Interp { spec: true }) {
                    self.pos += 1;
                    let spec_start = self.pos;
                    while let Some(ch) = self.peek() {
                        if ch == b'}' {
                            break;
                        }
                        if ch == b'\n' || ch == b'"' {
                            return Err(self.err(
                                spec_start,
                                self.pos,
                                "unterminated format spec: missing `}`",
                            ));
                        }
                        self.pos += 1;
                    }
                    if self.peek().is_none() {
                        return Err(self.err(
                            spec_start,
                            self.pos,
                            "unterminated format spec: missing `}`",
                        ));
                    }
                    let spec = self.src[spec_start..self.pos].to_string();
                    self.pos += 1;
                    return Ok((out, Some(spec)));
                }
            }

            let tok = self.token(c, &mut stack, mode)?;
            let space_before = prev_end != Some(start);
            prev_end = Some(self.pos);
            out.push(Token {
                tok,
                span: self.span(start, self.pos),
                space_before,
            });
        }
    }

    /// Lex one token starting with byte `c` at `self.pos`.
    fn token(&mut self, c: u8, stack: &mut Vec<(u8, usize)>, mode: Mode) -> Result<Tok, OkError> {
        let start = self.pos;
        if c.is_ascii_digit() {
            return self.number();
        }
        if is_ident_start(c) {
            return self.word_token();
        }
        if c == b'"' {
            self.pos += 1;
            let triple = self.src[self.pos..].starts_with("\"\"");
            if triple {
                self.pos += 2;
            }
            return Ok(Tok::Str(self.dq_string(start, triple)?));
        }
        if c == b'\'' {
            self.pos += 1;
            let triple = self.src[self.pos..].starts_with("''");
            if triple {
                self.pos += 2;
            }
            return Ok(Tok::Str(vec![StrPart::Lit(
                self.raw_string(start, triple)?,
            )]));
        }
        let two = |s: &Self, a: u8| s.peek_at(1) == Some(a);
        let (tok, len) = match c {
            b'+' if two(self, b'=') => (Tok::PlusEq, 2),
            b'+' => (Tok::Plus, 1),
            b'-' if two(self, b'=') => (Tok::MinusEq, 2),
            b'-' => (Tok::Minus, 1),
            b'*' => (Tok::Star, 1),
            b'/' => (Tok::Slash, 1),
            b'%' => (Tok::Percent, 1),
            b'=' if self.src[start..].starts_with("===") => {
                return Err(self
                    .err(start, start + 3, "`===` is not an ok operator")
                    .with_help("use `==`; ok compares values structurally"));
            }
            b'=' if two(self, b'=') => (Tok::EqEq, 2),
            b'=' if two(self, b'>') => (Tok::FatArrow, 2),
            b'=' => (Tok::Assign, 1),
            b'!' if two(self, b'=') => (Tok::NotEq, 2),
            b'!' => {
                return Err(self
                    .err(start, start + 1, "`!` is not an ok operator")
                    .with_help("use `not` for negation, as in `not done`"));
            }
            b'<' if two(self, b'=') => (Tok::Le, 2),
            b'<' => (Tok::Lt, 1),
            b'>' if two(self, b'=') => (Tok::Ge, 2),
            b'>' => (Tok::Gt, 1),
            b'?' if two(self, b'?') => (Tok::Coalesce, 2),
            b'?' => (Tok::Question, 1),
            b'|' if two(self, b'|') => {
                return Err(self
                    .err(start, start + 2, "`||` is not an ok operator")
                    .with_help("use `or`, as in `a or b`"));
            }
            b'|' => (Tok::Pipe, 1),
            b'&' if two(self, b'&') => {
                return Err(self
                    .err(start, start + 2, "`&&` is not an ok operator")
                    .with_help("use `and`, as in `a and b`"));
            }
            b'.' if self.src[start..].starts_with("...") => (Tok::Ellipsis, 3),
            b'.' => (Tok::Dot, 1),
            b',' => (Tok::Comma, 1),
            b':' => (Tok::Colon, 1),
            b';' => {
                return Err(self
                    .err(start, start + 1, "ok does not use semicolons")
                    .with_help("end the statement with a new line instead"));
            }
            b'(' | b'[' | b'{' => {
                stack.push((c, start));
                let tok = match c {
                    b'(' => Tok::LParen,
                    b'[' => Tok::LBracket,
                    _ => Tok::LBrace,
                };
                (tok, 1)
            }
            b')' | b']' | b'}' => {
                let open = match c {
                    b')' => b'(',
                    b']' => b'[',
                    _ => b'{',
                };
                match stack.pop() {
                    Some((o, _)) if o == open => {}
                    Some((o, at)) => {
                        return Err(self
                            .err(
                                start,
                                start + 1,
                                format!(
                                    "`{}` does not match the `{}` opened here",
                                    c as char, o as char
                                ),
                            )
                            .with_note(format!("the `{}` is at byte {at}", o as char)));
                    }
                    None => {
                        let _ = mode;
                        return Err(self.err(
                            start,
                            start + 1,
                            format!("unexpected `{}`: nothing to close", c as char),
                        ));
                    }
                }
                let tok = match c {
                    b')' => Tok::RParen,
                    b']' => Tok::RBracket,
                    _ => Tok::RBrace,
                };
                (tok, 1)
            }
            b'`' => {
                return Err(self
                    .err(start, start + 1, "backticks are not ok syntax")
                    .with_help(
                        "use \"...\" for strings, or `(run cmd).out` for a command's output",
                    ));
            }
            _ => {
                let ch = self.current_char();
                let len = ch.len_utf8();
                let help = if ch.is_alphabetic() {
                    Some("identifiers are ASCII letters, digits and `_`".to_string())
                } else {
                    None
                };
                return Err(self
                    .err(start, start + len, format!("unexpected character `{ch}`"))
                    .maybe_help(help));
            }
        };
        self.pos += len;
        Ok(tok)
    }

    fn word_token(&mut self) -> Result<Tok, OkError> {
        let start = self.pos;
        while self.peek().is_some_and(is_ident_char) {
            self.pos += 1;
        }
        let word = &self.src[start..self.pos];
        if !self.peek().is_some_and(|b| b < 0x80) && self.peek().is_some() {
            let ch = self.current_char();
            if ch.is_alphabetic() {
                return Err(self
                    .err(
                        start,
                        self.pos + ch.len_utf8(),
                        format!("unexpected character `{ch}` in a name"),
                    )
                    .with_help("identifiers are ASCII letters, digits and `_`"));
            }
        }
        if word == "run" {
            if let Some(tok) = self.run_keyword(start)? {
                return Ok(tok);
            }
            return Ok(Tok::Ident(word.to_string()));
        }
        Ok(match Kw::from_word(word) {
            Some(kw) => Tok::Kw(kw),
            None => Tok::Ident(word.to_string()),
        })
    }

    /// Decide what `run` at `start` is (SPEC.md 3.4, 9.1): the function form
    /// `run(`, a command in command mode, or a plain identifier (after `.`,
    /// glued to a `-` as in `--dry-run`, or followed by anything else).
    fn run_keyword(&mut self, start: usize) -> Result<Option<Tok>, OkError> {
        let before = self.src[..start].bytes().next_back();
        if matches!(before, Some(b'.') | Some(b'-')) {
            return Ok(None);
        }
        let mut bang = false;
        if self.peek() == Some(b'!') && self.peek_at(1) != Some(b'=') {
            bang = true;
        }
        let after = self.peek_at(usize::from(bang));
        match after {
            Some(b'(') => {
                self.pos += usize::from(bang);
                Ok(Some(Tok::RunFn { bang }))
            }
            Some(b' ') | Some(b'\t') => {
                self.pos += usize::from(bang);
                let cmd = self.command(start, bang)?;
                Ok(Some(Tok::Command(Box::new(cmd))))
            }
            _ if bang => Err(self.err(
                start,
                self.pos + 1,
                "`run!` must be followed by a command or `(`",
            )),
            _ => Ok(None),
        }
    }

    // ----- numbers ---------------------------------------------------------

    fn digits(&mut self, radix: u32) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if c == b'_' {
                self.pos += 1;
            } else if (c as char).is_digit(radix) {
                s.push(c as char);
                self.pos += 1;
            } else {
                break;
            }
        }
        s
    }

    fn number(&mut self) -> Result<Tok, OkError> {
        let start = self.pos;
        if self.peek() == Some(b'0') && matches!(self.peek_at(1), Some(b'x' | b'o' | b'b')) {
            let radix = match self.peek_at(1) {
                Some(b'x') => 16,
                Some(b'o') => 8,
                _ => 2,
            };
            self.pos += 2;
            let ds = self.digits(radix);
            if ds.is_empty() || self.peek().is_some_and(is_ident_char) {
                return Err(self.err(start, self.pos + 1, "invalid number literal"));
            }
            return i64::from_str_radix(&ds, radix).map(Tok::Int).map_err(|_| {
                self.err(start, self.pos, "integer literal is too large for 64 bits")
            });
        }

        let (text, is_float) = self.decimal();
        if self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            return self.duration(start, text, is_float);
        }
        if self.peek().is_some_and(is_ident_char) {
            return Err(self.err(start, self.pos + 1, "invalid number literal"));
        }
        if is_float {
            let v: f64 = text
                .parse()
                .map_err(|_| self.err(start, self.pos, "invalid number literal"))?;
            if !v.is_finite() {
                return Err(self.err(start, self.pos, "float literal is out of range"));
            }
            Ok(Tok::Float(v))
        } else {
            text.parse::<i64>()
                .map(Tok::Int)
                .map_err(|_| self.err(start, self.pos, "integer literal is too large for 64 bits"))
        }
    }

    /// Lex `123`, `1_000`, `1.5`, `2.5e-3`. Returns the text without
    /// underscores and whether it is a float.
    fn decimal(&mut self) -> (String, bool) {
        let mut text = self.digits(10);
        let mut is_float = false;
        if self.peek() == Some(b'.') && self.peek_at(1).is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
            text.push('.');
            text.push_str(&self.digits(10));
            is_float = true;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            let sign = matches!(self.peek_at(1), Some(b'+' | b'-'));
            let digit_at = if sign { 2 } else { 1 };
            if self.peek_at(digit_at).is_some_and(|c| c.is_ascii_digit()) {
                text.push('e');
                if sign {
                    text.push(self.peek_at(1).unwrap_or(b'+') as char);
                }
                self.pos += digit_at;
                text.push_str(&self.digits(10));
                is_float = true;
            }
        }
        (text, is_float)
    }

    /// A duration literal: `250ms`, `1.5s`, `1h30m`. `first` is the already
    /// lexed number before the first unit.
    fn duration(&mut self, start: usize, first: String, first_float: bool) -> Result<Tok, OkError> {
        let mut total: i128 = 0;
        let mut last_rank = usize::MAX;
        let mut number = (first, first_float);
        loop {
            let (unit_ns, rank, len) = if self.src[self.pos..].starts_with("ms") {
                (NS_PER_MS, 0, 2)
            } else {
                match self.peek() {
                    Some(b's') => (NS_PER_S, 1, 1),
                    Some(b'm') => (60 * NS_PER_S, 2, 1),
                    Some(b'h') => (3600 * NS_PER_S, 3, 1),
                    Some(b'd') => (86400 * NS_PER_S, 4, 1),
                    _ => {
                        let end = self.pos
                            + self.src[self.pos..]
                                .bytes()
                                .take_while(|&b| is_ident_char(b))
                                .count();
                        return Err(self
                            .err(
                                start,
                                end,
                                format!("invalid number suffix `{}`", &self.src[self.pos..end]),
                            )
                            .with_help(
                                "durations use the units ms, s, m, h and d, as in 250ms or 1h30m",
                            ));
                    }
                }
            };
            if rank >= last_rank {
                return Err(self
                    .err(
                        start,
                        self.pos + len,
                        "duration units must go from largest to smallest",
                    )
                    .with_help("write it like 1h30m or 2m5s"));
            }
            last_rank = rank;
            self.pos += len;
            let part = if number.1 {
                let v: f64 = number.0.parse().unwrap_or(f64::NAN);
                let ns = v * unit_ns as f64;
                if !ns.is_finite() || ns.abs() > i64::MAX as f64 {
                    return Err(self.err(start, self.pos, "duration literal is too large"));
                }
                ns.round() as i128
            } else {
                let v: i128 = number
                    .0
                    .parse()
                    .map_err(|_| self.err(start, self.pos, "duration literal is too large"))?;
                v.checked_mul(unit_ns)
                    .ok_or_else(|| self.err(start, self.pos, "duration literal is too large"))?
            };
            total = total
                .checked_add(part)
                .ok_or_else(|| self.err(start, self.pos, "duration literal is too large"))?;
            if self.peek().is_some_and(|c| c.is_ascii_digit()) {
                number = self.decimal();
                if !self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
                    return Err(self.err(
                        start,
                        self.pos,
                        "a number inside a duration needs a unit",
                    ));
                }
                continue;
            }
            if self.peek().is_some_and(is_ident_char) {
                let end = self.pos
                    + self.src[self.pos..]
                        .bytes()
                        .take_while(|&b| is_ident_char(b))
                        .count();
                return Err(self
                    .err(start, end, "invalid duration literal")
                    .with_help("durations use the units ms, s, m, h and d, as in 250ms or 1h30m"));
            }
            break;
        }
        i64::try_from(total)
            .map(Tok::Duration)
            .map_err(|_| self.err(start, self.pos, "duration literal is too large"))
    }

    // ----- strings ---------------------------------------------------------

    /// Find the byte offset of the closing `"""` or `'''` after `self.pos`,
    /// skipping backslash escapes in double-quoted strings.
    fn find_triple_end(&self, quote: u8, escapes: bool) -> Option<usize> {
        let mut i = self.pos;
        let b = self.bytes;
        while i + 3 <= b.len() {
            if escapes && b[i] == b'\\' {
                i += 2;
                continue;
            }
            if b[i] == quote && b[i + 1] == quote && b[i + 2] == quote {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Dedent parameters of a triple-quoted string (SPEC.md 3.5): whether the
    /// opening line break is skipped, the indentation removed from each line,
    /// and where the content stops (before a closing line that holds only
    /// indentation).
    fn triple_layout(&self, close: usize) -> (usize, usize, usize) {
        let content = &self.src[self.pos..close];
        let mut begin = self.pos;
        if content.starts_with("\r\n") {
            begin += 2;
        } else if content.starts_with('\n') {
            begin += 1;
        }
        let body = &self.src[begin..close];
        if !body.contains('\n') && begin == self.pos {
            return (begin, 0, close);
        }
        let last_nl = body.rfind('\n');
        let closing_line = match last_nl {
            Some(i) => &body[i + 1..],
            None => body,
        };
        let closing_own_line = closing_line.bytes().all(|b| b == b' ' || b == b'\t');
        let mut indent = usize::MAX;
        let lines: Vec<&str> = body.split('\n').collect();
        let count = lines.len();
        for (i, line) in lines.iter().enumerate() {
            let line = line.trim_end_matches('\r');
            let is_last = i + 1 == count;
            if is_last && closing_own_line {
                indent = indent.min(line.len());
                continue;
            }
            if line.trim().is_empty() {
                continue;
            }
            indent = indent.min(
                line.bytes()
                    .take_while(|&b| b == b' ' || b == b'\t')
                    .count(),
            );
        }
        if indent == usize::MAX {
            indent = 0;
        }
        let end = if closing_own_line {
            match last_nl {
                Some(i) => begin + i + 1,
                None => begin,
            }
        } else {
            close
        };
        (begin, indent, end)
    }

    /// Skip up to `n` spaces or tabs (dedent of one line).
    fn skip_indent(&mut self, n: usize, limit: usize) {
        let mut k = 0;
        while k < n && self.pos < limit && matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
            k += 1;
        }
    }

    /// Lex a double-quoted string after its opening quote(s).
    fn dq_string(&mut self, open: usize, triple: bool) -> Result<Vec<StrPart>, OkError> {
        let (limit, indent, close_len) = if triple {
            let Some(close) = self.find_triple_end(b'"', true) else {
                return Err(self.err(
                    open,
                    open + 3,
                    "unterminated string: missing closing `\"\"\"`",
                ));
            };
            let (begin, indent, end) = self.triple_layout(close);
            self.pos = begin;
            self.skip_indent(indent, end);
            // After the content, jump to the closing quotes.
            (end, indent, close + 3)
        } else {
            (usize::MAX, 0, 0)
        };

        let mut parts = Vec::new();
        let mut lit = String::new();
        loop {
            if triple && self.pos >= limit {
                self.pos = close_len;
                break;
            }
            let Some(c) = self.peek() else {
                return Err(self.err(open, open + 1, "unterminated string: missing closing `\"`"));
            };
            match c {
                b'"' if !triple => {
                    self.pos += 1;
                    break;
                }
                b'\\' => {
                    let esc_start = self.pos;
                    self.pos += 1;
                    let Some(e) = self.peek() else {
                        return Err(self.err(
                            open,
                            open + 1,
                            "unterminated string: missing closing `\"`",
                        ));
                    };
                    self.pos += 1;
                    match e {
                        b'n' => lit.push('\n'),
                        b'r' => lit.push('\r'),
                        b't' => lit.push('\t'),
                        b'0' => lit.push('\0'),
                        b'\\' => lit.push('\\'),
                        b'"' => lit.push('"'),
                        b'{' => lit.push('{'),
                        b'}' => lit.push('}'),
                        b'u' => lit.push(self.unicode_escape(esc_start)?),
                        _ => {
                            let ch = self.src[esc_start + 1..].chars().next().unwrap_or('?');
                            self.pos = esc_start + 1 + ch.len_utf8();
                            return Err(self
                                .err(esc_start, self.pos, format!("unknown escape `\\{ch}`"))
                                .with_help("escapes are \\n \\r \\t \\0 \\\\ \\\" \\{ \\} and \\u{...}; use a single-quoted string for raw text"));
                        }
                    }
                }
                b'{' => {
                    let brace = self.pos;
                    self.pos += 1;
                    if !lit.is_empty() {
                        parts.push(StrPart::Lit(std::mem::take(&mut lit)));
                    }
                    let (tokens, spec) = self.tokens(Mode::Interp { spec: true }).map_err(|e| {
                        if e.help.is_some() {
                            return e;
                        }
                        e.with_help("`{` in a double-quoted string starts an interpolation; write \\{ for a literal brace, or use a single-quoted string like '{\"a\": 1}'")
                    })?;
                    if tokens.is_empty() {
                        return Err(self
                            .err(brace, self.pos, "empty interpolation `{}`")
                            .with_help("write `\\{` for a literal brace"));
                    }
                    parts.push(StrPart::Interp {
                        tokens,
                        spec,
                        span: self.span(brace, self.pos),
                    });
                }
                b'\r' if self.peek_at(1) == Some(b'\n') => {
                    self.pos += 1;
                }
                b'\n' => {
                    self.pos += 1;
                    lit.push('\n');
                    if triple {
                        self.skip_indent(indent, limit);
                    }
                }
                _ => {
                    let ch = self.current_char();
                    lit.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        if !lit.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(lit));
        }
        Ok(parts)
    }

    fn unicode_escape(&mut self, esc_start: usize) -> Result<char, OkError> {
        if self.peek() != Some(b'{') {
            return Err(self.err(
                esc_start,
                self.pos,
                "a unicode escape looks like \\u{1F600}",
            ));
        }
        self.pos += 1;
        let hex_start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
            self.pos += 1;
        }
        let hex = &self.src[hex_start..self.pos];
        if self.peek() != Some(b'}') || hex.is_empty() || hex.len() > 6 {
            return Err(self.err(
                esc_start,
                self.pos,
                "a unicode escape looks like \\u{1F600}",
            ));
        }
        self.pos += 1;
        u32::from_str_radix(hex, 16)
            .ok()
            .and_then(char::from_u32)
            .ok_or_else(|| {
                self.err(
                    esc_start,
                    self.pos,
                    format!("\\u{{{hex}}} is not a Unicode scalar value"),
                )
            })
    }

    /// Lex a single-quoted raw string after its opening quote(s).
    fn raw_string(&mut self, open: usize, triple: bool) -> Result<String, OkError> {
        if triple {
            let Some(close) = self.find_triple_end(b'\'', false) else {
                return Err(self.err(open, open + 3, "unterminated string: missing closing `'''`"));
            };
            let (begin, indent, end) = self.triple_layout(close);
            self.pos = begin;
            let mut out = String::new();
            self.skip_indent(indent, end);
            while self.pos < end {
                let ch = self.current_char();
                self.pos += ch.len_utf8();
                match ch {
                    '\r' if self.peek() == Some(b'\n') => {}
                    '\n' => {
                        out.push('\n');
                        self.skip_indent(indent, end);
                    }
                    _ => out.push(ch),
                }
            }
            self.pos = close + 3;
            return Ok(out);
        }
        let mut out = String::new();
        loop {
            let Some(c) = self.peek() else {
                return Err(self
                    .err(open, open + 1, "unterminated string: missing closing `'`")
                    .with_help("single-quoted strings cannot contain `'`; use double quotes"));
            };
            if c == b'\'' {
                self.pos += 1;
                return Ok(out);
            }
            let ch = self.current_char();
            self.pos += ch.len_utf8();
            if ch == '\r' && self.peek() == Some(b'\n') {
                continue;
            }
            out.push(ch);
        }
    }

    // ----- command mode ------------------------------------------------------

    fn skip_blanks(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    /// `\` as the last character of a line continues a command (SPEC.md 9.2
    /// rule 11). Returns true if it consumed one.
    fn line_continuation(&mut self) -> bool {
        if self.peek() != Some(b'\\') {
            return false;
        }
        let rest = &self.src[self.pos + 1..];
        let nl = if rest.starts_with("\r\n") {
            2
        } else if rest.starts_with('\n') {
            1
        } else {
            return false;
        };
        self.pos += 1 + nl;
        true
    }

    /// Lex a command after `run` / `run!` (SPEC.md 9.2).
    fn command(&mut self, run_start: usize, bang: bool) -> Result<CommandTok, OkError> {
        let mut words: Vec<Word> = Vec::new();
        let mut redirects: Vec<Redirect> = Vec::new();
        let mut with = None;
        loop {
            self.skip_blanks();
            if self.line_continuation() {
                continue;
            }
            let Some(c) = self.peek() else { break };
            let start = self.pos;
            match c {
                b'\n' | b'\r' | b'|' | b')' | b'}' => break,
                b'#' => {
                    self.comment();
                    break;
                }
                b'>' | b'<' => {
                    let kind = if c == b'<' {
                        self.pos += 1;
                        RedirectKind::In
                    } else if self.peek_at(1) == Some(b'>') {
                        self.pos += 2;
                        RedirectKind::Append
                    } else {
                        self.pos += 1;
                        RedirectKind::Out
                    };
                    self.skip_blanks();
                    if matches!(
                        self.peek(),
                        None | Some(b'\n' | b'\r' | b'|' | b')' | b'}' | b'>' | b'<' | b'#')
                    ) {
                        return Err(self
                            .err(start, self.pos, "a redirect needs a target file")
                            .with_help("for example: run make > build.log"));
                    }
                    let target = self.word(words.is_empty())?;
                    let span = self.span(start, self.pos);
                    redirects.push(Redirect { kind, target, span });
                    continue;
                }
                _ => {}
            }
            if with.is_none() && self.at_with_clause() {
                self.pos += 4;
                self.skip_blanks();
                let open = self.pos;
                self.pos += 1;
                let (inner, _) = self.tokens(Mode::Interp { spec: false })?;
                let mut toks = vec![Token {
                    tok: Tok::LBrace,
                    span: self.span(open, open + 1),
                    space_before: true,
                }];
                toks.extend(inner);
                toks.push(Token {
                    tok: Tok::RBrace,
                    span: self.span(self.pos - 1, self.pos),
                    space_before: false,
                });
                with = Some(toks);
                self.skip_blanks();
                if self.peek() == Some(b'#') {
                    self.comment();
                }
                if !matches!(self.peek(), None | Some(b'\n' | b'\r' | b'|' | b')' | b'}')) {
                    return Err(self.err(
                        self.pos,
                        self.pos + 1,
                        "nothing may follow the `with {...}` options of a command",
                    ));
                }
                break;
            }
            if !redirects.is_empty() {
                return Err(self
                    .err(start, start + 1, "command words must come before redirects")
                    .with_help("move the redirect to the end of the command"));
            }
            let w = self.word(words.is_empty())?;
            words.push(w);
        }
        if words.is_empty() {
            return Err(self
                .err(run_start, self.pos, "`run` needs a command")
                .with_help("for example: run cargo build"));
        }
        Ok(CommandTok {
            bang,
            words,
            redirects,
            with,
        })
    }

    /// `with` followed by blanks and `{` starts the options clause.
    fn at_with_clause(&self) -> bool {
        let rest = &self.src[self.pos..];
        if !rest.starts_with("with") {
            return false;
        }
        let after = &rest[4..];
        let trimmed = after.trim_start_matches([' ', '\t']);
        trimmed.len() < after.len() && trimmed.starts_with('{')
    }

    /// Lex one command word (SPEC.md 9.2).
    fn word(&mut self, first: bool) -> Result<Word, OkError> {
        let start = self.pos;
        let mut parts: Vec<WordPart> = Vec::new();
        let mut bare = String::new();
        let flush = |bare: &mut String, parts: &mut Vec<WordPart>| {
            if !bare.is_empty() {
                parts.push(WordPart::Lit {
                    text: std::mem::take(bare),
                    quoted: false,
                });
            }
        };
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\n' | b'|' | b'>' | b'<' | b')' | b'}' => break,
                b'\r' => break,
                b'\\'
                    if self.src[self.pos + 1..].starts_with('\n')
                        || self.src[self.pos + 1..].starts_with("\r\n") =>
                {
                    break;
                }
                b'"' => {
                    flush(&mut bare, &mut parts);
                    let open = self.pos;
                    self.pos += 1;
                    let triple = self.src[self.pos..].starts_with("\"\"");
                    if triple {
                        self.pos += 2;
                    }
                    for part in self.dq_string(open, triple)? {
                        parts.push(match part {
                            StrPart::Lit(text) => WordPart::Lit { text, quoted: true },
                            StrPart::Interp { tokens, spec, span } => {
                                WordPart::Interp { tokens, spec, span }
                            }
                        });
                    }
                }
                b'\'' => {
                    flush(&mut bare, &mut parts);
                    let open = self.pos;
                    self.pos += 1;
                    let triple = self.src[self.pos..].starts_with("''");
                    if triple {
                        self.pos += 2;
                    }
                    let text = self.raw_string(open, triple)?;
                    parts.push(WordPart::Lit { text, quoted: true });
                }
                b'{' => {
                    flush(&mut bare, &mut parts);
                    let brace = self.pos;
                    self.pos += 1;
                    let spread = self.src[self.pos..].starts_with("...");
                    if spread {
                        self.pos += 3;
                    }
                    let (tokens, _) = self.tokens(Mode::Interp { spec: false })?;
                    let span = self.span(brace, self.pos);
                    if tokens.is_empty() {
                        return Err(
                            OkError::at(ErrKind::Syntax, span, "empty interpolation `{}`")
                                .with_help("quote a literal brace: '{}'"),
                        );
                    }
                    if spread {
                        let alone = parts.is_empty()
                            && matches!(
                                self.peek(),
                                None | Some(
                                    b' ' | b'\t' | b'\n' | b'\r' | b'|' | b')' | b'}' | b'>' | b'<'
                                )
                            );
                        if !alone {
                            return Err(OkError::at(ErrKind::Syntax, span, "a spread `{...list}` must be a whole word")
                                .with_help("put spaces around it; to join a list into one argument use {join(list, \",\")}"));
                        }
                        parts.push(WordPart::Spread { tokens, span });
                    } else {
                        parts.push(WordPart::Interp {
                            tokens,
                            spec: None,
                            span,
                        });
                    }
                }
                b';' => {
                    return Err(self
                        .err(self.pos, self.pos + 1, "`;` is not allowed in a command: ok commands are not shell")
                        .with_help("put each command on its own `run` line, or quote the `;` to pass it as an argument"));
                }
                b'(' => {
                    return Err(self
                        .err(self.pos, self.pos + 1, "unquoted `(` in a command")
                        .with_help("quote it ('(') to pass it as an argument, or use the function form run(\"prog\", arg)"));
                }
                b'$' if self.peek_at(1) == Some(b'(') => {
                    return Err(self
                        .err(self.pos, self.pos + 2, "`$(...)` is shell syntax")
                        .with_help(
                            "capture the output first: let x = (run cmd).out, then use {x}",
                        ));
                }
                b'`' => {
                    return Err(self
                        .err(self.pos, self.pos + 1, "backticks are shell syntax")
                        .with_help(
                            "capture the output first: let x = (run cmd).out, then use {x}",
                        ));
                }
                b'#' if parts.is_empty() && bare.is_empty() => break,
                _ => {
                    let ch = self.current_char();
                    bare.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
        }
        flush(&mut bare, &mut parts);
        let span = self.span(start, self.pos);

        // Shell habits (SPEC.md 9.2 rules 7 and 9).
        if let [
            WordPart::Lit {
                text,
                quoted: false,
            },
        ] = parts.as_slice()
            && matches!(text.as_str(), "&&" | "||" | "&")
        {
            let help = match text.as_str() {
                "&&" => {
                    "ok commands are not shell: put the next command on its own `run` line (a failure already stops the script)"
                }
                "||" => "ok commands are not shell: use `run!` and check `.ok`, or try/catch",
                _ => {
                    "ok commands are not shell and cannot run in the background; quote '&' to pass it as an argument"
                }
            };
            return Err(OkError::at(
                ErrKind::Syntax,
                span,
                format!("`{text}` is not allowed in a command"),
            )
            .with_help(help));
        }
        if first
            && let Some(WordPart::Lit {
                text,
                quoted: false,
            }) = parts.first()
            && let Some(eq) = text.find('=')
        {
            let name = &text[..eq];
            if !name.is_empty()
                && name.bytes().all(is_ident_char)
                && !name.as_bytes()[0].is_ascii_digit()
            {
                return Err(OkError::at(ErrKind::Syntax, span, format!("`{text}` looks like a shell environment assignment"))
                            .with_help(format!("set environment variables with options: run cmd with {{env: {{{name}: \"...\"}}}}")));
            }
        }
        Ok(Word { parts, span })
    }
}

/// Apply the newline rules of SPEC.md 3.3: drop a newline after a token that
/// cannot end a statement, and before a line starting with `|`, `.name`,
/// `else` or `catch`.
pub fn fix_newlines(tokens: Vec<Token>) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::with_capacity(tokens.len());
    let n = tokens.len();
    let mut i = 0;
    while i < n {
        let t = &tokens[i];
        if t.tok == Tok::Newline {
            let prev_continues = match out.last() {
                None => true,
                Some(p) => matches!(
                    p.tok,
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
                        | Tok::Kw(Kw::Not)
                        | Tok::Kw(Kw::In)
                        | Tok::Coalesce
                        | Tok::Pipe
                        | Tok::Assign
                        | Tok::PlusEq
                        | Tok::MinusEq
                        | Tok::FatArrow
                        | Tok::Comma
                        | Tok::Dot
                        | Tok::Colon
                        | Tok::LParen
                        | Tok::LBracket
                        | Tok::LBrace
                        | Tok::Newline
                ),
            };
            let next = tokens.get(i + 1);
            let next_continues = match next {
                Some(nt) => match nt.tok {
                    Tok::Pipe | Tok::Kw(Kw::Else) | Tok::Kw(Kw::Catch) => true,
                    Tok::Dot => matches!(
                        tokens.get(i + 2),
                        Some(Token {
                            tok: Tok::Ident(_),
                            space_before: false,
                            ..
                        })
                    ),
                    _ => false,
                },
                None => false,
            };
            if !(prev_continues || next_continues) {
                out.push(t.clone());
            }
        } else {
            out.push(t.clone());
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(src: &str) -> Vec<Tok> {
        lex(0, src)
            .unwrap()
            .tokens
            .into_iter()
            .map(|t| t.tok)
            .collect()
    }

    fn command(src: &str) -> CommandTok {
        match toks(src).into_iter().next() {
            Some(Tok::Command(c)) => *c,
            other => panic!("expected a command, got {other:?}"),
        }
    }

    fn lit(p: &WordPart) -> &str {
        match p {
            WordPart::Lit { text, .. } => text,
            other => panic!("expected literal, got {other:?}"),
        }
    }

    fn err(src: &str) -> OkError {
        lex(0, src).err().expect("expected a lex error")
    }

    #[test]
    fn numbers_and_durations() {
        assert_eq!(
            toks("1_000 0x1F 0o17 0b101 1.5 2.5e-3")[..6],
            [
                Tok::Int(1000),
                Tok::Int(31),
                Tok::Int(15),
                Tok::Int(5),
                Tok::Float(1.5),
                Tok::Float(0.0025)
            ]
        );
        assert_eq!(
            toks("250ms 1h30m 1.5s 7d")[..4],
            [
                Tok::Duration(250_000_000),
                Tok::Duration(5_400_000_000_000),
                Tok::Duration(1_500_000_000),
                Tok::Duration(604_800_000_000_000)
            ]
        );
        assert!(err("1m1h").message.contains("largest to smallest"));
        assert!(err("2min").message.contains("invalid"));
        assert!(err("99999999999999999999").message.contains("too large"));
    }

    #[test]
    fn strings_escapes_and_interpolation() {
        let t = toks(r#""a\tb {x} \{c\}""#);
        let Tok::Str(parts) = &t[0] else { panic!() };
        assert_eq!(parts[0], StrPart::Lit("a\tb ".into()));
        assert!(matches!(&parts[1], StrPart::Interp { spec: None, .. }));
        assert_eq!(parts[2], StrPart::Lit(" {c}".into()));
        assert_eq!(
            toks(r"'C:\x\{'")[0],
            Tok::Str(vec![StrPart::Lit(r"C:\x\{".into())])
        );
        let t = toks(r#""{n:>6}""#);
        let Tok::Str(parts) = &t[0] else { panic!() };
        assert!(matches!(&parts[0], StrPart::Interp { spec: Some(s), .. } if s == ">6"));
        assert!(err(r#""\q""#).message.contains("unknown escape"));
    }

    #[test]
    fn triple_quoted_strings_are_dedented() {
        let src = "let m = \"\"\"\n  Hello,\n    world\n  \"\"\"\n";
        let t = toks(src);
        assert_eq!(
            t[3],
            Tok::Str(vec![StrPart::Lit("Hello,\n  world\n".into())])
        );
        let src = "'''\n    a\n    b'''";
        assert_eq!(toks(src)[0], Tok::Str(vec![StrPart::Lit("a\nb".into())]));
    }

    #[test]
    fn crlf_in_strings_becomes_lf() {
        assert_eq!(
            toks("\"a\r\nb\"")[0],
            Tok::Str(vec![StrPart::Lit("a\nb".into())])
        );
    }

    #[test]
    fn newline_rules() {
        let t = toks("a\n  | b\nc +\n d\n");
        let nl = t.iter().filter(|t| **t == Tok::Newline).count();
        assert_eq!(nl, 2, "{t:?}");
        let t = toks("f(\n1,\n2\n)");
        assert!(!t.contains(&Tok::Newline));
        let t = toks("if a {\n}\nelse {\n}");
        assert_eq!(t.iter().filter(|t| **t == Tok::Newline).count(), 0, "{t:?}");
    }

    #[test]
    fn shell_operators_get_hints() {
        assert!(err("a && b").help.unwrap().contains("and"));
        assert!(err("a || b").help.unwrap().contains("or"));
        assert!(err("!a").help.unwrap().contains("not"));
        assert!(err("a;").message.contains("semicolons"));
    }

    #[test]
    fn run_forms() {
        assert_eq!(toks("run(\"a\")")[0], Tok::RunFn { bang: false });
        assert_eq!(toks("run!(\"a\")")[0], Tok::RunFn { bang: true });
        assert_eq!(toks("x.run")[2], Tok::Ident("run".into()));
        assert_eq!(toks("--dry-run")[4], Tok::Ident("run".into()));
        assert!(matches!(&toks("run! make test")[0], Tok::Command(c) if c.bang));
    }

    #[test]
    fn command_words_and_quotes() {
        let c = command("run git commit -m \"fix {x}\" 'a b' --target={t} \"\"");
        assert_eq!(c.words.len(), 7);
        assert_eq!(lit(&c.words[0].parts[0]), "git");
        assert_eq!(lit(&c.words[3].parts[0]), "fix ");
        assert!(matches!(c.words[3].parts[1], WordPart::Interp { .. }));
        assert_eq!(lit(&c.words[4].parts[0]), "a b");
        assert_eq!(c.words[5].parts.len(), 2);
        assert_eq!(lit(&c.words[6].parts[0]), "");
    }

    #[test]
    fn command_bare_words_keep_specials() {
        let c = command(r"run tool C:\x\y.exe *.rs ~ $HOME [a] http://h/#top");
        let words: Vec<&str> = c.words.iter().map(|w| lit(&w.parts[0])).collect();
        assert_eq!(
            words,
            [
                "tool",
                r"C:\x\y.exe",
                "*.rs",
                "~",
                "$HOME",
                "[a]",
                "http://h/#top"
            ]
        );
    }

    #[test]
    fn command_terminators() {
        let t = toks("(run git rev-parse HEAD).out");
        assert!(matches!(t[1], Tok::Command(_)));
        assert_eq!(t[2], Tok::RParen);
        let t = toks("if x { run make }");
        assert!(matches!(t[3], Tok::Command(_)));
        assert_eq!(t[4], Tok::RBrace);
        let t = toks("run curl -s {url} | parse(\"json\")");
        assert!(matches!(t[0], Tok::Command(_)));
        assert_eq!(t[1], Tok::Pipe);
        let c = command("run make # build it");
        assert_eq!(c.words.len(), 1);
    }

    #[test]
    fn command_spread_redirect_and_with() {
        let c = command("run cargo build {...flags} > {out / \"x\"} with {cwd: \"sub\"}");
        assert!(matches!(c.words[2].parts[0], WordPart::Spread { .. }));
        assert_eq!(c.redirects.len(), 1);
        assert_eq!(c.redirects[0].kind, RedirectKind::Out);
        assert!(c.with.is_some());
        let c = command("run echo with care");
        assert_eq!(c.words.len(), 3);
        assert!(c.with.is_none());
        let c = command("run a >> log.txt < in.txt");
        assert_eq!(c.redirects[0].kind, RedirectKind::Append);
        assert_eq!(c.redirects[1].kind, RedirectKind::In);
    }

    #[test]
    fn command_line_continuation() {
        let c = command("run cargo build \\\n  --release");
        assert_eq!(c.words.len(), 3);
    }

    #[test]
    fn command_shell_habits_are_errors() {
        assert!(err("run make && make install").message.contains("&&"));
        assert!(err("run a; b").message.contains(";"));
        assert!(err("run FOO=1 make").help.unwrap().contains("env"));
        assert!(err("run echo (hi)").message.contains("("));
        assert!(err("run echo $(date)").message.contains("$("));
        assert!(err("run x pre{...a}").message.contains("whole word"));
        assert!(err("run \n").message.contains("needs a command"));
        assert_eq!(toks("run"), vec![Tok::Ident("run".into()), Tok::Eof]);
    }

    #[test]
    fn command_first_word_equals_only_checked_on_first_word() {
        let c = command("run make CC=gcc");
        assert_eq!(c.words.len(), 2);
    }
}
