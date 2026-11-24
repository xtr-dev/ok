//! `ok self fmt`: the canonical layout (SPEC.md 17).
//!
//! The formatter changes whitespace only, so it can never change what a
//! program means:
//!
//! * each line is indented two spaces per open bracket, plus one level for a
//!   line that continues the previous one (a leading `|` or `.`, or a
//!   previous line ending in an operator);
//! * trailing whitespace is removed, runs of blank lines collapse to one,
//!   blank lines at the start of a block or the end of the file go away, and
//!   the file ends with one newline;
//! * lines inside multi-line strings and continued commands are kept as
//!   they are.

use crate::error::OkError;
use crate::lexer::{self, Kw, Tok, Token};

const INDENT: &str = "  ";

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

/// Format source text. Fails only if the text does not lex.
pub fn format_source(src: &str) -> Result<String, OkError> {
    let src_norm = src.replace("\r\n", "\n");
    let src = src_norm.strip_prefix('\u{feff}').unwrap_or(&src_norm);
    let tokens = lex_flat(src)?;
    let lines: Vec<&str> = src.split('\n').collect();
    let mut starts = Vec::with_capacity(lines.len());
    let mut off = 0;
    for l in &lines {
        starts.push(off);
        off += l.len() + 1;
    }
    // For each line: is it inside a multi-line token (keep verbatim)?
    let mut verbatim = vec![false; lines.len()];
    for t in &tokens {
        let (s, e) = (t.span.start as usize, t.span.end as usize);
        let first = line_of(&starts, s);
        let last = line_of(&starts, e.saturating_sub(1).max(s));
        for v in verbatim.iter_mut().take(last + 1).skip(first + 1) {
            *v = true;
        }
    }
    let mut out = String::new();
    let mut depth: i64 = 0;
    let mut prev_continues = false;
    let mut blank_run = 0;
    let mut ti = 0;
    for (li, line) in lines.iter().enumerate() {
        let line_start = starts[li];
        let line_end = line_start + line.len();
        let toks: Vec<&Token> = {
            let mut v = Vec::new();
            while ti < tokens.len() && (tokens[ti].span.start as usize) < line_end + 1 {
                if (tokens[ti].span.start as usize) >= line_start {
                    v.push(&tokens[ti]);
                }
                ti += 1;
            }
            v
        };
        if verbatim[li] {
            out.push_str(line.trim_end_matches([' ', '\t']));
            out.push('\n');
            // Tokens starting on this line still change the depth.
            for t in &toks {
                depth += bracket_delta(&t.tok);
            }
            if let Some(t) = toks.last() {
                prev_continues = continues(&t.tok);
            }
            blank_run = 0;
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            continue;
        }
        if blank_run > 0
            && !out.is_empty()
            && !out.ends_with("{\n")
            && !out.ends_with("(\n")
            && !out.ends_with("[\n")
        {
            out.push('\n');
        }
        blank_run = 0;
        let first = toks.first().map(|t| &t.tok);
        let closes_first = matches!(first, Some(Tok::RBrace | Tok::RParen | Tok::RBracket));
        let leading_cont = matches!(first, Some(Tok::Pipe))
            || (matches!(first, Some(Tok::Dot)) && trimmed.starts_with('.'));
        let mut level = depth - i64::from(closes_first);
        if leading_cont || (prev_continues && !closes_first) {
            level += 1;
        }
        let is_shebang = li == 0 && trimmed.starts_with("#!");
        if !is_shebang {
            for _ in 0..level.max(0) {
                out.push_str(INDENT);
            }
        }
        out.push_str(trimmed);
        out.push('\n');
        let before = depth;
        for t in &toks {
            depth += bracket_delta(&t.tok);
        }
        // A trailing operator continues the statement unless the line
        // opened a bracket (the bracket's contents carry the indentation).
        prev_continues = toks.last().is_some_and(|t| continues(&t.tok)) && depth <= before;
    }
    let trimmed = out.trim_end_matches('\n');
    Ok(format!("{trimmed}\n"))
}

fn bracket_delta(t: &Tok) -> i64 {
    match t {
        Tok::LParen | Tok::LBracket | Tok::LBrace => 1,
        Tok::RParen | Tok::RBracket | Tok::RBrace => -1,
        _ => 0,
    }
}

fn line_of(starts: &[usize], offset: usize) -> usize {
    match starts.binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

/// Tokens without newline handling, so every bracket is seen.
fn lex_flat(src: &str) -> Result<Vec<Token>, OkError> {
    let lexed = lexer::lex(0, src)?;
    Ok(lexed
        .tokens
        .into_iter()
        .filter(|t| !matches!(t.tok, Tok::Newline | Tok::Eof))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reindents_blocks_and_pipelines() {
        let src = "fn f(x) {\nif x {\n        print(1)\n}\nlet y = [1, 2]\n| map(v => v)\n\n\n\nprint(y)   \n}\n\n";
        let want = "fn f(x) {\n  if x {\n    print(1)\n  }\n  let y = [1, 2]\n    | map(v => v)\n\n  print(y)\n}\n";
        assert_eq!(format_source(src).unwrap(), want);
    }

    #[test]
    fn keeps_multiline_strings_verbatim() {
        let src = "let s = \"\"\"\n      keep   this\n  \"\"\"\nprint(s)\n";
        assert_eq!(format_source(src).unwrap(), src);
    }

    #[test]
    fn is_idempotent_on_its_output() {
        let src = "task build(\n--release: bool,\n) needs [gen] {\nrun cargo build\n}\n";
        let once = format_source(src).unwrap();
        assert_eq!(format_source(&once).unwrap(), once);
    }
}
