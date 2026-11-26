//! Environment, time, terminal input and hashing (SPEC.md 12.3, 12.7,
//! 12.8, 12.9).

use std::io::{IsTerminal, Read, Write};

use crate::builtins::{arg_err, eager, type_err};
use crate::error::{ErrKind, OkError, suggest};
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::list::{count_args, named};
use crate::path::OkPath;
use crate::source::Span;
use crate::value::{Record, Value};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("which", which),
    eager("now", now),
    eager("time", time),
    eager("format", format_time),
    eager("sleep", sleep),
    eager("duration", duration),
    eager("prompt", prompt),
    eager("confirm", confirm),
    eager("hash", hash),
];

// ----- env ----------------------------------------------------------------------

/// Refuse a name that cannot be an environment variable (SPEC.md 12.3).
fn valid_name(args: &Args, name: &str) -> R<()> {
    crate::command::check_env_name(name).map_err(|msg| {
        OkError::at(ErrKind::Value, args.span, msg)
            .with_help(crate::command::ENV_NAME_HELP)
            .into()
    })
}

/// `env.get`, `env.has`, `env.set`, `env.remove`, `env.all`.
pub fn env_method(interp: &mut Interp, name: &str, span: Span, mut args: Args) -> R<Value> {
    named(&mut args, &format!("env.{name}"), &[])?;
    let key = |args: &Args, fname: &str| -> R<String> {
        match args.pos.first() {
            Some(Value::Str(s)) => Ok(s.to_string()),
            Some(other) => type_err(
                args,
                format!(
                    "`env.{fname}` needs a variable name, got {}",
                    other.a_type()
                ),
            ),
            None => arg_err(args, format!("`env.{fname}` needs a variable name")),
        }
    };
    match name {
        "get" => {
            count_args(&args, "env.get", 1, 2)?;
            let k = key(&args, "get")?;
            Ok(interp
                .env_var(&k)
                .map(Value::str)
                .unwrap_or_else(|| args.pos.get(1).cloned().unwrap_or(Value::Null)))
        }
        "has" => {
            count_args(&args, "env.has", 1, 1)?;
            let k = key(&args, "has")?;
            Ok(Value::Bool(interp.env_var(&k).is_some()))
        }
        "set" => {
            count_args(&args, "env.set", 2, 2)?;
            let k = key(&args, "set")?;
            valid_name(&args, &k)?;
            let v = match &args.pos[1] {
                Value::Str(s) => s.to_string(),
                Value::Int(_) | Value::Float(_) | Value::Bool(_) => args.pos[1].display(),
                Value::Path(p) => p.native().to_string_lossy().into_owned(),
                other => {
                    return type_err(
                        &args,
                        format!("environment values are strings, got {}", other.a_type()),
                    );
                }
            };
            if v.contains('\0') {
                return Err(OkError::at(
                    ErrKind::Value,
                    args.span,
                    format!("the value of {k} contains a NUL character, which cannot be passed to a program"),
                )
                .into());
            }
            interp.set_env(&k, Some(v));
            Ok(Value::Null)
        }
        "remove" => {
            count_args(&args, "env.remove", 1, 1)?;
            let k = key(&args, "remove")?;
            valid_name(&args, &k)?;
            interp.set_env(&k, None);
            Ok(Value::Null)
        }
        "all" => {
            count_args(&args, "env.all", 0, 0)?;
            let mut names: Vec<String> = std::env::vars_os()
                .filter_map(|(k, _)| k.into_string().ok())
                .collect();
            for (k, _) in &interp.env_overlay {
                if !names.contains(k) {
                    names.push(k.clone());
                }
            }
            names.sort();
            let mut r = Record::new();
            for n in names {
                if let Some(v) = interp.env_var(&n) {
                    r.insert(n.as_str(), Value::str(v));
                }
            }
            Ok(Value::record(r))
        }
        other => {
            let help = suggest(other, ["get", "has", "set", "remove", "all"])
                .map(|s| format!("did you mean env.{s}?"));
            Err(OkError::at(
                ErrKind::Name,
                span,
                format!("`env` has no method `{other}`"),
            )
            .maybe_help(help.or_else(|| {
                Some(format!(
                    "to read a variable named {other}, write env.{other} without parentheses"
                ))
            }))
            .into())
        }
    }
}

fn which(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "which", &[])?;
    count_args(&args, "which", 1, 1)?;
    let Value::Str(name) = &args.pos[0] else {
        return type_err(
            &args,
            format!("`which` needs a command name, got {}", args.pos[0].a_type()),
        );
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let path = interp.env_var("PATH").map(std::ffi::OsString::from);
    Ok(
        match crate::command::resolve_program(name, &cwd, &cwd, path.as_deref()) {
            Ok(p) => Value::path(OkPath::from_native(&p)),
            Err(_) => Value::Null,
        },
    )
}

// ----- time ----------------------------------------------------------------------

fn zoned(ns: i64, utc: bool) -> Option<jiff::ZonedDateTime> {
    let ts = jiff::Timestamp::from_nanosecond(ns as i128).ok()?;
    let tz = if utc {
        jiff::tz::TimeZone::UTC
    } else {
        jiff::tz::TimeZone::system()
    };
    Some(ts.to_zoned_datetime(tz))
}

/// Display form of a time: RFC 3339 in local time (SPEC.md 5.1).
pub fn format_time_local(ns: i64) -> String {
    match zoned(ns, false) {
        Some(z) => z.strftime("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        None => crate::value::format_time_utc(ns),
    }
}

/// Properties of a time value (SPEC.md 5.9), in local time.
pub fn time_field(ns: i64, name: &str) -> Option<Value> {
    if name == "unix" {
        return Some(Value::Int(ns.div_euclid(1_000_000_000)));
    }
    let z = zoned(ns, false)?;
    let v = match name {
        "year" => z.year() as i64,
        "month" => z.month() as i64,
        "day" => z.day() as i64,
        "hour" => z.hour() as i64,
        "minute" => z.minute() as i64,
        "second" => z.second() as i64,
        _ => return None,
    };
    Some(Value::Int(v))
}

fn now(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "now", &[])?;
    count_args(&args, "now", 0, 0)?;
    let ns = jiff::Timestamp::now().as_nanosecond();
    Ok(Value::Time(
        ns.clamp(i64::MIN as i128, i64::MAX as i128) as i64
    ))
}

fn time(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "time", &[])?;
    count_args(&args, "time", 1, 1)?;
    let Value::Str(s) = &args.pos[0] else {
        return type_err(
            &args,
            format!(
                "`time` needs a string like \"2026-11-24\", got {}",
                args.pos[0].a_type()
            ),
        );
    };
    if let Ok(ts) = s.parse::<jiff::Timestamp>() {
        return Ok(Value::Time(ts.as_nanosecond() as i64));
    }
    if let Ok(d) = s.parse::<jiff::civil::Date>()
        && let Ok(z) = d.to_zoned_datetime(jiff::tz::TimeZone::system())
    {
        return Ok(Value::Time(z.timestamp().as_nanosecond() as i64));
    }
    Err(OkError::at(
        ErrKind::Value,
        args.span,
        format!("cannot read {} as a time", Value::Str(s.clone()).repr()),
    )
    .with_help("use RFC 3339 like \"2026-11-24T10:00:00Z\" or a date like \"2026-11-24\"")
    .into())
}

fn format_time(_: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "format", &["utc"])?;
    count_args(&args, "format", 2, 2)?;
    let utc = crate::list::bool_named(&opts, "utc", &args)?;
    let Value::Time(t) = &args.pos[0] else {
        return type_err(
            &args,
            format!(
                "`format` formats a time, got {}; for numbers use fmt(x, spec)",
                args.pos[0].a_type()
            ),
        );
    };
    let Value::Str(pattern) = &args.pos[1] else {
        return type_err(
            &args,
            format!(
                "the pattern must be a string like \"%Y-%m-%d\", got {}",
                args.pos[1].a_type()
            ),
        );
    };
    let Some(z) = zoned(*t, utc) else {
        return arg_err(&args, "the time is out of range");
    };
    let mut out = String::new();
    let fmt = jiff::fmt::strtime::format(pattern.as_bytes(), &z).map_err(|e| {
        OkError::at(
            ErrKind::Format,
            args.span,
            format!("invalid time pattern: {e}"),
        )
    })?;
    out.push_str(&fmt);
    Ok(Value::str(out))
}

fn sleep(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "sleep", &[])?;
    count_args(&args, "sleep", 1, 1)?;
    match &args.pos[0] {
        Value::Duration(d) if *d >= 0 => {
            std::thread::sleep(std::time::Duration::from_nanos(*d as u64));
            Ok(Value::Null)
        }
        Value::Duration(_) => arg_err(&args, "cannot sleep for a negative duration"),
        Value::Int(_) | Value::Float(_) => type_err(
            &args,
            "`sleep` needs a duration like 2s or 500ms, not a plain number",
        ),
        other => type_err(
            &args,
            format!("`sleep` needs a duration, got {}", other.a_type()),
        ),
    }
}

/// Parse duration literal syntax: `1h30m`, `250ms`, `-2s`.
pub fn parse_duration(s: &str) -> Option<i64> {
    let t = s.trim();
    let (neg, body) = match t.strip_prefix('-') {
        Some(b) => (true, b),
        None => (false, t),
    };
    let lexed = crate::lexer::lex(0, body).ok()?;
    match lexed.tokens.as_slice() {
        [
            crate::lexer::Token {
                tok: crate::lexer::Tok::Duration(d),
                ..
            },
            crate::lexer::Token {
                tok: crate::lexer::Tok::Eof,
                ..
            },
        ] => Some(if neg { -d } else { *d }),
        _ => None,
    }
}

fn duration(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "duration", &[])?;
    count_args(&args, "duration", 1, 1)?;
    match &args.pos[0] {
        Value::Duration(d) => Ok(Value::Duration(*d)),
        Value::Str(s) => parse_duration(s).map(Value::Duration).ok_or_else(|| {
            OkError::at(
                ErrKind::Value,
                args.span,
                format!("cannot read {} as a duration", Value::Str(s.clone()).repr()),
            )
            .with_help("durations look like 90s, 1h30m or 250ms")
            .into()
        }),
        other => type_err(
            &args,
            format!(
                "`duration` needs a string like \"1h30m\", got {}",
                other.a_type()
            ),
        ),
    }
}

// ----- terminal input ------------------------------------------------------------

fn read_answer(msg: &str) -> std::io::Result<String> {
    let mut err = std::io::stderr();
    write!(err, "{msg}")?;
    err.flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\n', '\r']).to_string())
}

fn prompt(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "prompt", &["default"])?;
    count_args(&args, "prompt", 1, 1)?;
    let msg = args.pos[0].display();
    let default = match opts.get("default") {
        None | Some(Value::Null) => None,
        Some(Value::Str(s)) => Some(s.to_string()),
        Some(other) => {
            return type_err(
                &args,
                format!("`default` must be a string, got {}", other.a_type()),
            );
        }
    };
    if !std::io::stdin().is_terminal() {
        return match default {
            Some(d) => Ok(Value::str(d)),
            None => Err(OkError::at(
                ErrKind::Value,
                args.span,
                format!("cannot ask \"{msg}\": there is no terminal"),
            )
            .with_help("give a default: prompt(msg, default: \"...\")")
            .maybe_note(parallel_note(interp))
            .into()),
        };
    }
    let shown = match &default {
        Some(d) => format!("{msg} [{d}] "),
        None => format!("{msg} "),
    };
    let answer = read_answer(&shown).map_err(|e| {
        OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot read the answer: {}", io_reason(&e)),
        )
    })?;
    Ok(Value::str(if answer.is_empty() {
        default.unwrap_or_default()
    } else {
        answer
    }))
}

/// Ask a yes/no question on the terminal. Shared with task `confirm`.
pub fn ask_yes_no(
    question: &str,
    default: bool,
    yes_flag: bool,
    span: Option<Span>,
) -> Result<bool, OkError> {
    if yes_flag {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        let mut e = OkError::new(
            ErrKind::Value,
            format!("cannot ask \"{question}\": there is no terminal"),
        );
        e.span = span;
        return Err(e.with_help("pass --yes before the script or task name to answer yes"));
    }
    let hint = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        let answer = read_answer(&format!("{question} {hint} ")).map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!("cannot read the answer: {}", io_reason(&e)),
            )
        })?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => eprintln!("please answer y or n"),
        }
    }
}

fn confirm(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "confirm", &["default"])?;
    count_args(&args, "confirm", 1, 1)?;
    let default = crate::list::bool_named(&opts, "default", &args)?;
    let msg = args.pos[0].display();
    Ok(Value::Bool(
        ask_yes_no(&msg, default, interp.run_opts.yes, Some(args.span))
            .map_err(|e| e.maybe_note(parallel_note(interp)))?,
    ))
}

/// Why a task has no terminal when it runs in parallel (SPEC.md 15.5).
fn parallel_note(interp: &Interp) -> Option<String> {
    interp.run_opts.parallel.then(|| {
        "this task runs in parallel with other tasks and has no terminal; ok --jobs 1 runs dependencies one at a time".to_string()
    })
}

// ----- hashing ---------------------------------------------------------------------

fn digest<D: sha2::Digest>(mut d: D, input: &mut dyn Read) -> std::io::Result<String> {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf)?;
        if n == 0 {
            break;
        }
        d.update(&buf[..n]);
    }
    Ok(hex::encode(d.finalize()))
}

fn hash(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "hash", &[])?;
    count_args(&args, "hash", 1, 2)?;
    let algo = match args.pos.get(1) {
        None => "sha256".to_string(),
        Some(Value::Str(s)) => s.to_ascii_lowercase(),
        Some(other) => {
            return type_err(
                &args,
                format!("the algorithm must be a string, got {}", other.a_type()),
            );
        }
    };
    let (mut reader, name): (Box<dyn Read>, String) = match &args.pos[0] {
        Value::Path(p) => {
            let f = std::fs::File::open(p.native()).map_err(|e| {
                OkError::at(
                    ErrKind::Io,
                    args.span,
                    format!("cannot read {}: {}", p.display(), io_reason(&e)),
                )
            })?;
            (Box::new(std::io::BufReader::new(f)), p.display())
        }
        Value::Bytes(b) => (
            Box::new(std::io::Cursor::new(b.to_vec())),
            "the bytes".into(),
        ),
        Value::Str(s) => {
            return Err(OkError::at(
                ErrKind::Type,
                args.span,
                "`hash` of a string is ambiguous: is it a file name or the text itself?",
            )
            .with_help(format!(
                "hash the file with hash(path({s:?})) or the text with hash(bytes(...))"
            ))
            .into());
        }
        other => {
            return type_err(
                &args,
                format!("`hash` needs a path or bytes, got {}", other.a_type()),
            );
        }
    };
    let r = match algo.as_str() {
        "sha256" => digest(sha2::Sha256::default(), &mut reader),
        "sha512" => digest(sha2::Sha512::default(), &mut reader),
        "sha1" => digest(sha1::Sha1::default(), &mut reader),
        "md5" => digest(md5::Md5::default(), &mut reader),
        other => {
            let help = suggest(other, ["sha256", "sha512", "sha1", "md5"])
                .map(|s| format!("did you mean \"{s}\"?"))
                .unwrap_or_else(|| "use sha256, sha512, sha1 or md5".into());
            return Err(OkError::at(
                ErrKind::Arg,
                args.span,
                format!("unknown hash algorithm \"{other}\""),
            )
            .with_help(help)
            .into());
        }
    };
    r.map(Value::str).map_err(|e| {
        OkError::at(
            ErrKind::Io,
            args.span,
            format!("cannot read {name}: {}", io_reason(&e)),
        )
        .into()
    })
}
