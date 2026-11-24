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

/// Largest list `range` builds. Ranges become lazy with the data functions
/// of milestone M2; until then this keeps a typo from exhausting memory.
const MAX_RANGE: i64 = 10_000_000;

static BUILTINS: &[Builtin] = &[
    Builtin {
        name: "print",
        f: print,
    },
    Builtin {
        name: "eprint",
        f: eprint,
    },
    Builtin {
        name: "str",
        f: str_,
    },
    Builtin {
        name: "int",
        f: int,
    },
    Builtin {
        name: "float",
        f: float,
    },
    Builtin {
        name: "type",
        f: type_,
    },
    Builtin {
        name: "len",
        f: len,
    },
    Builtin {
        name: "fail",
        f: fail,
    },
    Builtin {
        name: "exit",
        f: exit,
    },
    Builtin {
        name: "path",
        f: path,
    },
    Builtin {
        name: "range",
        f: range,
    },
    Builtin {
        name: "floor",
        f: floor,
    },
    Builtin {
        name: "ceil",
        f: ceil,
    },
    Builtin {
        name: "round",
        f: round,
    },
    Builtin {
        name: "fmt",
        f: fmt,
    },
    Builtin {
        name: "bytes",
        f: bytes,
    },
    Builtin {
        name: "text",
        f: text,
    },
];

/// Declare the builtins, `os` and `args` in the global scope.
pub fn install(globals: &Rc<Scope>, script_args: Vec<String>) {
    for b in BUILTINS {
        globals.declare(b.name, Value::Func(Rc::new(Func::Builtin(b))), false);
    }
    let mut os = Record::new();
    os.insert("name", Value::str(std::env::consts::OS));
    os.insert("arch", Value::str(std::env::consts::ARCH));
    os.insert("exe", Value::str(std::env::consts::EXE_SUFFIX));
    globals.declare("os", Value::record(os), false);
    let args = script_args.into_iter().map(Value::str).collect();
    globals.declare("args", Value::list(args), false);
}

fn arg_err<T>(args: &Args, msg: impl Into<String>) -> R<T> {
    Err(OkError::at(ErrKind::Arg, args.span, msg).into())
}

fn type_err<T>(args: &Args, msg: impl Into<String>) -> R<T> {
    Err(OkError::at(ErrKind::Type, args.span, msg).into())
}

/// Check the positional argument count and that no named arguments were
/// passed (for builtins that take none).
fn arity(args: &Args, name: &str, min: usize, max: usize) -> R<()> {
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

fn int_arg(args: &Args, i: usize, fname: &str) -> R<i64> {
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
    let count = if step > 0 {
        (end as i128 - start as i128 + step as i128 - 1) / step as i128
    } else {
        (start as i128 - end as i128 - step as i128 - 1) / (-step as i128)
    };
    if count > MAX_RANGE as i128 {
        return arg_err(
            &args,
            format!("range of {count} numbers is too large (the limit is {MAX_RANGE})"),
        );
    }
    let mut out = Vec::with_capacity(count.max(0) as usize);
    let mut i = start;
    while (step > 0 && i < end) || (step < 0 && i > end) {
        out.push(Value::Int(i));
        match i.checked_add(step) {
            Some(n) => i = n,
            None => break,
        }
    }
    Ok(Value::list(out))
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
