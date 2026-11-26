//! Built-in data formats (SPEC.md section 13): `read`, `write`, `parse`, `to`
//! and `stdin`.
//!
//! Line- and row-oriented formats (text, lines, JSON Lines, CSV, TSV) are
//! read lazily and written as a stream, so pipelines over large files run in
//! bounded memory. Document formats (JSON, TOML, YAML, .env) are read and
//! written whole.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::rc::Rc;

use crate::builtins::{arg_err, eager, streaming, type_err};
use crate::error::{ErrKind, OkError, suggest};
use crate::interp::{Args, Builtin, Interp, R, io_reason};
use crate::lazy::{self, Lazy, ReaderLines, Source, at_span};
use crate::list::{bool_named, count_args, named};
use crate::path::OkPath;
use crate::source::Span;
use crate::value::{
    Record, Value, days_from_civil, format_duration, format_float, format_time_utc,
};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    eager("read", read),
    streaming("write", write),
    eager("parse", parse),
    eager("to", to),
    eager("stdin", stdin),
];

const FORMATS: &[&str] = &[
    "json", "jsonl", "csv", "tsv", "lines", "text", "toml", "yaml", "env", "bytes",
];

/// Whether a file name says the file is gzip-compressed (SPEC.md 13.1.1).
fn gz_name(p: &OkPath) -> bool {
    p.ext().eq_ignore_ascii_case("gz")
}

/// The format a file name implies (SPEC.md 13.1); for a compressed file,
/// the name without `.gz` decides (13.1.1).
pub fn format_for(p: &OkPath) -> &'static str {
    if gz_name(p) {
        let name = p.name();
        return format_for(&OkPath::parse(&name[..name.len() - 3]));
    }
    let name = p.name();
    if name == ".env" || name.starts_with(".env.") {
        return "env";
    }
    match p.ext().to_ascii_lowercase().as_str() {
        "json" => "json",
        "jsonl" | "ndjson" => "jsonl",
        "csv" => "csv",
        "tsv" => "tsv",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "env" => "env",
        _ => "text",
    }
}

fn check_format(name: &str, args: &Args, extra: &[&'static str]) -> R<&'static str> {
    if let Some(f) = FORMATS.iter().chain(extra.iter()).find(|f| **f == name) {
        return Ok(f);
    }
    let aliases = match name {
        "yml" => Some("yaml"),
        "txt" | "string" => Some("text"),
        "ndjson" => Some("jsonl"),
        _ => None,
    };
    let help = aliases
        .map(|a| format!("use \"{a}\""))
        .or_else(|| {
            suggest(name, FORMATS.iter().copied()).map(|s| format!("did you mean \"{s}\"?"))
        })
        .unwrap_or_else(|| format!("the formats are {}", FORMATS.join(", ")));
    Err(OkError::at(
        ErrKind::Arg,
        args.span,
        format!("unknown format \"{name}\""),
    )
    .with_help(help)
    .into())
}

fn path_arg(args: &Args, i: usize, fname: &str) -> R<OkPath> {
    match args.pos.get(i) {
        Some(Value::Str(s)) => Ok(OkPath::parse(s)),
        Some(Value::Path(p)) => Ok((**p).clone()),
        Some(other) => type_err(
            args,
            format!("`{fname}` needs a path, got {}", other.a_type()),
        ),
        None => arg_err(args, format!("`{fname}` needs a path")),
    }
}

fn format_arg(args: &Args, i: usize) -> R<Option<String>> {
    match args.pos.get(i) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Str(s)) => Ok(Some(s.to_string())),
        Some(other) => type_err(
            args,
            format!(
                "the format must be a string like \"json\", got {}",
                other.a_type()
            ),
        ),
    }
}

/// CSV and TSV options (`delimiter`, `header`).
struct Table {
    delimiter: u8,
    header: bool,
    tsv: bool,
}

fn table_opts(opts: &HashMap<String, Value>, fmt: &str, args: &Args) -> R<Table> {
    let tsv = fmt == "tsv";
    let delimiter = match opts.get("delimiter") {
        None => {
            if tsv {
                b'\t'
            } else {
                b','
            }
        }
        Some(Value::Str(s)) if s.len() == 1 && s.is_ascii() => s.as_bytes()[0],
        Some(other) => {
            return type_err(
                args,
                format!(
                    "`delimiter` must be a single ASCII character, got {}",
                    other.repr()
                ),
            );
        }
    };
    let header = match opts.get("header") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            return type_err(
                args,
                format!("`header` must be true or false, got {}", other.a_type()),
            );
        }
    };
    Ok(Table {
        delimiter,
        header,
        tsv,
    })
}

fn format_err(args: &Args, msg: String) -> crate::interp::Flow {
    OkError::at(ErrKind::Format, args.span, msg).into()
}

/// Whether to (de)compress: the `gzip:` option, or else the file name.
fn use_gzip(opts: &HashMap<String, Value>, p: &OkPath, args: &Args) -> R<bool> {
    match opts.get("gzip") {
        None | Some(Value::Null) => Ok(gz_name(p)),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => type_err(
            args,
            format!("`gzip` must be true or false, got {}", other.a_type()),
        ),
    }
}

/// A gzip decoder whose errors say that the data is not gzip, because the
/// decoder's own messages ("invalid gzip header") do not name the cause.
/// Consecutive gzip members are read as one stream.
struct GzRead<R: Read>(flate2::read::MultiGzDecoder<R>);

impl<R: Read> Read for GzRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0
            .read(buf)
            .map_err(|e| std::io::Error::new(e.kind(), format!("not valid gzip data ({e})")))
    }
}

// ----- builtins ---------------------------------------------------------------------

fn read(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "read", &["delimiter", "header", "gzip"])?;
    count_args(&args, "read", 1, 2)?;
    let p = path_arg(&args, 0, "read")?;
    let fmt = match format_arg(&args, 1)? {
        Some(f) => check_format(&f, &args, &[])?,
        None => format_for(&p),
    };
    let gzip = use_gzip(&opts, &p, &args)?;
    let name = p.display();
    let open_file = || {
        std::fs::File::open(p.native()).map_err(|e| {
            let mut err = OkError::at(
                ErrKind::Io,
                args.span,
                format!("cannot read {name}: {}", io_reason(&e)),
            );
            if p.native().is_dir() {
                err = err.with_help("this is a directory; list it with list_dir(p)");
            }
            crate::interp::Flow::from(err)
        })
    };
    let open = || -> R<Box<dyn Read>> {
        let file = open_file()?;
        Ok(if gzip {
            Box::new(GzRead(flate2::read::MultiGzDecoder::new(BufReader::new(
                file,
            ))))
        } else {
            Box::new(file)
        })
    };
    let read_all = |mut r: Box<dyn Read>| -> R<Vec<u8>> {
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).map_err(|e| {
            crate::interp::Flow::from(OkError::at(
                ErrKind::Io,
                args.span,
                format!("cannot read {name}: {}", io_reason(&e)),
            ))
        })?;
        Ok(buf)
    };
    match fmt {
        "text" | "lines" | "jsonl" => {
            let src = ReaderLines::new(Box::new(BufReader::new(open()?)), name.clone());
            Ok(lazy_lines(fmt, Box::new(src), name))
        }
        "csv" | "tsv" => {
            let t = table_opts(&opts, fmt, &args)?;
            Ok(Lazy::list(Box::new(CsvSource::new(
                Box::new(open()?),
                name,
                t,
            ))))
        }
        "bytes" => Ok(Value::Bytes(read_all(open()?)?.into())),
        doc => {
            let Ok(text) = String::from_utf8(read_all(open()?)?) else {
                return Err(OkError::at(
                    ErrKind::Format,
                    args.span,
                    format!("{name} is not valid UTF-8 text"),
                )
                .with_help("read binary data with read(p, \"bytes\")")
                .into());
            };
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            let _ = interp;
            parse_document(doc, text, &name).map_err(|m| format_err(&args, m))
        }
    }
}

fn lazy_lines(fmt: &str, src: Box<dyn Source>, name: String) -> Value {
    match fmt {
        "text" => Lazy::text(src),
        "lines" => Lazy::list(Box::new(lazy::TextToLines { up: src })),
        _ => Lazy::list(Box::new(JsonLines {
            up: src,
            name,
            line: 0,
        })),
    }
}

fn parse(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "parse", &["delimiter", "header"])?;
    count_args(&args, "parse", 2, 2)?;
    let Some(f) = format_arg(&args, 1)? else {
        return arg_err(&args, "`parse` needs a format, like parse(text, \"json\")");
    };
    let fmt = check_format(&f, &args, &["yaml_all"])?;
    let text: Rc<str> = match &args.pos[0] {
        Value::Str(s) => s.clone(),
        Value::Bytes(b) if fmt == "bytes" => return Ok(Value::Bytes(b.clone())),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => s.into(),
            Err(_) => {
                return Err(format_err(
                    &args,
                    "the bytes are not valid UTF-8 text".into(),
                ));
            }
        },
        other => return type_err(&args, format!("`parse` needs text, got {}", other.a_type())),
    };
    let name = "the text".to_string();
    let span = args.span;
    match fmt {
        "text" => Ok(Value::Str(text)),
        "bytes" => Ok(Value::Bytes(text.as_bytes().into())),
        "lines" | "jsonl" => {
            let src = ReaderLines::new(
                Box::new(std::io::Cursor::new(text.as_bytes().to_vec())),
                name.clone(),
            );
            let v = lazy_lines(fmt, Box::new(src), name);
            lazy::force(interp, v, span)
        }
        "csv" | "tsv" => {
            let t = table_opts(&opts, fmt, &args)?;
            let src = CsvSource::new(
                Box::new(std::io::Cursor::new(text.as_bytes().to_vec())),
                name,
                t,
            );
            lazy::collect(interp, Box::new(src), span)
        }
        doc => parse_document(doc, text.strip_prefix('\u{feff}').unwrap_or(&text), &name)
            .map_err(|m| format_err(&args, m)),
    }
}

fn to(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "to", &["compact", "delimiter", "header"])?;
    count_args(&args, "to", 2, 2)?;
    let Some(f) = format_arg(&args, 1)? else {
        return arg_err(&args, "`to` needs a format, like to(value, \"json\")");
    };
    let fmt = check_format(&f, &args, &[])?;
    let mut out: Vec<u8> = Vec::new();
    let value = args.pos[0].clone();
    serialize(interp, value, fmt, &opts, &args, &mut out, false)?;
    if fmt == "bytes" {
        return Ok(Value::Bytes(out.into()));
    }
    match String::from_utf8(out) {
        Ok(s) => Ok(Value::str(s)),
        Err(_) => Err(format_err(
            &args,
            "the result is not valid UTF-8 text".into(),
        )),
    }
}

/// Whether a string looks like the name of a file `write` would write: no
/// spaces or newlines, and an extension of a known format. `write` and the
/// lint in `ok self check` use it to catch swapped arguments (SPEC.md 12.1).
pub(crate) fn looks_like_file_name(s: &str) -> bool {
    let known = [
        "json", "jsonl", "ndjson", "csv", "tsv", "txt", "log", "md", "toml", "yaml", "yml", "env",
        "gz",
    ];
    !s.contains(['\n', ' '])
        && known.contains(&OkPath::parse(s).ext().to_ascii_lowercase().as_str())
}

fn write(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(
        &mut args,
        "write",
        &["append", "compact", "delimiter", "header", "gzip"],
    )?;
    count_args(&args, "write", 2, 3)?;
    let span = args.span;
    let p = match &args.pos[1] {
        Value::Lazy(_) => {
            let v = lazy::force(interp, args.pos[1].clone(), span)?;
            args.pos[1] = v;
            path_arg(&args, 1, "write")?
        }
        _ => path_arg(&args, 1, "write")?,
    };
    if let Value::Str(s) = &args.pos[0]
        && looks_like_file_name(s)
        && p.ext().is_empty()
    {
        // `write("out.txt", text)` with the arguments swapped would write
        // a file named after the text; refuse the likely mistake.
        return Err(OkError::at(
            ErrKind::Arg,
            span,
            format!(
                "write(value, path) writes {} to a file named {}",
                Value::str(s.as_ref()).repr(),
                p.display()
            ),
        )
        .with_help("the value comes first and the path second: write(text, \"out.txt\")")
        .into());
    }
    let fmt = match format_arg(&args, 2)? {
        Some(f) => check_format(&f, &args, &[])?,
        None => format_for(&p),
    };
    let append = bool_named(&opts, "append", &args)?;
    let gzip = use_gzip(&opts, &p, &args)?;
    if !interp.mutation(&format!(
        "write {}{}",
        p.display(),
        if append { " (append)" } else { "" }
    )) {
        return Ok(Value::Null);
    }
    if append && !matches!(fmt, "text" | "lines" | "jsonl" | "csv" | "tsv" | "bytes") {
        return arg_err(
            &args,
            format!("`append` works with text, lines, jsonl, csv, tsv and bytes, not {fmt}"),
        );
    }
    let native = p.native();
    if let Some(parent) = native.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|e| {
            OkError::at(
                ErrKind::Io,
                span,
                format!(
                    "cannot create the directory {}: {}",
                    OkPath::from_native(parent).display(),
                    io_reason(&e)
                ),
            )
        })?;
    }
    let existing_nonempty = append
        && std::fs::metadata(&native)
            .map(|m| m.len() > 0)
            .unwrap_or(false);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .append(append)
        .truncate(!append)
        .open(&native)
        .map_err(|e| {
            OkError::at(
                ErrKind::Io,
                span,
                format!("cannot write {}: {}", p.display(), io_reason(&e)),
            )
        })?;
    let out = std::io::BufWriter::new(file);
    let value = std::mem::replace(&mut args.pos[0], Value::Null);
    let write_err = |e: std::io::Error| {
        OkError::at(
            ErrKind::Io,
            span,
            format!("cannot write {}: {}", p.display(), io_reason(&e)),
        )
    };
    if gzip {
        // A fixed header (no name, no time, OS "unknown") and a fixed level
        // make the compressed bytes identical on every OS (SPEC.md 13.1.1).
        let mut enc = flate2::GzBuilder::new()
            .operating_system(255)
            .write(out, flate2::Compression::new(6));
        serialize(
            interp,
            value,
            fmt,
            &opts,
            &args,
            &mut enc,
            existing_nonempty,
        )?;
        enc.finish()
            .and_then(|mut w| w.flush())
            .map_err(write_err)?;
    } else {
        let mut out = out;
        serialize(
            interp,
            value,
            fmt,
            &opts,
            &args,
            &mut out,
            existing_nonempty,
        )?;
        out.flush().map_err(write_err)?;
    }
    Ok(Value::Null)
}

fn stdin(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "stdin", &["delimiter", "header"])?;
    count_args(&args, "stdin", 0, 1)?;
    let fmt = match format_arg(&args, 0)? {
        Some(f) => check_format(&f, &args, &[])?,
        None => "text",
    };
    let name = "stdin".to_string();
    let reader: Box<dyn BufRead> = Box::new(BufReader::new(std::io::stdin()));
    match fmt {
        "text" | "lines" | "jsonl" => Ok(lazy_lines(
            fmt,
            Box::new(ReaderLines::new(reader, name.clone())),
            name,
        )),
        "csv" | "tsv" => {
            let t = table_opts(&opts, fmt, &args)?;
            Ok(Lazy::list(Box::new(CsvSource::new(reader, name, t))))
        }
        other => {
            let mut buf = Vec::new();
            let mut r = reader;
            r.read_to_end(&mut buf)
                .map_err(|e| format_err(&args, format!("cannot read stdin: {e}")))?;
            if other == "bytes" {
                return Ok(Value::Bytes(buf.into()));
            }
            let text = String::from_utf8(buf)
                .map_err(|_| format_err(&args, "stdin is not valid UTF-8 text".into()))?;
            let _ = interp;
            parse_document(other, &text, &name).map_err(|m| format_err(&args, m))
        }
    }
}

// ----- serialisation ------------------------------------------------------------

fn io_err(span: Span, e: std::io::Error) -> crate::interp::Flow {
    OkError::at(
        ErrKind::Io,
        span,
        format!("cannot write output: {}", io_reason(&e)),
    )
    .into()
}

/// Write `value` in `fmt` to `out`, streaming lazy lists and text.
fn serialize(
    interp: &mut Interp,
    value: Value,
    fmt: &str,
    opts: &HashMap<String, Value>,
    args: &Args,
    out: &mut dyn Write,
    appending: bool,
) -> R<()> {
    let span = args.span;
    match fmt {
        "text" => match value {
            Value::Str(s) => out.write_all(s.as_bytes()).map_err(|e| io_err(span, e)),
            Value::Lazy(l) if l.text => {
                let mut src = lazy::text_cursor(interp, Value::Lazy(l), span)?;
                while let Some(chunk) = at_span(src.next(interp), span)? {
                    out.write_all(chunk.display().as_bytes())
                        .map_err(|e| io_err(span, e))?;
                }
                Ok(())
            }
            v @ (Value::List(_) | Value::Lazy(_)) => write_lines(interp, v, args, out),
            other => type_err(
                args,
                format!(
                    "text output needs a string or a list of strings, got {}",
                    other.a_type()
                ),
            ),
        },
        "lines" => write_lines(interp, value, args, out),
        "jsonl" => {
            let mut src = lazy::cursor(interp, value, span, "JSON Lines output")?;
            while let Some(v) = at_span(src.next(interp), span)? {
                let mut s = String::new();
                to_json(&v, &mut s, None, 0).map_err(|m| format_err(args, m))?;
                s.push('\n');
                out.write_all(s.as_bytes()).map_err(|e| io_err(span, e))?;
            }
            Ok(())
        }
        "csv" | "tsv" => {
            let t = table_opts(opts, fmt, args)?;
            write_table(interp, value, t, args, out, appending)
        }
        "bytes" => match value {
            Value::Bytes(b) => out.write_all(&b).map_err(|e| io_err(span, e)),
            Value::Str(s) => out.write_all(s.as_bytes()).map_err(|e| io_err(span, e)),
            other => type_err(
                args,
                format!(
                    "bytes output needs bytes or a string, got {}",
                    other.a_type()
                ),
            ),
        },
        doc => {
            let value = lazy::force(interp, value, span)?;
            let compact = bool_named(opts, "compact", args)?;
            let text = match doc {
                "json" => {
                    let mut s = String::new();
                    to_json(&value, &mut s, if compact { None } else { Some(2) }, 0)
                        .map_err(|m| format_err(args, m))?;
                    s.push('\n');
                    s
                }
                "toml" => to_toml(&value).map_err(|m| format_err(args, m))?,
                "yaml" => to_yaml(&value).map_err(|m| format_err(args, m))?,
                "env" => to_env(&value).map_err(|m| format_err(args, m))?,
                _ => unreachable_format(doc),
            };
            out.write_all(text.as_bytes()).map_err(|e| io_err(span, e))
        }
    }
}

fn unreachable_format(f: &str) -> String {
    format!("internal error: no writer for {f}")
}

fn write_lines(interp: &mut Interp, value: Value, args: &Args, out: &mut dyn Write) -> R<()> {
    let span = args.span;
    let mut src = lazy::cursor(interp, value, span, "line output")?;
    while let Some(v) = at_span(src.next(interp), span)? {
        if matches!(
            v,
            Value::List(_) | Value::Record(_) | Value::Bytes(_) | Value::Func(_)
        ) {
            return type_err(
                args,
                format!("each line must be text or a number, got {}", v.a_type()),
            );
        }
        let mut line = v.display();
        line.push('\n');
        out.write_all(line.as_bytes())
            .map_err(|e| io_err(span, e))?;
    }
    Ok(())
}

// ----- JSON -----------------------------------------------------------------------

fn from_json(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => match n.as_i64() {
            Some(i) => Value::Int(i),
            None => Value::Float(n.as_f64().unwrap_or(0.0)),
        },
        serde_json::Value::String(s) => Value::str(s),
        serde_json::Value::Array(items) => Value::list(items.into_iter().map(from_json).collect()),
        serde_json::Value::Object(map) => {
            let mut r = Record::new();
            for (k, v) in map {
                r.insert(k.as_str(), from_json(v));
            }
            Value::record(r)
        }
    }
}

fn json_string(s: &str, out: &mut String) {
    out.push_str(&serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()));
}

/// Serialise as JSON. `indent: None` is compact.
pub fn to_json(
    v: &Value,
    out: &mut String,
    indent: Option<usize>,
    level: usize,
) -> Result<(), String> {
    let nl = |out: &mut String, level: usize| {
        if let Some(n) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(n * level));
        }
    };
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Int(i) => {
            let _ = write!(out, "{i}");
        }
        Value::Float(f) => out.push_str(&format_float(*f)),
        Value::Str(s) => json_string(s, out),
        Value::Path(p) => json_string(&p.display(), out),
        Value::Duration(d) => json_string(&format_duration(*d), out),
        Value::Time(t) => json_string(&format_time_utc(*t), out),
        Value::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return Ok(());
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                nl(out, level + 1);
                to_json(item, out, indent, level + 1)?;
            }
            nl(out, level);
            out.push(']');
        }
        Value::Record(r) => {
            if r.is_empty() {
                out.push_str("{}");
                return Ok(());
            }
            out.push('{');
            for (i, (k, item)) in r.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                    if indent.is_none() {
                        out.push(' ');
                    }
                }
                nl(out, level + 1);
                json_string(k, out);
                out.push_str(": ");
                to_json(item, out, indent, level + 1)?;
            }
            nl(out, level);
            out.push('}');
        }
        Value::Bytes(_) => return Err("bytes cannot be written as JSON".into()),
        Value::Func(_) => return Err("a function cannot be written as JSON".into()),
        Value::Lazy(_) => return Err("internal error: unmaterialised value in JSON output".into()),
        Value::Env => return Err("the env object cannot be written as JSON; use env.all()".into()),
    }
    Ok(())
}

// ----- documents ------------------------------------------------------------------

/// Parse a whole-document format. Errors are messages; the caller adds the span.
pub fn parse_document(fmt: &str, text: &str, name: &str) -> Result<Value, String> {
    match fmt {
        "json" => serde_json::from_str::<serde_json::Value>(text)
            .map(from_json)
            .map_err(|e| format!("invalid JSON in {name}: {e}")),
        "toml" => toml::from_str::<toml::Table>(text)
            .map(|t| from_toml(toml::Value::Table(t)))
            .map_err(|e| format!("invalid TOML in {name}: {}", e.message())),
        "yaml" => {
            let mut docs = parse_yaml(text, name)?;
            match docs.len() {
                0 => Ok(Value::Null),
                1 => Ok(docs.remove(0)),
                n => Err(format!(
                    "{name} holds {n} YAML documents; read them all with parse(text, \"yaml_all\")"
                )),
            }
        }
        "yaml_all" => Ok(Value::list(parse_yaml(text, name)?)),
        "env" => parse_env(text, name),
        other => Err(format!("internal error: {other} is not a document format")),
    }
}

fn from_toml(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::str(s),
        toml::Value::Integer(i) => Value::Int(i),
        toml::Value::Float(f) => Value::Float(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(dt) => match (dt.date, dt.time, dt.offset) {
            (Some(d), Some(t), Some(off)) => {
                let days = days_from_civil(d.year as i64, d.month as u32, d.day as u32);
                let offset_min = match off {
                    toml::value::Offset::Z => 0,
                    toml::value::Offset::Custom { minutes } => minutes as i64,
                };
                let secs = days * 86_400
                    + t.hour as i64 * 3600
                    + t.minute as i64 * 60
                    + t.second.unwrap_or(0) as i64
                    - offset_min * 60;
                Value::Time(secs * 1_000_000_000 + t.nanosecond.unwrap_or(0) as i64)
            }
            _ => Value::str(dt.to_string()),
        },
        toml::Value::Array(items) => Value::list(items.into_iter().map(from_toml).collect()),
        toml::Value::Table(t) => {
            let mut r = Record::new();
            for (k, v) in t {
                r.insert(k.as_str(), from_toml(v));
            }
            Value::record(r)
        }
    }
}

fn into_toml(v: &Value, at: &str) -> Result<toml::Value, String> {
    Ok(match v {
        Value::Null => {
            return Err(format!(
                "TOML has no null (at {at}); leave the field out instead"
            ));
        }
        Value::Bool(b) => toml::Value::Boolean(*b),
        Value::Int(i) => toml::Value::Integer(*i),
        Value::Float(f) => toml::Value::Float(*f),
        Value::Str(s) => toml::Value::String(s.to_string()),
        Value::Path(p) => toml::Value::String(p.display()),
        Value::Duration(d) => toml::Value::String(format_duration(*d)),
        Value::Time(t) => toml::Value::Datetime(
            format_time_utc(*t)
                .parse()
                .map_err(|_| format!("cannot write the time at {at} as TOML"))?,
        ),
        Value::List(items) => {
            let mut arr = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                arr.push(into_toml(item, &format!("{at}[{i}]"))?);
            }
            toml::Value::Array(arr)
        }
        Value::Record(r) => {
            let mut t = toml::Table::new();
            for (k, item) in r.iter() {
                t.insert(
                    k.to_string(),
                    into_toml(
                        item,
                        &if at.is_empty() {
                            k.to_string()
                        } else {
                            format!("{at}.{k}")
                        },
                    )?,
                );
            }
            toml::Value::Table(t)
        }
        other => {
            return Err(format!(
                "{} cannot be written as TOML (at {at})",
                other.a_type()
            ));
        }
    })
}

fn to_toml(v: &Value) -> Result<String, String> {
    let Value::Record(_) = v else {
        return Err(format!(
            "a TOML document must be a record, got {}",
            v.a_type()
        ));
    };
    match into_toml(v, "")? {
        toml::Value::Table(t) => toml::to_string(&t).map_err(|e| format!("cannot write TOML: {e}")),
        _ => Err("internal error: TOML root".into()),
    }
}

/// The most values that aliases may add to one YAML text (SPEC.md 13.6).
const YAML_ALIAS_LIMIT: u64 = 1_000_000;

/// Counts, from the parser's events, how many values the aliases in a YAML
/// text would add once resolved, without building them. `YamlLoader` copies
/// an anchor's whole value at every alias, so aliases that repeat each other
/// ("billion laughs") turn a 1 KB file into billions of values and exhaust
/// memory; this pass lets `parse_yaml` refuse such a file first.
#[derive(Default)]
struct AliasCounter {
    /// Size in values of each anchor seen so far, by the parser's anchor id.
    anchors: HashMap<usize, u64>,
    /// Open sequences and mappings: (anchor id, values inside so far).
    open: Vec<(usize, u64)>,
    /// Values added by aliases in the whole text.
    added: u64,
}

impl AliasCounter {
    /// Account for a finished value of `size` values (itself included).
    fn finish(&mut self, anchor: usize, size: u64) {
        if anchor > 0 {
            self.anchors.insert(anchor, size);
        }
        if let Some((_, count)) = self.open.last_mut() {
            *count = count.saturating_add(size);
        }
    }
}

impl yaml_rust2::parser::EventReceiver for AliasCounter {
    fn on_event(&mut self, ev: yaml_rust2::parser::Event) {
        use yaml_rust2::parser::Event;
        match ev {
            Event::Scalar(_, _, anchor, _) => self.finish(anchor, 1),
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                self.open.push((anchor, 1));
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((anchor, size)) = self.open.pop() {
                    self.finish(anchor, size);
                }
            }
            Event::Alias(id) => {
                let size = self.anchors.get(&id).copied().unwrap_or(1);
                self.added = self.added.saturating_add(size);
                self.finish(0, size);
            }
            _ => {}
        }
    }
}

fn parse_yaml(text: &str, name: &str) -> Result<Vec<Value>, String> {
    let mut counter = AliasCounter::default();
    yaml_rust2::parser::Parser::new_from_str(text)
        .load(&mut counter, true)
        .map_err(|e| format!("invalid YAML in {name}: {e}"))?;
    if counter.added > YAML_ALIAS_LIMIT {
        return Err(format!(
            "invalid YAML in {name}: its aliases expand to more than {YAML_ALIAS_LIMIT} values"
        ));
    }
    let docs = yaml_rust2::YamlLoader::load_from_str(text)
        .map_err(|e| format!("invalid YAML in {name}: {e}"))?;
    docs.into_iter().map(|d| from_yaml(d, name)).collect()
}

fn from_yaml(y: yaml_rust2::Yaml, name: &str) -> Result<Value, String> {
    use yaml_rust2::Yaml;
    Ok(match y {
        Yaml::Null => Value::Null,
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Integer(i) => Value::Int(i),
        Yaml::Real(s) => {
            let f: f64 = s
                .parse()
                .map_err(|_| format!("YAML number {s} in {name} is not a finite number"))?;
            if !f.is_finite() {
                return Err(format!("YAML number {s} in {name} is not a finite number"));
            }
            Value::Float(f)
        }
        Yaml::String(s) => Value::str(s),
        Yaml::Array(items) => Value::list(
            items
                .into_iter()
                .map(|i| from_yaml(i, name))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Hash(h) => {
            let mut r = Record::new();
            let mut merged: Vec<Record> = Vec::new();
            for (k, v) in h {
                // Merge keys: `<<: *anchor` or `<<: [*a, *b]` (SPEC.md 13.6).
                if matches!(&k, Yaml::String(s) if s == "<<") {
                    let sources = match v {
                        Yaml::Array(items) => items,
                        other => vec![other],
                    };
                    for src in sources {
                        match from_yaml(src, name)? {
                            Value::Record(m) => merged.push((*m).clone()),
                            _ => {
                                return Err(format!(
                                    "a `<<` merge key in {name} must refer to a mapping"
                                ));
                            }
                        }
                    }
                    continue;
                }
                let key = match k {
                    Yaml::String(s) => s,
                    Yaml::Integer(i) => i.to_string(),
                    Yaml::Real(s) => s,
                    Yaml::Boolean(b) => b.to_string(),
                    Yaml::Null => "null".to_string(),
                    _ => {
                        return Err(format!(
                            "{name} uses a list or mapping as a key, which ok records cannot hold"
                        ));
                    }
                };
                r.insert(key.as_str(), from_yaml(v, name)?);
            }
            for m in merged {
                for (k, v) in m.iter() {
                    if !r.contains(k) {
                        r.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::record(r)
        }
        Yaml::Alias(_) | Yaml::BadValue => {
            return Err(format!("{name} refers to an unknown YAML anchor"));
        }
    })
}

fn into_yaml(v: &Value) -> Result<yaml_rust2::Yaml, String> {
    use yaml_rust2::Yaml;
    Ok(match v {
        Value::Null => Yaml::Null,
        Value::Bool(b) => Yaml::Boolean(*b),
        Value::Int(i) => Yaml::Integer(*i),
        Value::Float(f) => Yaml::Real(format_float(*f)),
        Value::Str(s) => Yaml::String(s.to_string()),
        Value::Path(p) => Yaml::String(p.display()),
        Value::Duration(d) => Yaml::String(format_duration(*d)),
        Value::Time(t) => Yaml::String(format_time_utc(*t)),
        Value::List(items) => Yaml::Array(items.iter().map(into_yaml).collect::<Result<_, _>>()?),
        Value::Record(r) => {
            let mut h = yaml_rust2::yaml::Hash::new();
            for (k, item) in r.iter() {
                h.insert(Yaml::String(k.to_string()), into_yaml(item)?);
            }
            Yaml::Hash(h)
        }
        other => return Err(format!("{} cannot be written as YAML", other.a_type())),
    })
}

fn to_yaml(v: &Value) -> Result<String, String> {
    let y = into_yaml(v)?;
    let mut s = String::new();
    yaml_rust2::YamlEmitter::new(&mut s)
        .dump(&y)
        .map_err(|e| format!("cannot write YAML: {e:?}"))?;
    let body = s
        .strip_prefix("---\n")
        .or_else(|| s.strip_prefix("---"))
        .unwrap_or(&s)
        .trim_start_matches(' ');
    Ok(format!("{body}\n"))
}

// ----- .env -----------------------------------------------------------------------

fn parse_env(text: &str, name: &str) -> Result<Value, String> {
    let mut r = Record::new();
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    let mut i = 0;
    while i < lines.len() {
        let lineno = i + 1;
        let line = lines[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line
            .strip_prefix("export ")
            .map(str::trim_start)
            .unwrap_or(line);
        let Some((key, rest)) = line.split_once('=') else {
            return Err(format!("line {lineno} of {name} is not KEY=value"));
        };
        let key = key.trim();
        let valid = key
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
        if !valid {
            return Err(format!(
                "line {lineno} of {name} has an invalid variable name `{key}`"
            ));
        }
        let rest = rest.trim_start();
        let value = if let Some(body) = rest.strip_prefix('"') {
            // Double-quoted: escapes, may continue over following lines.
            let mut acc = body.to_string();
            loop {
                if let Some(v) = unescape_until_quote(&acc) {
                    break v;
                }
                if i >= lines.len() {
                    return Err(format!(
                        "line {lineno} of {name} has an unterminated double-quoted value"
                    ));
                }
                acc.push('\n');
                acc.push_str(lines[i]);
                i += 1;
            }
        } else if let Some(body) = rest.strip_prefix('\'') {
            match body.find('\'') {
                Some(end) => body[..end].to_string(),
                None => {
                    return Err(format!(
                        "line {lineno} of {name} has an unterminated single-quoted value"
                    ));
                }
            }
        } else {
            let v = match rest.find(" #") {
                Some(c) => &rest[..c],
                None => rest,
            };
            v.trim_end().to_string()
        };
        r.insert(key, Value::str(value));
    }
    Ok(Value::record(r))
}

/// The value of a double-quoted `.env` string up to its closing quote, or
/// `None` if the quote is not in `s` yet.
fn unescape_until_quote(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some(other) => out.push(other),
                None => return None,
            },
            c => out.push(c),
        }
    }
    None
}

fn to_env(v: &Value) -> Result<String, String> {
    let Value::Record(r) = v else {
        return Err(format!(
            ".env output needs a record of variables, got {}",
            v.a_type()
        ));
    };
    let mut out = String::new();
    for (k, item) in r.iter() {
        let text = match item {
            Value::Str(s) => s.to_string(),
            Value::Null => String::new(),
            Value::Int(_)
            | Value::Float(_)
            | Value::Bool(_)
            | Value::Path(_)
            | Value::Duration(_) => item.display(),
            other => {
                return Err(format!(
                    "variable {k} is {}; .env values must be text or numbers",
                    other.a_type()
                ));
            }
        };
        let needs_quotes = text.is_empty()
            || text
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '#' | '"' | '\'' | '\\' | '$'));
        if text.is_empty() {
            let _ = writeln!(out, "{k}=");
        } else if needs_quotes {
            let escaped = text
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t");
            let _ = writeln!(out, "{k}=\"{escaped}\"");
        } else {
            let _ = writeln!(out, "{k}={text}");
        }
    }
    Ok(out)
}

// ----- JSON Lines -------------------------------------------------------------------

struct JsonLines {
    up: Box<dyn Source>,
    name: String,
    line: usize,
}

impl Source for JsonLines {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        loop {
            let Some(chunk) = self.up.next(interp)? else {
                return Ok(None);
            };
            self.line += 1;
            let text = chunk.display();
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            return serde_json::from_str::<serde_json::Value>(text)
                .map(|v| Some(from_json(v)))
                .map_err(|e| {
                    OkError::new(
                        ErrKind::Format,
                        format!("invalid JSON on line {} of {}: {e}", self.line, self.name),
                    )
                    .into()
                });
        }
    }
}

// ----- CSV and TSV ------------------------------------------------------------------

struct CsvSource {
    rdr: csv::Reader<Box<dyn Read>>,
    header: Option<Vec<Rc<str>>>,
    use_header: bool,
    name: String,
}

impl CsvSource {
    fn new(input: Box<dyn Read>, name: String, t: Table) -> CsvSource {
        let rdr = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .delimiter(t.delimiter)
            .quoting(!t.tsv)
            .from_reader(input);
        CsvSource {
            rdr,
            header: None,
            use_header: t.header,
            name,
        }
    }

    fn error(&self, e: csv::Error) -> crate::interp::Flow {
        let line = e.position().map(|p| p.line());
        let msg = match e.kind() {
            csv::ErrorKind::Utf8 { .. } => format!(
                "{} is not valid UTF-8 text{}",
                self.name,
                line.map(|l| format!(" (line {l})")).unwrap_or_default()
            ),
            _ => format!("invalid CSV in {}: {e}", self.name),
        };
        OkError::new(ErrKind::Format, msg).into()
    }
}

impl Source for CsvSource {
    fn next(&mut self, _: &mut Interp) -> R<Option<Value>> {
        loop {
            let mut rec = csv::StringRecord::new();
            match self.rdr.read_record(&mut rec) {
                Ok(false) => return Ok(None),
                Err(e) => return Err(self.error(e)),
                Ok(true) => {}
            }
            let line = rec.position().map(|p| p.line()).unwrap_or(0);
            if rec.len() == 1 && rec[0].is_empty() {
                continue;
            }
            if !self.use_header {
                return Ok(Some(Value::list(rec.iter().map(Value::str).collect())));
            }
            let Some(header) = &self.header else {
                let mut h: Vec<Rc<str>> = rec.iter().map(Rc::from).collect();
                if let Some(first) = h.first_mut()
                    && let Some(stripped) = first.strip_prefix('\u{feff}')
                {
                    *first = Rc::from(stripped);
                }
                self.header = Some(h);
                continue;
            };
            if rec.len() != header.len() {
                return Err(OkError::new(
                    ErrKind::Format,
                    format!(
                        "line {line} of {} has {} fields, but the header has {}",
                        self.name,
                        rec.len(),
                        header.len()
                    ),
                )
                .into());
            }
            let mut r = Record::new();
            for (k, v) in header.iter().zip(rec.iter()) {
                r.insert(k.clone(), Value::str(v));
            }
            return Ok(Some(Value::record(r)));
        }
    }
}

fn csv_cell(v: &Value, tsv: bool) -> Result<String, String> {
    let s = match v {
        Value::Str(s) => s.to_string(),
        Value::Null => String::new(),
        Value::Int(_)
        | Value::Float(_)
        | Value::Bool(_)
        | Value::Path(_)
        | Value::Duration(_)
        | Value::Time(_) => v.display(),
        other => {
            return Err(format!(
                "{} cannot be a {} field",
                other.a_type(),
                if tsv { "TSV" } else { "CSV" }
            ));
        }
    };
    if tsv && s.contains(['\t', '\n', '\r']) {
        return Err(format!(
            "the TSV value {} contains a tab or line break",
            Value::str(s.as_str()).repr()
        ));
    }
    Ok(s)
}

fn write_table(
    interp: &mut Interp,
    value: Value,
    t: Table,
    args: &Args,
    out: &mut dyn Write,
    appending: bool,
) -> R<()> {
    let span = args.span;
    let quote = if t.tsv {
        csv::QuoteStyle::Never
    } else {
        csv::QuoteStyle::Necessary
    };
    let mut w = csv::WriterBuilder::new()
        .delimiter(t.delimiter)
        .terminator(csv::Terminator::Any(b'\n'))
        .quote_style(quote)
        .flexible(!t.header)
        .from_writer(out);
    let mut src = lazy::cursor(interp, value, span, "CSV output")?;
    let mut header: Option<Vec<Rc<str>>> = None;
    let mut row = 0usize;
    let fail = |m: String| -> crate::interp::Flow { OkError::at(ErrKind::Format, span, m).into() };
    while let Some(v) = at_span(src.next(interp), span)? {
        row += 1;
        let fields: Vec<String> = match (&v, t.header) {
            (Value::Record(r), true) => {
                if header.is_none() {
                    let h: Vec<Rc<str>> = r.keys().cloned().collect();
                    if !appending {
                        w.write_record(h.iter().map(|k| k.as_bytes()))
                            .map_err(|e| fail(e.to_string()))?;
                    }
                    header = Some(h);
                }
                let h = header.as_ref().expect("header set");
                if let Some(extra) = r.keys().find(|k| !h.contains(k)) {
                    return Err(fail(format!(
                        "row {row} has the field `{extra}`, which the first row (the header) does not have"
                    )));
                }
                let mut fields = Vec::with_capacity(h.len());
                for k in h {
                    fields.push(csv_cell(r.get(k).unwrap_or(&Value::Null), t.tsv).map_err(&fail)?);
                }
                fields
            }
            (Value::List(items), false) => items
                .iter()
                .map(|x| csv_cell(x, t.tsv))
                .collect::<Result<_, _>>()
                .map_err(&fail)?,
            (Value::List(_), true) => {
                return Err(OkError::at(
                    ErrKind::Type,
                    span,
                    "rows are lists; write them with header: false",
                )
                .into());
            }
            (other, _) => {
                return type_err(
                    args,
                    format!("CSV rows must be records, got {}", other.a_type()),
                );
            }
        };
        w.write_record(&fields).map_err(|e| fail(e.to_string()))?;
    }
    w.flush().map_err(|e| io_err(span, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> String {
        let mut i = Interp::new(vec![]);
        match i.run_source("<test>", src, std::env::temp_dir()) {
            Ok(v) => v.display(),
            Err(crate::interp::Flow::Err(e)) => format!("error: {}", e.message),
            Err(_) => "flow".into(),
        }
    }

    #[test]
    fn csv_quotes_newlines_bom_and_crlf() {
        let src = "parse(\"\\u{feff}name,note\\r\\n\\\"Visser, Anna\\\",\\\"two\\nlines\\\"\\r\\n\\r\\nbo,\\\"say \\\"\\\"hi\\\"\\\"\\\"\\r\\n\", \"csv\")";
        assert_eq!(
            run(src),
            r#"[{name: "Visser, Anna", note: "two\nlines"}, {name: "bo", note: "say \"hi\""}]"#
        );
    }

    #[test]
    fn csv_values_stay_strings() {
        assert_eq!(
            run("parse(\"n,ok\\n1,true\\n\", \"csv\")"),
            r#"[{n: "1", ok: "true"}]"#
        );
    }

    #[test]
    fn csv_field_count_mismatch_names_the_line() {
        assert!(
            run("parse(\"a,b\\n1,2\\n3\\n\", \"csv\")").contains("line 3 of the text has 1 fields")
        );
    }

    #[test]
    fn csv_round_trip_quotes_only_when_needed() {
        assert_eq!(
            run("to([{a: \"x,y\", b: 1, c: null}, {a: \"plain\", b: 2}], \"csv\")"),
            "a,b,c\n\"x,y\",1,\nplain,2,\n"
        );
    }

    #[test]
    fn tsv_has_no_quoting() {
        assert_eq!(
            run("parse(\"a\\tb\\n\\\"x\\\"\\t2\\n\", \"tsv\")"),
            r#"[{a: "\"x\"", b: "2"}]"#
        );
        assert!(run("to([{a: \"x\\ty\"}], \"tsv\")").contains("contains a tab"));
    }

    #[test]
    fn json_round_trip_keeps_order_and_ints() {
        assert_eq!(
            run(r#"parse('{"b": 1, "a": [1.5, null, 2e3, 99999999999999999999]}', "json")"#),
            "{b: 1, a: [1.5, null, 2000.0, 100000000000000000000.0]}"
        );
        assert_eq!(
            run("to({b: 1, a: [true]}, \"json\")"),
            "{\n  \"b\": 1,\n  \"a\": [\n    true\n  ]\n}\n"
        );
        assert_eq!(
            run("to({b: 1, a: []}, \"json\", compact: true)"),
            "{\"b\": 1, \"a\": []}\n"
        );
    }

    #[test]
    fn yaml_alias_expansion_is_limited() {
        // Nine levels of nine aliases each: 9^9 values once resolved. Without
        // the limit this exhausts memory; with it, it fails before building.
        let mut lines = vec!["a: &a [x, x, x, x, x, x, x, x, x]".to_string()];
        for (prev, cur) in "abcdefgh".chars().zip("bcdefghi".chars()) {
            lines.push(format!(
                "{cur}: &{cur} [{}]",
                vec![format!("*{prev}"); 9].join(", ")
            ));
        }
        let text = lines.join("\\n");
        let started = std::time::Instant::now();
        let v = run(&format!("parse(\"{text}\", \"yaml\")"));
        assert_eq!(
            v,
            "error: invalid YAML in the text: its aliases expand to more than 1000000 values"
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
        // Ordinary aliases and merge keys still resolve.
        assert_eq!(
            run(r#"parse("d: &d\n  a: 1\nx: *d\ny:\n  <<: *d\n  b: 2", "yaml")"#),
            "{d: {a: 1}, x: {a: 1}, y: {b: 2, a: 1}}"
        );
    }

    #[test]
    fn env_files() {
        let v = run(
            "parse(\"# c\\nexport A=1\\nB = two words # note\\nC=\\\"x\\\\ny\\\"\\nD='$HOME'\\n\", \"env\")",
        );
        assert_eq!(v, r#"{A: "1", B: "two words", C: "x\ny", D: "$HOME"}"#);
        assert_eq!(
            run("to({A: \"1\", B: \"two words\", C: \"\"}, \"env\")"),
            "A=1\nB=\"two words\"\nC=\n"
        );
    }

    #[test]
    fn yaml_core_schema() {
        assert_eq!(
            run("parse(\"a: yes\\nb: true\\nc: ~\\nd: 1.5\\n\", \"yaml\")"),
            r#"{a: "yes", b: true, c: null, d: 1.5}"#
        );
        assert!(run("parse(\"a: 1\\n---\\nb: 2\\n\", \"yaml\")").contains("yaml_all"));
    }

    #[test]
    fn toml_datetimes() {
        assert_eq!(
            run("type(parse(\"t = 2026-11-24T10:00:00Z\\nd = 2026-11-24\\n\", \"toml\").t)"),
            "time"
        );
        assert_eq!(
            run("parse(\"d = 2026-11-24\\n\", \"toml\").d"),
            "2026-11-24"
        );
    }

    #[test]
    fn format_detection() {
        let f = |s: &str| format_for(&OkPath::parse(s));
        assert_eq!(f("a.JSON"), "json");
        assert_eq!(f("x/.env"), "env");
        assert_eq!(f(".env.production"), "env");
        assert_eq!(f("data.ndjson"), "jsonl");
        assert_eq!(f("notes"), "text");
        assert_eq!(f("c.yml"), "yaml");
        // Compressed files: the name without `.gz` decides (SPEC.md 13.1.1).
        assert_eq!(f("events.jsonl.gz"), "jsonl");
        assert_eq!(f("a/b.CSV.GZ"), "csv");
        assert_eq!(f(".env.gz"), "env");
        assert_eq!(f("data.gz"), "text");
        assert!(gz_name(&OkPath::parse("x.json.Gz")));
        assert!(!gz_name(&OkPath::parse("x.tgz")));
    }
}
