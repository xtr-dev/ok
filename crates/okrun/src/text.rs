//! Text functions and regular expressions (SPEC.md 12.4).
//!
//! Regexes use the `regex` crate's syntax (linear time, no backreferences).
//! Compiled patterns are cached, so a regex in a loop is compiled once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::builtins::{arg_err, eager, type_err};
use crate::error::{ErrKind, OkError};
use crate::interp::{Args, Builtin, Interp, R};
use crate::list::{count_args, named};
use crate::value::{Record, Value, equal};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("trim", trim),
    eager("trim_start", trim_start),
    eager("trim_end", trim_end),
    eager("split", split),
    eager("replace", replace),
    eager("contains", contains),
    eager("starts_with", starts_with),
    eager("ends_with", ends_with),
    eager("upper", upper),
    eager("lower", lower),
    eager("chars", chars),
    eager("repeat", repeat),
    eager("pad", pad),
    eager("match", match_),
    eager("matches", matches),
    eager("find", find),
    eager("sub", sub),
];

fn str_at(args: &Args, i: usize, fname: &str) -> R<Rc<str>> {
    match args.pos.get(i) {
        Some(Value::Str(s)) => Ok(s.clone()),
        Some(other) => {
            let mut e = OkError::at(
                ErrKind::Type,
                args.span,
                format!("`{fname}` needs a string, got {}", other.a_type()),
            );
            if matches!(other, Value::Path(_)) {
                e = e.with_help("convert the path first with str(p)");
            }
            Err(e.into())
        }
        None => arg_err(args, format!("`{fname}` needs a string")),
    }
}

fn one_str(mut args: Args, fname: &str, f: impl Fn(&str) -> String) -> R<Value> {
    named(&mut args, fname, &[])?;
    count_args(&args, fname, 1, 1)?;
    Ok(Value::str(f(&str_at(&args, 0, fname)?)))
}

fn trim(_: &mut Interp, args: Args) -> R<Value> {
    one_str(args, "trim", |s| s.trim().to_string())
}

fn trim_start(_: &mut Interp, args: Args) -> R<Value> {
    one_str(args, "trim_start", |s| s.trim_start().to_string())
}

fn trim_end(_: &mut Interp, args: Args) -> R<Value> {
    one_str(args, "trim_end", |s| s.trim_end().to_string())
}

fn upper(_: &mut Interp, args: Args) -> R<Value> {
    one_str(args, "upper", str::to_uppercase)
}

fn lower(_: &mut Interp, args: Args) -> R<Value> {
    one_str(args, "lower", str::to_lowercase)
}

fn split(_: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "split", &["limit"])?;
    count_args(&args, "split", 1, 2)?;
    let s = str_at(&args, 0, "split")?;
    let limit = match opts.get("limit") {
        None | Some(Value::Null) => None,
        Some(Value::Int(n)) if *n >= 1 => Some(*n as usize),
        Some(other) => {
            return type_err(
                &args,
                format!("`limit` must be an int of 1 or more, got {}", other.repr()),
            );
        }
    };
    let parts: Vec<Value> = match args.pos.get(1) {
        None | Some(Value::Null) => {
            let words: Vec<&str> = s.split_whitespace().collect();
            match limit {
                Some(n) if words.len() > n => {
                    // Keep the rest of the text after the n-1th word intact.
                    let mut out: Vec<Value> =
                        words[..n - 1].iter().map(|w| Value::str(*w)).collect();
                    let mut rest = &*s;
                    for _ in 0..n - 1 {
                        rest = rest.trim_start();
                        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
                        rest = &rest[end..];
                    }
                    out.push(Value::str(rest.trim()));
                    out
                }
                _ => words.into_iter().map(Value::str).collect(),
            }
        }
        Some(Value::Str(sep)) if sep.is_empty() => {
            return arg_err(
                &args,
                "the separator of `split` cannot be empty; use chars(s) for characters",
            );
        }
        Some(Value::Str(sep)) => match limit {
            Some(n) => s.splitn(n, &**sep).map(Value::str).collect(),
            None => s.split(&**sep).map(Value::str).collect(),
        },
        Some(other) => {
            return type_err(
                &args,
                format!(
                    "the separator of `split` must be a string, got {}",
                    other.a_type()
                ),
            );
        }
    };
    Ok(Value::list(parts))
}

fn replace(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "replace", &[])?;
    count_args(&args, "replace", 3, 3)?;
    let s = str_at(&args, 0, "replace")?;
    let old = str_at(&args, 1, "replace")?;
    let new = str_at(&args, 2, "replace")?;
    if old.is_empty() {
        return arg_err(&args, "`replace` cannot replace an empty string");
    }
    Ok(Value::str(s.replace(&*old, &new)))
}

fn contains(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "contains", &[])?;
    count_args(&args, "contains", 2, 2)?;
    Ok(Value::Bool(match (&args.pos[0], &args.pos[1]) {
        (Value::Str(s), Value::Str(sub)) => s.contains(&**sub),
        (Value::Str(_), other) => {
            return type_err(
                &args,
                format!("a string can only contain a string, got {}", other.a_type()),
            );
        }
        (Value::List(items), x) => items.iter().any(|i| equal(i, x)),
        (Value::Record(r), Value::Str(k)) => r.contains(k),
        (Value::Record(_), other) => {
            return type_err(
                &args,
                format!("record keys are strings, got {}", other.a_type()),
            );
        }
        (other, _) => {
            return type_err(
                &args,
                format!(
                    "`contains` needs a string, list or record, got {}",
                    other.a_type()
                ),
            );
        }
    }))
}

fn starts_with(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "starts_with", &[])?;
    count_args(&args, "starts_with", 2, 2)?;
    Ok(Value::Bool(
        str_at(&args, 0, "starts_with")?.starts_with(&*str_at(&args, 1, "starts_with")?),
    ))
}

fn ends_with(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "ends_with", &[])?;
    count_args(&args, "ends_with", 2, 2)?;
    Ok(Value::Bool(
        str_at(&args, 0, "ends_with")?.ends_with(&*str_at(&args, 1, "ends_with")?),
    ))
}

fn chars(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "chars", &[])?;
    count_args(&args, "chars", 1, 1)?;
    let s = str_at(&args, 0, "chars")?;
    Ok(Value::list(
        s.chars().map(|c| Value::str(c.to_string())).collect(),
    ))
}

fn repeat(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "repeat", &[])?;
    count_args(&args, "repeat", 2, 2)?;
    let s = str_at(&args, 0, "repeat")?;
    let Value::Int(n) = args.pos[1] else {
        return type_err(
            &args,
            format!("`repeat` needs a count, got {}", args.pos[1].a_type()),
        );
    };
    if n < 0 || (n as u128) * (s.len() as u128) > interp.max_built_size() as u128 {
        return arg_err(&args, format!("cannot repeat a string {n} times"));
    }
    Ok(Value::str(s.repeat(n as usize)))
}

fn pad(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "pad", &["fill", "align"])?;
    count_args(&args, "pad", 2, 2)?;
    let s = args.pos[0].display();
    let Value::Int(width) = args.pos[1] else {
        return type_err(
            &args,
            format!(
                "the width of `pad` must be an int, got {}",
                args.pos[1].a_type()
            ),
        );
    };
    let fill = match opts.get("fill") {
        None => ' ',
        Some(Value::Str(f)) if f.chars().count() == 1 => f.chars().next().unwrap_or(' '),
        Some(other) => {
            return type_err(
                &args,
                format!("`fill` must be a single character, got {}", other.repr()),
            );
        }
    };
    let align = match opts.get("align") {
        None => "left".to_string(),
        Some(Value::Str(a)) if matches!(&**a, "left" | "right" | "center") => a.to_string(),
        // A string that is not one of the choices is a bad argument, not a
        // wrong type, like an unknown hash algorithm or format.
        Some(Value::Str(a)) => {
            let help = crate::error::suggest(a, ["left", "right", "center"])
                .map(|s| format!("did you mean \"{s}\"?"));
            return Err(OkError::at(
                ErrKind::Arg,
                args.span,
                format!("`align` must be \"left\", \"right\" or \"center\", got \"{a}\""),
            )
            .maybe_help(help)
            .into());
        }
        Some(other) => {
            return type_err(
                &args,
                format!(
                    "`align` must be \"left\", \"right\" or \"center\", got {}",
                    other.repr()
                ),
            );
        }
    };
    let len = s.chars().count();
    let width = width.max(0) as usize;
    if len >= width {
        return Ok(Value::str(s));
    }
    let n = width - len;
    let max = interp.max_built_size();
    if (n as u128) * (fill.len_utf8() as u128) + s.len() as u128 > max as u128 {
        return arg_err(
            &args,
            format!(
                "cannot pad a string to {width} characters: the result would be over {}",
                crate::list::byte_size(max as u64)
            ),
        );
    }
    let f = |k: usize| fill.to_string().repeat(k);
    Ok(Value::str(match align.as_str() {
        "right" => format!("{}{s}", f(n)),
        "center" => format!("{}{s}{}", f(n / 2), f(n - n / 2)),
        _ => format!("{s}{}", f(n)),
    }))
}

// ----- regex -------------------------------------------------------------------

/// Compiled regexes kept for reuse. A script that builds patterns from data
/// (one per input line, say) would otherwise grow the cache without bound,
/// so it is emptied when it reaches this many entries. Patterns written in
/// the source are few and are compiled again on their next use.
const REGEX_CACHE_LIMIT: usize = 256;

thread_local! {
    static REGEXES: RefCell<HashMap<String, regex::Regex>> = RefCell::new(HashMap::new());
}

fn regex_at(args: &Args, i: usize, fname: &str) -> R<regex::Regex> {
    let pattern = str_at(args, i, fname)?;
    REGEXES.with(|cache| {
        if let Some(r) = cache.borrow().get(&*pattern) {
            return Ok(r.clone());
        }
        let r = regex::Regex::new(&pattern).map_err(|e| {
            let msg = e.to_string();
            let short = msg
                .lines()
                .last()
                .unwrap_or(&msg)
                .trim()
                .trim_start_matches("error: ")
                .to_string();
            OkError::at(
                ErrKind::Value,
                args.span,
                format!("invalid regex {:?}: {short}", &*pattern),
            )
            .with_help(
                "ok regexes use the Rust regex syntax; write them in single quotes, like '\\d+'",
            )
        })?;
        let mut cache = cache.borrow_mut();
        if cache.len() >= REGEX_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(pattern.to_string(), r.clone());
        Ok(r)
    })
}

fn match_record(re: &regex::Regex, caps: &regex::Captures) -> Value {
    let whole = caps.get(0).expect("group 0 exists");
    let mut r = Record::new();
    r.insert("text", Value::str(whole.as_str()));
    r.insert("start", Value::Int(whole.start() as i64));
    r.insert("end", Value::Int(whole.end() as i64));
    let groups = (1..caps.len())
        .map(|i| {
            caps.get(i)
                .map(|m| Value::str(m.as_str()))
                .unwrap_or(Value::Null)
        })
        .collect();
    r.insert("groups", Value::list(groups));
    let mut named = Record::new();
    for name in re.capture_names().flatten() {
        named.insert(
            name,
            caps.name(name)
                .map(|m| Value::str(m.as_str()))
                .unwrap_or(Value::Null),
        );
    }
    r.insert("named", Value::record(named));
    Value::record(r)
}

fn match_(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "match", &[])?;
    count_args(&args, "match", 2, 2)?;
    let s = str_at(&args, 0, "match")?;
    let re = regex_at(&args, 1, "match")?;
    Ok(re
        .captures(&s)
        .map(|c| match_record(&re, &c))
        .unwrap_or(Value::Null))
}

fn matches(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "matches", &[])?;
    count_args(&args, "matches", 2, 2)?;
    let s = str_at(&args, 0, "matches")?;
    Ok(Value::Bool(regex_at(&args, 1, "matches")?.is_match(&s)))
}

fn find(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "find", &[])?;
    count_args(&args, "find", 2, 2)?;
    let s = str_at(&args, 0, "find")?;
    let re = regex_at(&args, 1, "find")?;
    Ok(Value::list(
        re.captures_iter(&s)
            .map(|c| match_record(&re, &c))
            .collect(),
    ))
}

fn sub(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "sub", &[])?;
    count_args(&args, "sub", 3, 3)?;
    let s = str_at(&args, 0, "sub")?;
    let re = regex_at(&args, 1, "sub")?;
    match &args.pos[2] {
        Value::Str(rep) => Ok(Value::str(re.replace_all(&s, &**rep).into_owned())),
        f @ Value::Func(_) => {
            let mut out = String::new();
            let mut last = 0;
            for caps in re.captures_iter(&s) {
                let m = caps.get(0).expect("group 0 exists");
                out.push_str(&s[last..m.start()]);
                let v = interp.call(f, vec![match_record(&re, &caps)], args.span)?;
                out.push_str(&v.display());
                last = m.end();
            }
            out.push_str(&s[last..]);
            Ok(Value::str(out))
        }
        other => type_err(
            &args,
            format!(
                "the replacement must be a string or a function, got {}",
                other.a_type()
            ),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_regex_cache_stays_bounded() {
        let mut i = Interp::new(vec![]);
        let src = "for n in range(1000) { let _ = \"x{n}\".matches(\"x{n}\") }";
        assert!(i.run_source("<test>", src, std::env::temp_dir()).is_ok());
        assert!(REGEXES.with(|c| c.borrow().len()) <= REGEX_CACHE_LIMIT);
    }
}
