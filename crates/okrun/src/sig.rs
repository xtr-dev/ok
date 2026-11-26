//! Command-line arguments from a signature (SPEC.md section 14).
//!
//! The parameters of a script's `main` (and, in project mode, of a task)
//! define its command line: positional parameters become arguments, `--`
//! parameters become flags, type annotations decide how text is parsed, and
//! the doc comment plus trailing parameter comments become `--help`.
//! Parsing happens before any code runs, so bad arguments fail fast with
//! exit code 2.

use std::fmt::Write as _;
use std::path::Path;

use crate::ast::{Expr, ExprKind, Param, StrPiece, TypeAnn};
use crate::error::{ErrKind, OkError, suggest};
use crate::path::OkPath;
use crate::source::SourceMap;
use crate::value::{Record, Value};

/// ok's global options, for the "put them before the name" hint.
const GLOBAL_OPTIONS: &[&str] = &[
    "--trace",
    "--dry-run",
    "--yes",
    "--trust",
    "--force",
    "--env",
    "--jobs",
    "--color",
    "-C",
    "-e",
];

/// Parsed arguments, ready to pass to the function.
pub struct Parsed {
    pub pos: Vec<Value>,
    pub named: Vec<(String, Value)>,
}

pub enum Outcome {
    Run(Parsed),
    /// `--help` was given; the text to print.
    Help(String),
}

/// What a signature belongs to, for usage lines and messages.
pub struct Target<'a> {
    /// `ok csv-to-json.ok` or `ok db reset`.
    pub usage: String,
    pub doc: Option<&'a str>,
    pub params: &'a [Param],
}

fn type_label(ty: Option<&TypeAnn>) -> String {
    match ty {
        None | Some(TypeAnn::Any) | Some(TypeAnn::String) => "value".into(),
        Some(TypeAnn::Optional(t)) => type_label(Some(t)),
        Some(TypeAnn::List(Some(t))) => type_label(Some(t)),
        Some(TypeAnn::List(None)) => "value".into(),
        Some(TypeAnn::Choice(cs)) => cs.join("|"),
        Some(t) => t.describe(),
    }
}

fn is_bool(p: &Param) -> bool {
    matches!(p.ty, Some(TypeAnn::Bool))
}

fn is_list(p: &Param) -> bool {
    matches!(p.ty, Some(TypeAnn::List(_)))
}

fn required(p: &Param) -> bool {
    p.default.is_none()
        && !p.rest
        && !is_bool(p)
        && !matches!(p.ty, Some(TypeAnn::Optional(_)))
        && !is_list(p)
}

/// The usage line: `ok tool.ok <input> [--out <path>] [--verbose]`.
pub fn usage(t: &Target) -> String {
    let mut parts = vec![t.usage.clone()];
    for p in t.params.iter().filter(|p| !p.flag) {
        parts.push(if p.rest {
            format!("[{}...]", p.cli_name)
        } else if required(p) {
            format!("<{}>", p.cli_name)
        } else {
            format!("[{}]", p.cli_name)
        });
    }
    for p in t.params.iter().filter(|p| p.flag) {
        let body = if is_bool(p) {
            let default_true = p
                .default
                .as_ref()
                .is_some_and(|d| matches!(d.kind, ExprKind::Bool(true)));
            if default_true {
                format!("--no-{}", p.cli_name)
            } else {
                format!("--{}", p.cli_name)
            }
        } else {
            format!("--{} <{}>", p.cli_name, type_label(p.ty.as_ref()))
        };
        parts.push(if required(p) {
            body
        } else {
            format!("[{body}]")
        });
    }
    parts.join(" ")
}

/// How a default is shown in `--help`: literals by value, anything else by
/// its source text, because printing help never runs code.
fn default_text(e: &Expr, sources: &SourceMap) -> String {
    match &e.kind {
        ExprKind::Str(pieces) if pieces.iter().all(|p| matches!(p, StrPiece::Lit(_))) => pieces
            .iter()
            .map(|p| {
                if let StrPiece::Lit(s) = p {
                    s.as_str()
                } else {
                    ""
                }
            })
            .collect(),
        ExprKind::Int(i) => i.to_string(),
        ExprKind::Float(f) => crate::value::format_float(*f),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Null => "null".into(),
        ExprKind::Duration(d) => crate::value::format_duration(*d),
        _ => {
            let f = sources.file(e.span.file);
            f.text
                .get(e.span.start as usize..e.span.end as usize)
                .unwrap_or("")
                .to_string()
        }
    }
}

/// The text of `--help` (SPEC.md 14.2).
pub fn help(t: &Target, sources: &SourceMap) -> String {
    let mut out = String::new();
    if let Some(doc) = t.doc {
        let _ = writeln!(out, "{}\n", doc.trim_end());
    }
    let _ = writeln!(out, "Usage: {}", usage(t));
    let mut args: Vec<(String, String)> = Vec::new();
    let mut opts: Vec<(String, String)> = Vec::new();
    let any_short = t.params.iter().any(|p| p.short.is_some());
    for p in t.params {
        let mut desc = p.help.clone().unwrap_or_default();
        if let Some(d) = &p.default {
            let shown = default_text(d, sources);
            if !(is_bool(p) && shown == "false") {
                if !desc.is_empty() {
                    desc.push(' ');
                }
                let _ = write!(desc, "[default: {shown}]");
            }
        }
        if p.flag {
            let left = if is_bool(p) {
                let default_true = p
                    .default
                    .as_ref()
                    .is_some_and(|d| matches!(d.kind, ExprKind::Bool(true)));
                // `--[no-]name`: the help text describes the setting, so it
                // must sit next to the positive name; `--no-name` alone read
                // as if `--no-compress` would compress (SPEC.md 14.2).
                if default_true {
                    format!("--[no-]{}", p.cli_name)
                } else {
                    format!("--{}", p.cli_name)
                }
            } else {
                format!("--{} <{}>", p.cli_name, type_label(p.ty.as_ref()))
            };
            if is_list(p) {
                if !desc.is_empty() {
                    desc.push(' ');
                }
                desc.push_str("(repeatable)");
            }
            // With any short form present, long names line up after a
            // `-x, ` column (SPEC.md 14.2).
            let left = match p.short {
                Some(c) => format!("-{c}, {left}"),
                None if any_short => format!("    {left}"),
                None => left,
            };
            opts.push((left, desc));
        } else {
            let left = if p.rest {
                format!("[{}...]", p.cli_name)
            } else if required(p) {
                format!("<{}>", p.cli_name)
            } else {
                format!("[{}]", p.cli_name)
            };
            if let Some(cs) = p.ty.as_ref().and_then(TypeAnn::choices) {
                if !desc.is_empty() {
                    desc.push(' ');
                }
                let _ = write!(desc, "[choices: {}]", cs.join(", "));
            }
            args.push((left, desc));
        }
    }
    opts.push(("-h, --help".into(), "print this help".into()));
    let width = args
        .iter()
        .chain(opts.iter())
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0)
        + 2;
    let section = |out: &mut String, title: &str, rows: &[(String, String)]| {
        if rows.is_empty() {
            return;
        }
        let _ = writeln!(out, "\n{title}:");
        for (l, d) in rows {
            if d.is_empty() {
                let _ = writeln!(out, "  {l}");
            } else {
                let _ = writeln!(out, "  {l:<width$}{d}");
            }
        }
    };
    section(&mut out, "Arguments", &args);
    section(&mut out, "Options", &opts);
    // Doc comments and defaults come from the file, which may be a project
    // nobody has reviewed yet (`--help` needs no trust).
    crate::error::terminal_safe(&out).into_owned()
}

/// An argument error: message, optional help, and the usage line, exit 2.
fn arg_error(t: &Target, msg: String, help: Option<String>) -> OkError {
    let mut e = OkError::new(ErrKind::Arg, msg);
    e.help = help;
    e.exit_code = Some(2);
    e.trailer = Some(format!("usage: {}\nrun with --help for details", usage(t)));
    e
}

fn parse_value(
    t: &Target,
    p: &Param,
    ty: Option<&TypeAnn>,
    text: &str,
    base: &Path,
    anchor: bool,
) -> Result<Value, OkError> {
    let shown = if p.flag {
        format!("--{}", p.cli_name)
    } else {
        format!("<{}>", p.cli_name)
    };
    let bad = |what: &str| arg_error(t, format!("{shown} expects {what}, got {text:?}"), None);
    Ok(match ty {
        None | Some(TypeAnn::Any) | Some(TypeAnn::String) => Value::str(text),
        Some(TypeAnn::Optional(inner)) | Some(TypeAnn::List(Some(inner))) => {
            return parse_value(t, p, Some(inner), text, base, anchor);
        }
        Some(TypeAnn::List(None)) => Value::str(text),
        Some(TypeAnn::Int) => Value::Int(text.trim().parse().map_err(|_| bad("an integer"))?),
        Some(TypeAnn::Float) => match text.trim().parse::<f64>() {
            Ok(f) if f.is_finite() => Value::Float(f),
            _ => return Err(bad("a number")),
        },
        Some(TypeAnn::Path) => {
            let p = OkPath::parse(text);
            // Relative paths mean "relative to where the user typed it".
            if !p.is_abs() && anchor {
                Value::path(OkPath::from_native(base).join(&p))
            } else {
                Value::path(p)
            }
        }
        Some(TypeAnn::Duration) => Value::Duration(
            crate::system::parse_duration(text)
                .ok_or_else(|| bad("a duration like 30s or 1h30m"))?,
        ),
        Some(TypeAnn::Bool) => match text {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(bad("true or false")),
        },
        Some(TypeAnn::Choice(cs)) => {
            if !cs.iter().any(|c| c == text) {
                let listed: Vec<String> = cs.iter().map(|c| format!("{c:?}")).collect();
                // An abbreviation (`prod`) is too far from the full word for
                // edit distance, so a unique prefix match is tried first.
                let mut prefixed = cs
                    .iter()
                    .filter(|c| !text.is_empty() && c.starts_with(text));
                let by_prefix = match (prefixed.next(), prefixed.next()) {
                    (Some(c), None) => Some(c.clone()),
                    _ => None,
                };
                let help = by_prefix
                    .or_else(|| suggest(text, cs.iter().map(String::as_str)))
                    .map(|s| format!("did you mean {s:?}?"));
                return Err(arg_error(
                    t,
                    format!("{shown} expects one of {}, got {text:?}", listed.join(", ")),
                    help,
                ));
            }
            Value::str(text)
        }
        Some(other) => {
            return Err(arg_error(
                t,
                format!(
                    "{shown} has type {}, which cannot come from the command line",
                    other.describe()
                ),
                None,
            ));
        }
    })
}

/// Parse `argv` against the signature. `base` is the directory relative
/// path arguments are resolved against (`invocation_dir`).
/// When `anchor` is set, relative path arguments are joined to `base`.
pub fn parse(t: &Target, argv: &[String], base: &Path, anchor: bool) -> Result<Outcome, OkError> {
    let positional: Vec<&Param> = t.params.iter().filter(|p| !p.flag && !p.rest).collect();
    let rest = t.params.iter().find(|p| p.rest);
    let flags: Vec<&Param> = t.params.iter().filter(|p| p.flag).collect();
    let mut pos_values: Vec<Value> = Vec::new();
    let mut rest_values: Vec<Value> = Vec::new();
    let mut named: Vec<(String, Value)> = Vec::new();
    let mut lists: Record = Record::new();
    let mut only_positional = false;
    let mut i = 0;
    while i < argv.len() {
        let a = &argv[i];
        i += 1;
        if !only_positional && a == "--" {
            only_positional = true;
            continue;
        }
        if !only_positional && (a == "-h" || a == "--help") {
            return Ok(Outcome::Help(String::new()));
        }
        if !only_positional && a.starts_with("--") && a.len() > 2 {
            let (name, inline) = match a[2..].split_once('=') {
                Some((n, v)) => (n.to_string(), Some(v.to_string())),
                None => (a[2..].to_string(), None),
            };
            let negated = name
                .strip_prefix("no-")
                .and_then(|n| flags.iter().find(|p| p.cli_name == n && is_bool(p)));
            let Some(p) = flags.iter().find(|p| p.cli_name == name).or(negated) else {
                let shown = format!("--{name}");
                let help = if GLOBAL_OPTIONS.contains(&shown.as_str()) {
                    Some(format!(
                        "ok's own options go before the name: ok {shown} {}",
                        t.usage.trim_start_matches("ok ")
                    ))
                } else {
                    let names: Vec<String> =
                        flags.iter().map(|p| format!("--{}", p.cli_name)).collect();
                    suggest(&shown, names.iter().map(String::as_str))
                        .map(|s| format!("did you mean {s}?"))
                };
                return Err(arg_error(t, format!("unknown flag {shown}"), help));
            };
            if is_bool(p) {
                let value = match &inline {
                    None => negated.is_none(),
                    Some(v) => match v.as_str() {
                        "true" => true,
                        "false" => false,
                        _ => {
                            return Err(arg_error(
                                t,
                                format!("--{} is a switch; use it without a value", p.cli_name),
                                None,
                            ));
                        }
                    },
                };
                named.retain(|(n, _)| n != &p.name);
                named.push((p.name.clone(), Value::Bool(value)));
                continue;
            }
            let text = match inline {
                Some(v) => v,
                None => match argv.get(i) {
                    Some(v) => {
                        i += 1;
                        v.clone()
                    }
                    None => {
                        return Err(arg_error(
                            t,
                            format!("--{} needs a value", p.cli_name),
                            None,
                        ));
                    }
                },
            };
            let v = parse_value(t, p, p.ty.as_ref(), &text, base, anchor)?;
            store(p, v, &mut named, &mut lists);
            continue;
        }
        if !only_positional && is_short_arg(a) {
            let letters: Vec<char> = a[1..].chars().collect();
            if letters.contains(&'h') && letters.iter().all(char::is_ascii_alphabetic) {
                return Ok(Outcome::Help(String::new()));
            }
            let lookup = |c: char| flags.iter().copied().find(|p| p.short == Some(c));
            let first = lookup(letters[0]);
            // `-o VALUE`: a flag that takes a value stands alone.
            if let Some(p) = first.filter(|p| !is_bool(p)) {
                if letters.len() > 1 {
                    return Err(arg_error(
                        t,
                        format!("-{} takes a value, given as the next argument", letters[0]),
                        Some(format!(
                            "write -{} VALUE (or --{} VALUE)",
                            letters[0], p.cli_name
                        )),
                    ));
                }
                let Some(text) = argv.get(i) else {
                    return Err(arg_error(
                        t,
                        format!("-{} (--{}) needs a value", letters[0], p.cli_name),
                        None,
                    ));
                };
                i += 1;
                let v = parse_value(t, p, p.ty.as_ref(), text, base, anchor)?;
                store(p, v, &mut named, &mut lists);
                continue;
            }
            // `-v` or a group of switches `-vq`.
            for &c in &letters {
                let Some(p) = (if c.is_ascii_alphabetic() {
                    lookup(c)
                } else {
                    None
                }) else {
                    let free_slot = rest.is_some() || pos_values.len() < positional.len();
                    return Err(unknown_short(t, &flags, a, c, free_slot));
                };
                if !is_bool(p) {
                    return Err(arg_error(
                        t,
                        format!(
                            "-{c} takes a value, so it cannot be grouped with other flags in {a}"
                        ),
                        Some(format!("write it on its own: -{c} VALUE")),
                    ));
                }
                store(p, Value::Bool(true), &mut named, &mut lists);
            }
            continue;
        }
        // A positional argument.
        if pos_values.len() < positional.len() {
            let p = positional[pos_values.len()];
            pos_values.push(parse_value(t, p, p.ty.as_ref(), a, base, anchor)?);
        } else if let Some(r) = rest {
            rest_values.push(parse_value(t, r, r.ty.as_ref(), a, base, anchor)?);
        } else {
            return Err(arg_error(t, format!("unexpected argument {a:?}"), None));
        }
    }
    for p in positional.iter().skip(pos_values.len()) {
        if required(p) {
            return Err(arg_error(
                t,
                format!("missing argument <{}>", p.cli_name),
                None,
            ));
        }
    }
    for p in &flags {
        let given = named.iter().any(|(n, _)| n == &p.name) || lists.contains(&p.name);
        if required(p) && !given {
            return Err(arg_error(
                t,
                format!(
                    "missing option --{} <{}>",
                    p.cli_name,
                    type_label(p.ty.as_ref())
                ),
                None,
            ));
        }
    }
    for (k, v) in lists.iter() {
        named.push((k.to_string(), v.clone()));
    }
    let mut pos = pos_values;
    pos.extend(rest_values);
    Ok(Outcome::Run(Parsed { pos, named }))
}

/// Whether a command-line word is a short flag or a group of switches:
/// `-` followed by an ASCII letter. `-`, `-5` and `-1.5` are values.
fn is_short_arg(a: &str) -> bool {
    let b = a.as_bytes();
    b.len() >= 2 && b[0] == b'-' && b[1].is_ascii_alphabetic()
}

/// Record a flag's value: list flags collect, others keep the last value.
fn store(p: &Param, v: Value, named: &mut Vec<(String, Value)>, lists: &mut Record) {
    if is_list(p) {
        let mut items = match lists.get(&p.name) {
            Some(Value::List(l)) => l.to_vec(),
            _ => Vec::new(),
        };
        items.push(v);
        lists.insert(p.name.as_str(), Value::list(items));
    } else {
        named.retain(|(n, _)| n != &p.name);
        named.push((p.name.clone(), v));
    }
}

/// The error for a letter in `word` that is no declared short flag.
fn unknown_short(t: &Target, flags: &[&Param], word: &str, c: char, takes_values: bool) -> OkError {
    let shown = format!("-{c}");
    let msg = if word.len() > 2 {
        format!("unknown flag {shown} in {word}")
    } else {
        format!("unknown flag {shown}")
    };
    let mut by_letter = flags.iter().filter(|p| p.cli_name.starts_with(c));
    let help = if GLOBAL_OPTIONS.contains(&shown.as_str()) {
        Some(format!(
            "ok's own options go before the name: ok {shown} ... {}",
            t.usage.trim_start_matches("ok ")
        ))
    } else if let (Some(p), None) = (by_letter.next(), by_letter.next()) {
        Some(format!("did you mean --{}?", p.cli_name))
    } else if takes_values {
        Some(format!("to pass {word:?} as a value, put it after --"))
    } else {
        None
    };
    arg_error(t, msg, help)
}

/// Completion candidates for the arguments after a task name (SPEC.md
/// 14.5). `done` are the complete words after the task name and `current`
/// is the word being typed. The caller filters by prefix.
pub fn complete_args(params: &[Param], done: &[String], current: &str) -> Vec<String> {
    let flags: Vec<&Param> = params.iter().filter(|p| p.flag).collect();
    let find_flag = |word: &str| -> Option<&Param> {
        if let Some(name) = word.strip_prefix("--") {
            return flags.iter().copied().find(|p| p.cli_name == name);
        }
        let mut letters = word.strip_prefix('-')?.chars();
        match (letters.next(), letters.next()) {
            (Some(c), None) => flags.iter().copied().find(|p| p.short == Some(c)),
            _ => None,
        }
    };
    let choices_of = |p: &Param| -> Vec<String> {
        p.ty.as_ref()
            .and_then(TypeAnn::choices)
            .map(|cs| cs.to_vec())
            .unwrap_or_default()
    };
    // `--role=<TAB>`
    if let Some((name, _)) = current.split_once('=')
        && name.starts_with("--")
        && let Some(p) = find_flag(name)
    {
        return choices_of(p)
            .into_iter()
            .map(|c| format!("{name}={c}"))
            .collect();
    }
    // Walk the finished words to learn whether a flag is waiting for its
    // value and how many positional arguments are already given.
    let mut awaiting: Option<&Param> = None;
    let mut positional = 0;
    let mut only_positional = false;
    for w in done {
        if awaiting.take().is_some() {
            continue;
        }
        if only_positional {
            positional += 1;
        } else if w == "--" {
            only_positional = true;
        } else if w.starts_with("--") || is_short_arg(w) {
            if !w.contains('=')
                && let Some(p) = find_flag(w)
                && !is_bool(p)
            {
                awaiting = Some(p);
            }
        } else {
            positional += 1;
        }
    }
    if let Some(p) = awaiting {
        return choices_of(p);
    }
    let mut cands = Vec::new();
    if !only_positional {
        cands.extend(flags.iter().map(|p| format!("--{}", p.cli_name)));
        cands.push("--help".into());
        // Short forms only once the user typed a single dash, so that the
        // usual list shows each flag once, by its readable name.
        if current.starts_with('-') && !current.starts_with("--") {
            cands.extend(
                flags
                    .iter()
                    .filter_map(|p| p.short.map(|c| format!("-{c}"))),
            );
            cands.push("-h".into());
        }
    }
    if !current.starts_with('-') || only_positional {
        let mut pos = params.iter().filter(|p| !p.flag && !p.rest);
        let next = pos
            .nth(positional)
            .or_else(|| params.iter().find(|p| p.rest));
        if let Some(p) = next {
            cands.extend(choices_of(p));
        }
    }
    cands
}
