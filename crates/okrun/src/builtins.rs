//! Built-in functions available in milestone M1: output, conversions and a
//! few numeric helpers (SPEC.md 12.4, 12.5, 12.8). The data, file, command
//! and network functions of later milestones are added here as they land.

use std::io::Write as _;
use std::rc::Rc;

use crate::error::{ErrKind, OkError};
use crate::format;
use crate::interp::{Args, Builtin, Flow, Interp, R, Scope};
use crate::path::OkPath;
use crate::value::{Func, Record, Value};

/// Builtins that receive their arguments materialised.
pub const fn eager(name: &'static str, f: BuiltinFn) -> Builtin {
    Builtin {
        name,
        f,
        lazy: false,
    }
}

/// Builtins that stream lazy arguments (SPEC.md 8.3).
pub const fn streaming(name: &'static str, f: BuiltinFn) -> Builtin {
    Builtin {
        name,
        f,
        lazy: true,
    }
}

pub type BuiltinFn = fn(&mut Interp, Args) -> R<Value>;

#[rustfmt::skip]
static CORE: &[Builtin] = &[
    eager("print", print),
    eager("eprint", eprint),
    eager("str", str_),
    eager("int", int),
    eager("float", float),
    eager("type", type_),
    eager("len", len),
    eager("fail", fail),
    eager("exit", exit),
    eager("path", path),
    eager("floor", floor),
    eager("ceil", ceil),
    eager("round", round),
    eager("fmt", fmt),
    eager("bytes", bytes),
    eager("text", text),
    eager("range", range),
];

/// Every table of builtins.
fn tables() -> [&'static [Builtin]; 8] {
    [
        CORE,
        crate::list::BUILTINS,
        crate::formats::BUILTINS,
        crate::files::BUILTINS,
        crate::text::BUILTINS,
        crate::system::BUILTINS,
        crate::net::BUILTINS,
        crate::archive::BUILTINS,
    ]
}

/// The names of all builtin functions.
pub fn names() -> impl Iterator<Item = &'static str> {
    tables().into_iter().flatten().map(|b| b.name)
}

/// The globals that are values, not functions (SPEC.md 12.2, 12.3).
pub const GLOBAL_VALUES: &[&str] = &[
    "os",
    "env",
    "cwd",
    "invocation_dir",
    "home",
    "args",
    "script_dir",
];

/// What a builtin accepts, for the static checks of `ok self check`
/// (SPEC.md 17): how many positional arguments (counting the receiver of a
/// method call and the input of a pipeline stage) and which named ones. It
/// must agree with the argument checks inside each builtin; a test calls
/// every builtin to make sure it does.
pub struct BuiltinSig {
    pub name: &'static str,
    pub min: usize,
    /// `usize::MAX` for no limit.
    pub max: usize,
    pub named: &'static [&'static str],
}

const fn sig(
    name: &'static str,
    min: usize,
    max: usize,
    named: &'static [&'static str],
) -> BuiltinSig {
    BuiltinSig {
        name,
        min,
        max,
        named,
    }
}

const ANY: usize = usize::MAX;

#[rustfmt::skip]
pub static SIGNATURES: &[BuiltinSig] = &[
    sig("print", 0, ANY, &[]), sig("eprint", 0, ANY, &[]), sig("str", 1, 1, &[]),
    sig("int", 1, 1, &[]), sig("float", 1, 1, &[]), sig("type", 1, 1, &[]),
    sig("len", 1, 1, &[]), sig("fail", 1, 1, &[]), sig("exit", 0, 1, &[]),
    sig("path", 1, 1, &[]), sig("floor", 1, 1, &[]), sig("ceil", 1, 1, &[]),
    sig("round", 1, 1, &[]), sig("fmt", 2, 2, &[]), sig("bytes", 1, 1, &[]),
    sig("text", 1, 1, &[]), sig("range", 1, 3, &[]),
    // Lists and records (list.rs).
    sig("where", 2, 2, &[]), sig("map", 2, 2, &[]), sig("each", 2, 2, &[]),
    sig("take", 2, 2, &[]), sig("skip", 2, 2, &[]), sig("uniq", 1, 2, &[]),
    sig("flat", 1, 1, &[]), sig("enumerate", 1, 1, &[]), sig("count", 1, 2, &[]),
    sig("sum", 1, 2, &[]), sig("min", 1, 2, &[]), sig("max", 1, 2, &[]),
    sig("first", 1, 2, &[]), sig("last", 1, 2, &[]), sig("any", 2, 2, &[]),
    sig("all", 2, 2, &[]), sig("join", 1, 2, &[]), sig("lines", 1, 1, &[]),
    sig("sort", 1, 2, &["desc"]), sig("reverse", 1, 1, &[]), sig("group", 2, 2, &[]),
    sig("keys", 1, 1, &[]), sig("values", 1, 1, &[]), sig("entries", 1, 1, &[]),
    sig("from_entries", 1, 1, &[]), sig("get", 2, 3, &[]), sig("has", 2, 2, &[]),
    // Files and paths (files.rs).
    sig("copy", 2, 2, &[]), sig("move", 2, 2, &[]), sig("remove", 1, 1, &[]),
    sig("mkdir", 1, 1, &[]), sig("exists", 1, 1, &[]), sig("is_file", 1, 1, &[]),
    sig("is_dir", 1, 1, &[]), sig("glob", 1, 1, &["root", "hidden", "exclude"]),
    sig("list_dir", 0, 1, &[]), sig("stat", 1, 1, &[]), sig("touch", 1, 1, &[]),
    sig("tempdir", 0, 0, &[]), sig("tempfile", 0, 0, &["ext"]), sig("chmod", 2, 2, &[]),
    sig("abs", 1, 1, &[]), sig("relative_to", 2, 2, &[]), sig("with_ext", 2, 2, &[]),
    sig("parts", 1, 1, &[]), sig("is_abs", 1, 1, &[]),
    // Data formats (formats.rs).
    sig("read", 1, 2, &["delimiter", "header", "gzip"]),
    sig("write", 2, 3, &["append", "compact", "delimiter", "header", "gzip"]),
    sig("parse", 2, 2, &["delimiter", "header"]),
    sig("to", 2, 2, &["compact", "delimiter", "header"]),
    sig("stdin", 0, 1, &["delimiter", "header"]),
    // Network and archives.
    sig("fetch", 1, 1, &["method", "headers", "body", "json", "timeout", "to", "check", "bytes"]),
    sig("zip", 2, 2, &[]), sig("unzip", 2, 2, &[]), sig("tar", 2, 2, &[]), sig("untar", 2, 2, &[]),
    // Text (text.rs).
    sig("trim", 1, 1, &[]), sig("trim_start", 1, 1, &[]), sig("trim_end", 1, 1, &[]),
    sig("split", 1, 2, &["limit"]), sig("replace", 3, 3, &[]), sig("contains", 2, 2, &[]),
    sig("starts_with", 2, 2, &[]), sig("ends_with", 2, 2, &[]), sig("upper", 1, 1, &[]),
    sig("lower", 1, 1, &[]), sig("chars", 1, 1, &[]), sig("repeat", 2, 2, &[]),
    sig("pad", 2, 2, &["fill", "align"]), sig("match", 2, 2, &[]), sig("matches", 2, 2, &[]),
    sig("find", 2, 2, &[]), sig("sub", 3, 3, &[]),
    // Environment and system (system.rs).
    sig("which", 1, 1, &[]), sig("now", 0, 0, &[]), sig("time", 1, 1, &[]),
    sig("format", 2, 2, &["utc"]), sig("sleep", 1, 1, &[]), sig("duration", 1, 1, &[]),
    sig("prompt", 1, 1, &["default"]), sig("confirm", 1, 1, &["default"]),
    sig("hash", 1, 2, &[]),
];

/// The methods of `env` (SPEC.md 12.3) and their positional arguments.
pub const ENV_METHODS: &[(&str, usize, usize)] = &[
    ("get", 1, 2),
    ("has", 1, 1),
    ("set", 2, 2),
    ("remove", 1, 1),
    ("all", 0, 0),
];

/// The signature of a builtin function.
pub fn signature(name: &str) -> Option<&'static BuiltinSig> {
    SIGNATURES.iter().find(|s| s.name == name)
}

/// Declare the builtins, `os` and `args` in the global scope.
pub fn install(globals: &Rc<Scope>, script_args: Vec<String>) {
    for table in tables() {
        for b in table {
            globals.declare(b.name, Value::Func(Rc::new(Func::Builtin(b))), false);
        }
    }
    let mut os = Record::new();
    // Rust's `consts::OS` is empty on wasm32-wasip1; SPEC.md 19 names it "wasi".
    let os_name = if cfg!(target_os = "wasi") {
        "wasi"
    } else {
        std::env::consts::OS
    };
    os.insert("name", Value::str(os_name));
    os.insert("arch", Value::str(std::env::consts::ARCH));
    os.insert("exe", Value::str(std::env::consts::EXE_SUFFIX));
    globals.declare("os", Value::record(os), false);
    globals.declare("env", Value::Env, false);
    let cwd = std::env::current_dir().unwrap_or_default();
    globals.declare("cwd", Value::path(OkPath::from_native(&cwd)), false);
    globals.declare(
        "invocation_dir",
        Value::path(OkPath::from_native(&cwd)),
        false,
    );
    let home = crate::files::home_dir()
        .map(|h| Value::path(OkPath::from_native(&h)))
        .unwrap_or(Value::Null);
    globals.declare("home", home, false);
    let args = script_args.into_iter().map(Value::str).collect();
    globals.declare("args", Value::list(args), false);
}

pub(crate) fn arg_err<T>(args: &Args, msg: impl Into<String>) -> R<T> {
    Err(OkError::at(ErrKind::Arg, args.span, msg).into())
}

pub(crate) fn type_err<T>(args: &Args, msg: impl Into<String>) -> R<T> {
    Err(OkError::at(ErrKind::Type, args.span, msg).into())
}

/// Check the positional argument count and that no named arguments were
/// passed (for builtins that take none).
pub(crate) fn arity(args: &Args, name: &str, min: usize, max: usize) -> R<()> {
    if let Some((n, _, span)) = args.named.first() {
        return Err(OkError::at(
            ErrKind::Arg,
            *span,
            format!("`{name}` has no parameter `{n}`"),
        )
        .into());
    }
    let n = args.pos.len();
    if n < min || n > max {
        let want = if min == max {
            format!("{min}")
        } else if max == usize::MAX {
            format!("at least {min}")
        } else {
            format!("{min} to {max}")
        };
        return arg_err(
            args,
            format!(
                "`{name}` takes {want} argument{}, got {n}",
                if want == "1" { "" } else { "s" }
            ),
        );
    }
    Ok(())
}

fn write_line(to_stderr: bool, args: &Args) -> R<Value> {
    let line = args
        .pos
        .iter()
        .map(Value::display)
        .collect::<Vec<_>>()
        .join(" ");
    let result = if to_stderr {
        let mut e = std::io::stderr().lock();
        writeln!(e, "{line}")
    } else {
        let mut o = std::io::stdout().lock();
        writeln!(o, "{line}")
    };
    match result {
        Ok(()) => Ok(Value::Null),
        // Output closed (for example piped into `head`): stop quietly.
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Err(Flow::Exit(0)),
        Err(e) => {
            Err(OkError::at(ErrKind::Io, args.span, format!("cannot write output: {e}")).into())
        }
    }
}

fn print(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "print", 0, usize::MAX)?;
    write_line(false, &args)
}

fn eprint(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "eprint", 0, usize::MAX)?;
    write_line(true, &args)
}

fn str_(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "str", 1, 1)?;
    Ok(Value::str(args.pos[0].display()))
}

fn int(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "int", 1, 1)?;
    match &args.pos[0] {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Float(f) => float_to_int(f.trunc(), &args),
        Value::Bool(b) => Ok(Value::Int(i64::from(*b))),
        Value::Str(s) => s.trim().parse::<i64>().map(Value::Int).map_err(|_| {
            OkError::at(
                ErrKind::Value,
                args.span,
                format!("cannot convert {} to an int", Value::Str(s.clone()).repr()),
            )
            .into()
        }),
        other => type_err(
            &args,
            format!("cannot convert {} to an int", other.a_type()),
        ),
    }
}

fn float_to_int(f: f64, args: &Args) -> R<Value> {
    if !f.is_finite() || f < i64::MIN as f64 || f >= i64::MAX as f64 {
        return Err(OkError::at(
            ErrKind::Value,
            args.span,
            "the number is too large for an int",
        )
        .into());
    }
    Ok(Value::Int(f as i64))
}

fn float(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "float", 1, 1)?;
    match &args.pos[0] {
        Value::Int(i) => Ok(Value::Float(*i as f64)),
        Value::Float(f) => Ok(Value::Float(*f)),
        Value::Bool(b) => Ok(Value::Float(if *b { 1.0 } else { 0.0 })),
        Value::Str(s) => match s.trim().parse::<f64>() {
            Ok(f) if f.is_finite() => Ok(Value::Float(f)),
            _ => Err(OkError::at(
                ErrKind::Value,
                args.span,
                format!("cannot convert {} to a float", Value::Str(s.clone()).repr()),
            )
            .into()),
        },
        other => type_err(
            &args,
            format!("cannot convert {} to a float", other.a_type()),
        ),
    }
}

fn type_(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "type", 1, 1)?;
    Ok(Value::str(args.pos[0].type_name()))
}

fn len(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "len", 1, 1)?;
    let n = match &args.pos[0] {
        Value::Str(s) => s.chars().count(),
        Value::Bytes(b) => b.len(),
        Value::List(l) => l.len(),
        Value::Record(r) => r.len(),
        other => {
            return type_err(
                &args,
                format!(
                    "`len` needs a string, bytes, list or record, got {}",
                    other.a_type()
                ),
            );
        }
    };
    Ok(Value::Int(n as i64))
}

fn fail(interp: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "fail", 1, 1)?;
    if let Value::Record(r) = &args.pos[0] {
        if let Some(original) = interp.original_error(r) {
            return Err(original.into());
        }
        if let (Some(Value::Str(kind)), Some(Value::Str(msg))) = (r.get("kind"), r.get("message")) {
            let kind = ErrKind::from_name(kind).unwrap_or(ErrKind::Fail);
            return Err(OkError::at(kind, args.span, msg.to_string()).into());
        }
    }
    Err(OkError::at(ErrKind::Fail, args.span, args.pos[0].display()).into())
}

fn exit(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "exit", 0, 1)?;
    match args.pos.first() {
        None => Err(Flow::Exit(0)),
        Some(Value::Int(c)) if (0..=255).contains(c) => Err(Flow::Exit(*c as i32)),
        Some(Value::Int(c)) => arg_err(&args, format!("exit codes go from 0 to 255, got {c}")),
        Some(other) => type_err(
            &args,
            format!("`exit` needs an int, got {}", other.a_type()),
        ),
    }
}

fn path(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "path", 1, 1)?;
    match &args.pos[0] {
        Value::Str(s) => Ok(Value::path(OkPath::parse(s))),
        Value::Path(p) => Ok(Value::Path(p.clone())),
        other => type_err(
            &args,
            format!("`path` needs a string, got {}", other.a_type()),
        ),
    }
}

pub(crate) fn int_arg(args: &Args, i: usize, fname: &str) -> R<i64> {
    match &args.pos[i] {
        Value::Int(n) => Ok(*n),
        other => type_err(
            args,
            format!("`{fname}` needs ints, got {}", other.a_type()),
        ),
    }
}

fn range(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "range", 1, 3)?;
    let (start, end, step) = match args.pos.len() {
        1 => (0, int_arg(&args, 0, "range")?, 1),
        2 => (int_arg(&args, 0, "range")?, int_arg(&args, 1, "range")?, 1),
        _ => (
            int_arg(&args, 0, "range")?,
            int_arg(&args, 1, "range")?,
            int_arg(&args, 2, "range")?,
        ),
    };
    if step == 0 {
        return arg_err(&args, "the step of `range` cannot be 0");
    }
    // Lazy, so `range(1_000_000_000) | take 3` and `for i in range(n)` never
    // build the whole list.
    Ok(crate::lazy::Lazy::list(Box::new(
        crate::lazy::RangeSource {
            next: start,
            end,
            step,
            done: false,
        },
    )))
}

fn rounding(args: Args, name: &str, f: fn(f64) -> f64) -> R<Value> {
    arity(&args, name, 1, 1)?;
    match &args.pos[0] {
        Value::Int(i) => Ok(Value::Int(*i)),
        Value::Float(x) => float_to_int(f(*x), &args),
        other => type_err(
            &args,
            format!("`{name}` needs a number, got {}", other.a_type()),
        ),
    }
}

fn floor(_: &mut Interp, args: Args) -> R<Value> {
    rounding(args, "floor", f64::floor)
}

fn ceil(_: &mut Interp, args: Args) -> R<Value> {
    rounding(args, "ceil", f64::ceil)
}

fn round(_: &mut Interp, args: Args) -> R<Value> {
    rounding(args, "round", f64::round)
}

fn fmt(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "fmt", 2, 2)?;
    let Value::Str(spec) = &args.pos[1] else {
        return type_err(
            &args,
            format!(
                "the format spec must be a string, got {}",
                args.pos[1].a_type()
            ),
        );
    };
    let parsed =
        format::parse_spec(spec).map_err(|m| OkError::at(ErrKind::Format, args.span, m))?;
    format::apply(&args.pos[0], &parsed)
        .map(Value::str)
        .map_err(|m| OkError::at(ErrKind::Format, args.span, m).into())
}

fn bytes(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "bytes", 1, 1)?;
    match &args.pos[0] {
        Value::Str(s) => Ok(Value::Bytes(s.as_bytes().into())),
        Value::Bytes(b) => Ok(Value::Bytes(b.clone())),
        other => type_err(
            &args,
            format!("`bytes` needs a string, got {}", other.a_type()),
        ),
    }
}

fn text(_: &mut Interp, args: Args) -> R<Value> {
    arity(&args, "text", 1, 1)?;
    match &args.pos[0] {
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => Ok(Value::str(s)),
            Err(e) => Err(OkError::at(
                ErrKind::Value,
                args.span,
                format!(
                    "the bytes are not valid UTF-8 (at byte {})",
                    e.valid_up_to()
                ),
            )
            .into()),
        },
        Value::Str(s) => Ok(Value::Str(s.clone())),
        other => type_err(&args, format!("`text` needs bytes, got {}", other.a_type())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message of the error raised by running `src`, if any.
    fn error_of(src: &str) -> Option<String> {
        let mut interp = Interp::new(Vec::new());
        let r = interp.run_source("<test>", src, std::env::temp_dir());
        interp.cleanup();
        match r {
            Err(Flow::Err(e)) => Some(e.message),
            _ => None,
        }
    }

    /// A call of builtin `name` with `pos` null arguments and `named`
    /// arguments. The method form `null.name(...)` is used when there is an
    /// argument, so that keywords like `match` can be called too.
    fn call(name: &str, pos: usize, named: &str) -> String {
        let mut args: Vec<String> = vec!["null".to_string(); pos.saturating_sub(1)];
        if !named.is_empty() {
            args.push(named.to_string());
        }
        if pos == 0 {
            format!("{name}({})", args.join(", "))
        } else {
            format!("null.{name}({})", args.join(", "))
        }
    }

    #[test]
    fn every_builtin_has_a_signature() {
        for n in names() {
            assert!(signature(n).is_some(), "no signature for `{n}`");
        }
        for s in SIGNATURES {
            assert!(
                names().any(|n| n == s.name),
                "`{}` is not a builtin",
                s.name
            );
        }
    }

    #[test]
    fn signatures_agree_with_the_argument_checks() {
        // These read the terminal or print; their valid calls are not run.
        let interactive = ["prompt", "confirm", "stdin", "print", "eprint"];
        for s in SIGNATURES {
            let count_error = format!("`{}` takes ", s.name);
            let named_error = format!("`{}` has no parameter", s.name);
            if s.min > 0 {
                let e = error_of(&call(s.name, s.min - 1, "")).unwrap_or_default();
                assert!(
                    e.starts_with(&count_error),
                    "{}: too few gave {e:?}",
                    s.name
                );
            }
            if s.max != ANY {
                let e = error_of(&call(s.name, s.max + 1, "")).unwrap_or_default();
                assert!(
                    e.starts_with(&count_error),
                    "{}: too many gave {e:?}",
                    s.name
                );
            }
            let e = error_of(&call(s.name, s.min, "zzz_unknown: 1")).unwrap_or_default();
            assert!(
                e.starts_with(&named_error),
                "{}: unknown named gave {e:?}",
                s.name
            );
            if interactive.contains(&s.name) {
                continue;
            }
            let most = if s.max == ANY { s.min + 2 } else { s.max };
            for n in [s.min, most] {
                let e = error_of(&call(s.name, n, "")).unwrap_or_default();
                assert!(
                    !e.starts_with(&count_error),
                    "{}: {n} arguments gave {e:?}",
                    s.name
                );
            }
            for named in s.named {
                let e =
                    error_of(&call(s.name, s.min, &format!("{named}: null"))).unwrap_or_default();
                assert!(
                    !e.starts_with(&named_error),
                    "{}: `{named}` gave {e:?}",
                    s.name
                );
            }
        }
    }
}
