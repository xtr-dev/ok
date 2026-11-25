//! Running commands (SPEC.md section 9).
//!
//! Commands are spawned directly from an argument vector, never through a
//! shell. Where a `run` expression stands decides what happens to its
//! output ([`Mode`]): an expression statement streams live to the terminal,
//! the left side of `|` streams stdout into the pipeline, and anywhere else
//! captures stdout and stderr into the result record.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command as Proc, ExitStatus, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::ast::{Arg, CmdPart, CmdWord, Command, Expr};
use crate::error::{ErrKind, OkError, suggest};
use crate::interp::{Interp, R, Scope, io_reason};
use crate::lazy::{self, Lazy, Source};
use crate::lexer::RedirectKind;
use crate::path::OkPath;
use crate::source::Span;
use crate::value::{Record, Value};

/// Where a `run` expression stands (SPEC.md 9.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// An expression statement: output goes to the terminal as it happens.
    Live,
    /// The value is used: stdout and stderr are captured.
    Captured,
    /// Left of `|`: stdout streams into the pipeline, stderr is live.
    Piped,
}

/// Lines of stderr kept in a command error.
const STDERR_TAIL: usize = 20;

#[derive(Default)]
struct Options {
    cwd: Option<PathBuf>,
    env: Vec<(String, Option<String>)>,
    stdin: Option<Value>,
    timeout: Option<Duration>,
    bytes: bool,
    quiet: bool,
}

/// A command ready to start.
struct Prepared {
    argv: Vec<String>,
    program: PathBuf,
    opts: Options,
    stdout_to: Option<(PathBuf, bool)>,
    stdin_from: Option<PathBuf>,
    bang: bool,
    span: Span,
}

fn cmd_err(span: Span, msg: impl Into<String>) -> OkError {
    OkError::at(ErrKind::Command, span, msg)
}

/// Convert an interpolated value to one argument (SPEC.md 9.2). `alone` is
/// true when the value is the whole word, in which case a path keeps its
/// native form.
fn to_arg(v: &Value, alone: bool, span: Span) -> R<String> {
    Ok(match v {
        Value::Str(s) => s.to_string(),
        Value::Path(p) if alone => p.native().to_string_lossy().into_owned(),
        Value::Path(p) => p.display(),
        Value::Int(_) | Value::Float(_) | Value::Bool(_) => v.display(),
        Value::List(_) => {
            return Err(OkError::at(ErrKind::Type, span, "a list cannot be one command argument")
                .with_help("spread it into several arguments with {...list}, or join it: {join(list, \",\")}")
                .into());
        }
        Value::Null => {
            return Err(OkError::at(ErrKind::Type, span, "null cannot be a command argument")
                .with_help("give a default with ?? or leave the argument out with {...if x != null { [x] } else { [] }}")
                .into());
        }
        other => {
            return Err(OkError::at(
                ErrKind::Type,
                span,
                format!("{} cannot be a command argument", other.a_type()),
            )
            .into());
        }
    })
}

impl Interp {
    fn word_args(&mut self, w: &CmdWord, scope: &Rc<Scope>, out: &mut Vec<String>) -> R<()> {
        if let [CmdPart::Spread(e)] = w.parts.as_slice() {
            match self.eval(e, scope)? {
                Value::List(items) => {
                    for item in items.iter() {
                        out.push(to_arg(item, true, e.span)?);
                    }
                    return Ok(());
                }
                other => {
                    return Err(OkError::at(
                        ErrKind::Type,
                        e.span,
                        format!(
                            "can only spread a list into arguments, got {}",
                            other.a_type()
                        ),
                    )
                    .into());
                }
            }
        }
        let alone = w.parts.len() == 1;
        let mut s = String::new();
        for part in &w.parts {
            match part {
                CmdPart::Lit(t) => s.push_str(t),
                CmdPart::Interp { expr, spec } => {
                    let v = self.eval(expr, scope)?;
                    match spec {
                        Some(spec) => {
                            let parsed = crate::format::parse_spec(spec)
                                .map_err(|m| OkError::at(ErrKind::Format, expr.span, m))?;
                            s.push_str(
                                &crate::format::apply(&v, &parsed)
                                    .map_err(|m| OkError::at(ErrKind::Format, expr.span, m))?,
                            );
                        }
                        None => s.push_str(&to_arg(&v, alone, expr.span)?),
                    }
                }
                CmdPart::Spread(e) => {
                    return Err(OkError::at(
                        ErrKind::Syntax,
                        e.span,
                        "a spread `{...list}` must be a whole word",
                    )
                    .into());
                }
            }
        }
        out.push(s);
        Ok(())
    }

    fn redirect_target(&mut self, w: &CmdWord, scope: &Rc<Scope>) -> R<PathBuf> {
        if let [CmdPart::Interp { expr, spec: None }] = w.parts.as_slice() {
            return match self.eval(expr, scope)? {
                Value::Path(p) => Ok(p.native()),
                Value::Str(s) => Ok(OkPath::parse(&s).native()),
                other => Err(OkError::at(
                    ErrKind::Type,
                    expr.span,
                    format!(
                        "a redirect target must be a path or string, got {}",
                        other.a_type()
                    ),
                )
                .into()),
            };
        }
        let mut parts = Vec::new();
        self.word_args(w, scope, &mut parts)?;
        Ok(OkPath::parse(&parts.concat()).native())
    }

    /// Evaluate a command-mode `run` (SPEC.md 9.2).
    pub fn run_command(
        &mut self,
        cmd: &Command,
        span: Span,
        mode: Mode,
        stdin: Option<Value>,
        scope: &Rc<Scope>,
    ) -> R<Value> {
        let mut argv = Vec::new();
        for w in &cmd.words {
            self.word_args(w, scope, &mut argv)?;
        }
        let mut opts = match &cmd.options {
            Some(e) => {
                let v = self.eval(e, scope)?;
                self.options_from(&v, e.span)?
            }
            None => Options::default(),
        };
        let mut stdout_to = None;
        let mut stdin_from = None;
        for (kind, w) in &cmd.redirects {
            let target = self.redirect_target(w, scope)?;
            match kind {
                RedirectKind::Out => stdout_to = Some((target, false)),
                RedirectKind::Append => stdout_to = Some((target, true)),
                RedirectKind::In => stdin_from = Some(target),
            }
        }
        if stdin.is_some() {
            if opts.stdin.is_some() || stdin_from.is_some() {
                return Err(cmd_err(span, "this command gets stdin twice: from the pipeline and from `<` or the stdin option").into());
            }
            opts.stdin = stdin;
        }
        self.start(argv, opts, stdout_to, stdin_from, cmd.bang, span, mode)
    }

    /// Evaluate the function form `run("prog", arg, ..., cwd: ...)`.
    pub fn run_function(
        &mut self,
        args: &[Arg],
        bang: bool,
        span: Span,
        mode: Mode,
        stdin: Option<Value>,
        scope: &Rc<Scope>,
    ) -> R<Value> {
        let mut argv = Vec::new();
        let mut rec = Record::new();
        for a in args {
            match a {
                Arg::Pos(e) => {
                    let v = self.eval(e, scope)?;
                    argv.push(to_arg(&v, true, e.span)?);
                }
                Arg::Spread(e) => match self.eval(e, scope)? {
                    Value::List(items) => {
                        for i in items.iter() {
                            argv.push(to_arg(i, true, e.span)?);
                        }
                    }
                    other => {
                        return Err(OkError::at(
                            ErrKind::Type,
                            e.span,
                            format!(
                                "can only spread a list into arguments, got {}",
                                other.a_type()
                            ),
                        )
                        .into());
                    }
                },
                Arg::Named { name, value, .. } => {
                    let v = self.eval(value, scope)?;
                    rec.insert(name.as_str(), v);
                }
            }
        }
        if argv.is_empty() {
            return Err(cmd_err(span, "`run(...)` needs at least the program name").into());
        }
        if argv.len() == 1 && argv[0].contains(' ') && !Path::new(&argv[0]).exists() {
            let words: Vec<&str> = argv[0].split_whitespace().collect();
            return Err(cmd_err(
                span,
                format!(
                    "no program named \"{}\": ok never splits a string into words",
                    argv[0]
                ),
            )
            .with_help(format!(
                "pass each word separately: run({}) or run {}",
                words
                    .iter()
                    .map(|w| format!("{w:?}"))
                    .collect::<Vec<_>>()
                    .join(", "),
                words.join(" ")
            ))
            .into());
        }
        let mut opts = self.options_from(&Value::record(rec), span)?;
        if stdin.is_some() {
            if opts.stdin.is_some() {
                return Err(cmd_err(
                    span,
                    "this command gets stdin twice: from the pipeline and from the stdin option",
                )
                .into());
            }
            opts.stdin = stdin;
        }
        self.start(argv, opts, None, None, bang, span, mode)
    }

    fn options_from(&mut self, v: &Value, span: Span) -> R<Options> {
        let Value::Record(r) = v else {
            return Err(OkError::at(
                ErrKind::Type,
                span,
                format!("command options must be a record, got {}", v.a_type()),
            )
            .into());
        };
        let mut o = Options::default();
        for (k, val) in r.iter() {
            match (&**k, val) {
                ("cwd", Value::Str(s)) => o.cwd = Some(OkPath::parse(s).native()),
                ("cwd", Value::Path(p)) => o.cwd = Some(p.native()),
                ("env", Value::Record(e)) => {
                    for (name, value) in e.iter() {
                        let value = match value {
                            Value::Null => None,
                            Value::Str(s) => Some(s.to_string()),
                            Value::Int(_) | Value::Float(_) | Value::Bool(_) => {
                                Some(value.display())
                            }
                            Value::Path(p) => Some(p.native().to_string_lossy().into_owned()),
                            other => {
                                return Err(OkError::at(
                                    ErrKind::Type,
                                    span,
                                    format!(
                                        "env value {name} must be a string or null, got {}",
                                        other.a_type()
                                    ),
                                )
                                .into());
                            }
                        };
                        o.env.push((name.to_string(), value));
                    }
                }
                ("stdin", v @ (Value::Str(_) | Value::Bytes(_) | Value::Path(_))) => {
                    o.stdin = Some(v.clone())
                }
                ("timeout", Value::Duration(d)) if *d > 0 => {
                    o.timeout = Some(Duration::from_nanos(*d as u64))
                }
                ("bytes", Value::Bool(b)) => o.bytes = *b,
                ("quiet", Value::Bool(b)) => o.quiet = *b,
                (name @ ("cwd" | "env" | "stdin" | "timeout" | "bytes" | "quiet"), other) => {
                    let want = match name {
                        "cwd" => "a path",
                        "env" => "a record",
                        "stdin" => "a string, bytes or a path",
                        "timeout" => "a positive duration like 30s",
                        _ => "true or false",
                    };
                    return Err(OkError::at(
                        ErrKind::Type,
                        span,
                        format!("the `{name}` option must be {want}, got {}", other.a_type()),
                    )
                    .into());
                }
                (other, _) => {
                    let names = ["cwd", "env", "stdin", "timeout", "bytes", "quiet"];
                    let help = suggest(other, names)
                        .map(|s| format!("did you mean `{s}`?"))
                        .unwrap_or_else(|| format!("the options are {}", names.join(", ")));
                    return Err(OkError::at(
                        ErrKind::Arg,
                        span,
                        format!("unknown command option `{other}`"),
                    )
                    .with_help(help)
                    .into());
                }
            }
        }
        Ok(o)
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        argv: Vec<String>,
        opts: Options,
        stdout_to: Option<(PathBuf, bool)>,
        stdin_from: Option<PathBuf>,
        bang: bool,
        span: Span,
        mode: Mode,
    ) -> R<Value> {
        if mode == Mode::Piped && stdout_to.is_some() {
            return Err(cmd_err(
                span,
                "stdout cannot be redirected with `>` and piped with `|` at the same time",
            )
            .into());
        }
        let line = display_command(&argv);
        let cwd_note = opts
            .cwd
            .as_ref()
            .map(|c| format!("(in {}) ", OkPath::from_native(c).display()))
            .unwrap_or_default();
        let redirect_note = match (&stdout_to, &stdin_from) {
            (Some((p, false)), _) => format!(" > {}", OkPath::from_native(p).display()),
            (Some((p, true)), _) => format!(" >> {}", OkPath::from_native(p).display()),
            (None, Some(p)) => format!(" < {}", OkPath::from_native(p).display()),
            _ => String::new(),
        };
        if self.run_opts.dry_run {
            eprintln!("~ {cwd_note}{line}{redirect_note}");
            return Ok(match mode {
                Mode::Piped => Value::str(""),
                _ => result_record(
                    &argv,
                    0,
                    Some(String::new()),
                    Some(String::new()),
                    Duration::ZERO,
                    false,
                ),
            });
        }
        if self.run_opts.trace {
            eprintln!("+ {cwd_note}{line}{redirect_note}");
        }
        let base_cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let cwd = match &opts.cwd {
            Some(c) => base_cwd.join(c),
            None => base_cwd,
        };
        if !cwd.is_dir() {
            return Err(cmd_err(
                span,
                format!(
                    "the working directory {} does not exist",
                    OkPath::from_native(&cwd).display()
                ),
            )
            .into());
        }
        let path_var = self
            .env_overlay
            .iter()
            .chain(opts.env.iter())
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case("PATH") || k == "PATH")
            .map(|(_, v)| v.clone().map(OsString::from))
            .unwrap_or_else(|| std::env::var_os("PATH"));
        let program = resolve_program(&argv[0], &cwd, path_var.as_deref()).map_err(|msg| {
            let mut e = cmd_err(span, msg);
            e.exit_code = Some(127);
            e.command = Some(argv.clone());
            e.help = Some("check that it is installed and on PATH".into());
            e
        })?;
        let p = Prepared {
            argv,
            program,
            opts,
            stdout_to,
            stdin_from,
            bang,
            span,
        };
        match mode {
            Mode::Piped => self.spawn_piped(p),
            Mode::Live if !p.opts.quiet => self.spawn_live(p),
            _ => self.spawn_captured(p, mode),
        }
    }

    fn base_command(&self, p: &Prepared) -> R<Proc> {
        let mut c = Proc::new(&p.program);
        c.args(&p.argv[1..]);
        if let Some(cwd) = &p.opts.cwd {
            c.current_dir(cwd);
        }
        for (k, v) in self.env_overlay.iter().chain(p.opts.env.iter()) {
            match v {
                Some(v) => {
                    c.env(k, v);
                }
                None => {
                    c.env_remove(k);
                }
            }
        }
        Ok(c)
    }

    /// Set up stdin; returns bytes that a writer thread must feed in.
    fn stdin_setup(&mut self, c: &mut Proc, p: &mut Prepared) -> R<Option<Vec<u8>>> {
        if let Some(path) = &p.stdin_from {
            let f = std::fs::File::open(path).map_err(|e| {
                cmd_err(
                    p.span,
                    format!(
                        "cannot read {}: {}",
                        OkPath::from_native(path).display(),
                        io_reason(&e)
                    ),
                )
            })?;
            c.stdin(f);
            return Ok(None);
        }
        match p.opts.stdin.take() {
            None => Ok(None),
            Some(Value::Path(path)) => {
                let f = std::fs::File::open(path.native()).map_err(|e| {
                    cmd_err(
                        p.span,
                        format!("cannot read {}: {}", path.display(), io_reason(&e)),
                    )
                })?;
                c.stdin(f);
                Ok(None)
            }
            Some(Value::Bytes(b)) => {
                c.stdin(Stdio::piped());
                Ok(Some(b.to_vec()))
            }
            Some(Value::Lazy(l)) if l.text => {
                // A piped command upstream: connect its stdout directly.
                let mut src = lazy::text_cursor(self, Value::Lazy(l), p.span)?;
                if let Some(out) = src.take_child_stdout() {
                    c.stdin(Stdio::from(out));
                    self.upstream.push(src);
                    return Ok(None);
                }
                let mut data = String::new();
                while let Some(chunk) = lazy::at_span(src.next(self), p.span)? {
                    data.push_str(&chunk.display());
                }
                c.stdin(Stdio::piped());
                Ok(Some(data.into_bytes()))
            }
            Some(other) => {
                let other = lazy::force(self, other, p.span)?;
                let data = match &other {
                    Value::Str(s) => s.as_bytes().to_vec(),
                    Value::List(items) => {
                        let mut s = String::new();
                        for i in items.iter() {
                            if matches!(i, Value::List(_) | Value::Record(_)) {
                                return Err(OkError::at(
                                    ErrKind::Type,
                                    p.span,
                                    "stdin from a list needs strings, one per line",
                                )
                                .into());
                            }
                            s.push_str(&i.display());
                            s.push('\n');
                        }
                        s.into_bytes()
                    }
                    other => {
                        return Err(OkError::at(
                            ErrKind::Type,
                            p.span,
                            format!("cannot send {} to a command's stdin", other.a_type()),
                        )
                        .into());
                    }
                };
                c.stdin(Stdio::piped());
                Ok(Some(data))
            }
        }
    }

    fn stdout_file(&self, p: &Prepared) -> R<Option<std::fs::File>> {
        let Some((path, append)) = &p.stdout_to else {
            return Ok(None);
        };
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(*append)
            .truncate(!*append)
            .open(path)
            .map_err(|e| {
                cmd_err(
                    p.span,
                    format!(
                        "cannot write {}: {}",
                        OkPath::from_native(path).display(),
                        io_reason(&e)
                    ),
                )
            })?;
        Ok(Some(f))
    }

    fn spawn(&self, c: &mut Proc, p: &Prepared) -> R<Child> {
        c.spawn().map_err(|e| {
            let mut err = cmd_err(
                p.span,
                format!("cannot start {}: {}", p.argv[0], spawn_reason(&e)),
            );
            err.command = Some(p.argv.clone());
            err.exit_code = Some(126);
            err.into()
        })
    }

    fn spawn_live(&mut self, mut p: Prepared) -> R<Value> {
        let mut c = self.base_command(&p)?;
        let feed = self.stdin_setup(&mut c, &mut p)?;
        if let Some(f) = self.stdout_file(&p)? {
            c.stdout(f);
        }
        let started = Instant::now();
        let mut child = self.spawn(&mut c, &p)?;
        let feeder = feed_stdin(&mut child, feed);
        let status = wait_with_timeout(&mut child, p.opts.timeout);
        if let Some(t) = feeder {
            let _ = t.join();
        }
        self.upstream_done();
        let elapsed = started.elapsed();
        let status = status
            .map_err(|e| cmd_err(p.span, format!("waiting for {} failed: {e}", p.argv[0])))?;
        let Some(status) = status else {
            return Err(timeout_error(&p).into());
        };
        let code = exit_code(&status);
        if code != 0 && !p.bang {
            return Err(failure(&p, code, None).into());
        }
        Ok(result_record(&p.argv, code, None, None, elapsed, false))
    }

    fn spawn_captured(&mut self, mut p: Prepared, mode: Mode) -> R<Value> {
        let mut c = self.base_command(&p)?;
        let feed = self.stdin_setup(&mut c, &mut p)?;
        match self.stdout_file(&p)? {
            Some(f) => {
                c.stdout(f);
            }
            None => {
                c.stdout(Stdio::piped());
            }
        }
        c.stderr(Stdio::piped());
        let started = Instant::now();
        let mut child = self.spawn(&mut c, &p)?;
        let feeder = feed_stdin(&mut child, feed);
        let out_reader = child
            .stdout
            .take()
            .map(|s| std::thread::spawn(move || read_all(s)));
        let err_reader = child
            .stderr
            .take()
            .map(|s| std::thread::spawn(move || read_all(s)));
        let status = wait_with_timeout(&mut child, p.opts.timeout);
        if let Some(t) = feeder {
            let _ = t.join();
        }
        self.upstream_done();
        let out = out_reader
            .map(|t| t.join().unwrap_or_default())
            .unwrap_or_default();
        let err = err_reader
            .map(|t| t.join().unwrap_or_default())
            .unwrap_or_default();
        let elapsed = started.elapsed();
        let status = status
            .map_err(|e| cmd_err(p.span, format!("waiting for {} failed: {e}", p.argv[0])))?;
        let Some(status) = status else {
            return Err(timeout_error(&p).into());
        };
        let code = exit_code(&status);
        if code != 0 && !p.bang {
            return Err(failure(&p, code, Some(&err)).into());
        }
        if p.opts.quiet && mode == Mode::Live {
            return Ok(result_record(&p.argv, code, None, None, elapsed, false));
        }
        if p.opts.bytes {
            let mut r = result_record(&p.argv, code, None, None, elapsed, false);
            if let Value::Record(rec) = &mut r {
                let rec = Rc::make_mut(rec);
                rec.insert("out", Value::Bytes(out.into()));
                rec.insert("err", Value::Bytes(err.into()));
            }
            return Ok(r);
        }
        let to_text = |b: Vec<u8>, which: &str| -> R<String> {
            String::from_utf8(b).map_err(|_| {
                OkError::at(
                    ErrKind::Command,
                    p.span,
                    format!("the {which} of {} is not valid UTF-8 text", p.argv[0]),
                )
                .with_help("capture raw output with the option bytes: true")
                .into()
            })
        };
        let out = to_text(out, "output")?;
        let err = to_text(err, "error output")?;
        Ok(result_record(
            &p.argv,
            code,
            Some(strip_trailing_newlines(&out)),
            Some(strip_trailing_newlines(&err)),
            elapsed,
            false,
        ))
    }

    fn spawn_piped(&mut self, mut p: Prepared) -> R<Value> {
        let mut c = self.base_command(&p)?;
        let feed = self.stdin_setup(&mut c, &mut p)?;
        c.stdout(Stdio::piped());
        if p.opts.quiet {
            c.stderr(Stdio::null());
        }
        let mut child = self.spawn(&mut c, &p)?;
        let feeder = feed_stdin(&mut child, feed);
        let stdout = child.stdout.take();
        let upstream = std::mem::take(&mut self.upstream);
        Ok(Lazy::text(Box::new(ProcessOutput {
            child: Some(child),
            reader: stdout.map(BufReader::new),
            feeder,
            argv: p.argv,
            bang: p.bang,
            span: p.span,
            timeout: p.opts.timeout.map(|t| Instant::now() + t),
            upstream,
        })))
    }

    /// Finish commands whose stdout was connected directly to a downstream
    /// command's stdin.
    fn upstream_done(&mut self) {
        for mut src in std::mem::take(&mut self.upstream) {
            let _ = src.finish();
        }
    }
}

fn feed_stdin(child: &mut Child, data: Option<Vec<u8>>) -> Option<std::thread::JoinHandle<()>> {
    let data = data?;
    let mut stdin = child.stdin.take()?;
    Some(std::thread::spawn(move || {
        let _ = stdin.write_all(&data);
    }))
}

fn read_all(mut r: impl Read) -> Vec<u8> {
    let mut buf = Vec::new();
    let _ = r.read_to_end(&mut buf);
    buf
}

/// Wait for the child; `Ok(None)` means the timeout expired and the child
/// was killed.
fn wait_with_timeout(
    child: &mut Child,
    timeout: Option<Duration>,
) -> std::io::Result<Option<ExitStatus>> {
    let Some(t) = timeout else {
        return child.wait().map(Some);
    };
    let deadline = Instant::now() + t;
    loop {
        if let Some(s) = child.try_wait()? {
            return Ok(Some(s));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The process exit code; on Unix a child killed by signal S gives 128+S.
pub fn exit_code(status: &ExitStatus) -> i32 {
    if let Some(c) = status.code() {
        return c;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    1
}

fn strip_trailing_newlines(s: &str) -> String {
    s.trim_end_matches(['\n', '\r']).to_string()
}

fn result_record(
    argv: &[String],
    code: i32,
    out: Option<String>,
    err: Option<String>,
    elapsed: Duration,
    _: bool,
) -> Value {
    let mut r = Record::new();
    r.insert("out", out.map(Value::str).unwrap_or(Value::Null));
    r.insert("err", err.map(Value::str).unwrap_or(Value::Null));
    r.insert("code", Value::Int(code as i64));
    r.insert("ok", Value::Bool(code == 0));
    r.insert(
        "cmd",
        Value::list(argv.iter().map(|a| Value::str(a.as_str())).collect()),
    );
    r.insert(
        "duration",
        Value::Duration(elapsed.as_nanos().min(i64::MAX as u128) as i64),
    );
    Value::record(r)
}

fn stderr_tail(err: &[u8]) -> String {
    let text = String::from_utf8_lossy(err);
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let start = lines.len().saturating_sub(STDERR_TAIL);
    lines[start..].join("\n")
}

fn failure(p: &Prepared, code: i32, err: Option<&[u8]>) -> OkError {
    let mut e = cmd_err(p.span, format!("command failed with exit code {code}"));
    e.exit_code = Some(code);
    e.command = Some(p.argv.clone());
    e.details
        .push(format!("command: {}", display_command(&p.argv)));
    if let Some(err) = err {
        let tail = stderr_tail(err);
        if !tail.is_empty() {
            let n = tail.lines().count();
            e.details.push(format!(
                "stderr (last {n} line{}):\n{tail}",
                if n == 1 { "" } else { "s" }
            ));
            e.stderr = Some(tail);
        }
    }
    e
}

fn timeout_error(p: &Prepared) -> OkError {
    let t = p.opts.timeout.unwrap_or_default();
    let mut e = OkError::at(
        ErrKind::Timeout,
        p.span,
        format!(
            "command timed out after {}",
            crate::value::format_duration(t.as_nanos() as i64)
        ),
    );
    e.exit_code = Some(124);
    e.command = Some(p.argv.clone());
    e.details
        .push(format!("command: {}", display_command(&p.argv)));
    e
}

fn spawn_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied (is it executable?)".into(),
        std::io::ErrorKind::InvalidInput => "an argument cannot be passed safely to this program (batch files refuse some characters)".into(),
        _ => io_reason(e),
    }
}

/// Show an argument list as it would be typed, quoting where needed.
pub fn display_command(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            let plain = !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,^~".contains(c));
            if plain {
                a.clone()
            } else {
                format!("\"{}\"", a.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Find the program to run (SPEC.md 9.3).
pub fn resolve_program(
    name: &str,
    cwd: &Path,
    path_var: Option<&std::ffi::OsStr>,
) -> Result<PathBuf, String> {
    let has_sep = name.contains('/') || (cfg!(windows) && name.contains('\\'));
    let exts = windows_exts(name);
    if has_sep {
        let base = cwd.join(OkPath::parse(name).native());
        for cand in candidates(&base, &exts) {
            if is_runnable(&cand) {
                return Ok(cand);
            }
        }
        return Err(format!("command not found: {name}"));
    }
    if let Some(paths) = path_var {
        for dir in std::env::split_paths(paths) {
            if dir.as_os_str().is_empty() {
                continue;
            }
            for cand in candidates(&dir.join(name), &exts) {
                if is_runnable(&cand) {
                    return Ok(cand);
                }
            }
        }
    }
    Err(format!("command not found: {name} (searched PATH)"))
}

/// Extensions to try on Windows (PATHEXT); none elsewhere.
fn windows_exts(name: &str) -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let pathext = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let exts: Vec<String> = pathext
        .split(';')
        .filter(|e| !e.is_empty())
        .map(|e| e.to_ascii_lowercase())
        .collect();
    let has_known_ext = Path::new(name).extension().is_some_and(|e| {
        exts.iter()
            .any(|x| x.trim_start_matches('.') == e.to_string_lossy().to_ascii_lowercase())
    });
    if has_known_ext { Vec::new() } else { exts }
}

fn candidates(base: &Path, exts: &[String]) -> Vec<PathBuf> {
    if exts.is_empty() {
        return vec![base.to_path_buf()];
    }
    let mut out = Vec::new();
    for e in exts {
        let mut s = base.as_os_str().to_os_string();
        s.push(e);
        out.push(PathBuf::from(s));
    }
    out
}

fn is_runnable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    // On Unix a file must also have an execute bit; Windows has none, so
    // any regular file found by the PATHEXT search counts as runnable.
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = true;
    meta.is_file() && executable
}

/// The lazy stdout of a piped command.
struct ProcessOutput {
    child: Option<Child>,
    reader: Option<BufReader<ChildStdout>>,
    feeder: Option<std::thread::JoinHandle<()>>,
    argv: Vec<String>,
    bang: bool,
    span: Span,
    timeout: Option<Instant>,
    upstream: Vec<Box<dyn Source>>,
}

impl ProcessOutput {
    fn wait(&mut self) -> R<()> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };
        if let Some(t) = self.feeder.take() {
            let _ = t.join();
        }
        let remaining = self
            .timeout
            .map(|d| d.saturating_duration_since(Instant::now()));
        let status = wait_with_timeout(&mut child, remaining).map_err(|e| {
            cmd_err(
                self.span,
                format!("waiting for {} failed: {e}", self.argv[0]),
            )
        })?;
        for mut up in std::mem::take(&mut self.upstream) {
            up.finish()?;
        }
        let prepared = Prepared {
            argv: self.argv.clone(),
            program: PathBuf::new(),
            opts: Options::default(),
            stdout_to: None,
            stdin_from: None,
            bang: self.bang,
            span: self.span,
        };
        let Some(status) = status else {
            return Err(timeout_error(&prepared).into());
        };
        let code = exit_code(&status);
        if code != 0 && !self.bang {
            return Err(failure(&prepared, code, None).into());
        }
        Ok(())
    }
}

impl Source for ProcessOutput {
    fn next(&mut self, _: &mut Interp) -> R<Option<Value>> {
        let Some(reader) = &mut self.reader else {
            self.wait()?;
            return Ok(None);
        };
        let mut buf = Vec::new();
        let n = reader.read_until(b'\n', &mut buf).map_err(|e| {
            cmd_err(
                self.span,
                format!("cannot read the output of {}: {e}", self.argv[0]),
            )
        })?;
        if n == 0 {
            self.reader = None;
            self.wait()?;
            return Ok(None);
        }
        match String::from_utf8(buf) {
            Ok(s) => Ok(Some(Value::str(s))),
            Err(_) => Err(OkError::at(
                ErrKind::Command,
                self.span,
                format!("the output of {} is not valid UTF-8 text", self.argv[0]),
            )
            .with_help("capture raw output with the option bytes: true")
            .into()),
        }
    }

    fn take_child_stdout(&mut self) -> Option<ChildStdout> {
        self.reader.take().map(BufReader::into_inner)
    }

    fn finish(&mut self) -> R<()> {
        self.reader = None;
        self.wait()
    }
}

impl Drop for ProcessOutput {
    /// A consumer that stopped early (`run yes | take 3`) closes the pipe and
    /// terminates the command; its exit status is ignored (SPEC.md 9.5).
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            self.reader = None;
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Whether `e` is a `run`, a `run(...)`, or a pipeline ending in one.
pub fn is_run(e: &Expr) -> bool {
    use crate::ast::{ExprKind, Stage};
    matches!(
        &e.kind,
        ExprKind::Run(_)
            | ExprKind::RunFn { .. }
            | ExprKind::Pipe {
                stage: Stage::Run(_),
                ..
            }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_only_when_needed() {
        let argv: Vec<String> = [
            "cargo",
            "build",
            "--target=x86",
            "two words",
            "",
            "say \"hi\"",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            display_command(&argv),
            r#"cargo build --target=x86 "two words" "" "say \"hi\"""#
        );
    }

    #[test]
    fn resolution_searches_path_and_never_the_current_directory() {
        let dir = std::env::temp_dir().join(format!("ok-resolve-test-{}", std::process::id()));
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) { "tool.exe" } else { "tool" };
        let tool = bin.join(name);
        std::fs::write(&tool, b"").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let path = std::env::join_paths([bin.clone()]).unwrap();
        assert_eq!(resolve_program("tool", &dir, Some(&path)).unwrap(), tool);
        // Not found when only present in the working directory.
        assert!(resolve_program("tool", &bin, Some(std::ffi::OsStr::new(""))).is_err());
        // A name with a separator is relative to the working directory.
        assert_eq!(
            resolve_program("bin/tool", &dir, None).unwrap(),
            if cfg!(windows) {
                dir.join("bin\\tool.exe")
            } else {
                dir.join("bin/tool")
            }
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert!(
                resolve_program("tool", &dir, Some(&path)).is_err(),
                "non-executable files are skipped"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn windows_finds_cmd_files_through_pathext() {
        let dir = std::env::temp_dir().join(format!("ok-pathext-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("npm.cmd"), b"@echo off\r\necho %*\r\n").unwrap();
        let path = std::env::join_paths([dir.clone()]).unwrap();
        let found = resolve_program("npm", &dir, Some(&path)).unwrap();
        assert!(
            found
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with("npm.cmd")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
