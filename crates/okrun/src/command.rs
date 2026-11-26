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

/// Where a command's stderr goes when it is redirected (SPEC.md 9.5).
#[derive(Clone, Debug, PartialEq)]
enum StderrTo {
    /// `2> path` (false) or `2>> path` (true).
    File(PathBuf, bool),
    /// `2>&1`: the same place as stdout.
    Stdout,
}

/// The redirects of one command.
#[derive(Default)]
struct Redirects {
    stdout_to: Option<(PathBuf, bool)>,
    stdin_from: Option<PathBuf>,
    stderr_to: Option<StderrTo>,
}

impl Redirects {
    /// The redirects as written, for `--trace` and `--dry-run`:
    /// ` < in > out 2> err`.
    fn note(&self) -> String {
        let show = |p: &PathBuf| display_arg(&OkPath::from_native(p).display());
        let mut s = String::new();
        if let Some(p) = &self.stdin_from {
            s.push_str(&format!(" < {}", show(p)));
        }
        match &self.stdout_to {
            Some((p, false)) => s.push_str(&format!(" > {}", show(p))),
            Some((p, true)) => s.push_str(&format!(" >> {}", show(p))),
            None => {}
        }
        match &self.stderr_to {
            Some(StderrTo::File(p, false)) => s.push_str(&format!(" 2> {}", show(p))),
            Some(StderrTo::File(p, true)) => s.push_str(&format!(" 2>> {}", show(p))),
            Some(StderrTo::Stdout) => s.push_str(" 2>&1"),
            None => {}
        }
        s
    }

    /// The file stderr ends up in, if any: `2> file`, or `2>&1` with `> file`.
    fn stderr_file(&self) -> Option<&PathBuf> {
        match &self.stderr_to {
            Some(StderrTo::File(p, _)) => Some(p),
            Some(StderrTo::Stdout) => self.stdout_to.as_ref().map(|(p, _)| p),
            None => None,
        }
    }

    fn merged(&self) -> bool {
        self.stderr_to == Some(StderrTo::Stdout)
    }
}

/// A command ready to start.
struct Prepared {
    argv: Vec<String>,
    program: PathBuf,
    opts: Options,
    redirects: Redirects,
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
        let mut r = Redirects::default();
        for (kind, w) in &cmd.redirects {
            if *kind == RedirectKind::ErrToOut {
                r.stderr_to = Some(StderrTo::Stdout);
                continue;
            }
            let target = self.redirect_target(w, scope)?;
            match kind {
                RedirectKind::Out => r.stdout_to = Some((target, false)),
                RedirectKind::Append => r.stdout_to = Some((target, true)),
                RedirectKind::In => r.stdin_from = Some(target),
                RedirectKind::ErrOut => r.stderr_to = Some(StderrTo::File(target, false)),
                RedirectKind::ErrAppend => r.stderr_to = Some(StderrTo::File(target, true)),
                RedirectKind::ErrToOut => {}
            }
        }
        if stdin.is_some() {
            if opts.stdin.is_some() || r.stdin_from.is_some() {
                return Err(cmd_err(span, "this command gets stdin twice: from the pipeline and from `<` or the stdin option").into());
            }
            opts.stdin = stdin;
        }
        self.start(argv, opts, r, cmd.bang, span, mode)
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
        self.start(argv, opts, Redirects::default(), bang, span, mode)
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

    fn start(
        &mut self,
        argv: Vec<String>,
        opts: Options,
        redirects: Redirects,
        bang: bool,
        span: Span,
        mode: Mode,
    ) -> R<Value> {
        if mode == Mode::Piped && redirects.stdout_to.is_some() {
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
            .map(|c| format!("(in {}) ", display_arg(&OkPath::from_native(c).display())))
            .unwrap_or_default();
        let redirect_note = redirects.note();
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
        // WASI has no way to start a process (SPEC.md 17). Say so plainly
        // instead of the misleading "command not found" a PATH search gives.
        if cfg!(target_os = "wasi") {
            return Err(cmd_err(
                span,
                format!("cannot run `{line}`: the WebAssembly build of ok cannot start programs"),
            )
            .with_help("`run` needs the native ok binary; everything else in the script works here")
            .into());
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
            redirects,
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
        if let Some(path) = &p.redirects.stdin_from {
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
                    c.stdin(out);
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
        match &p.redirects.stdout_to {
            Some((path, append)) => open_target(path, *append, p.span).map(Some),
            None => Ok(None),
        }
    }

    /// Connect stderr for `2>`, `2>>` and `2>&1`. `stdout_file` is the file
    /// opened for `>`; `merged_pipe` is the write end of the pipe stdout
    /// was connected to, when ok reads stdout itself. Returns false when
    /// stderr is not redirected and the caller decides.
    fn stderr_setup(
        &self,
        c: &mut Proc,
        p: &Prepared,
        stdout_file: Option<&std::fs::File>,
        merged_pipe: Option<&std::io::PipeWriter>,
    ) -> R<bool> {
        let io = |e: std::io::Error| cmd_err(p.span, format!("cannot redirect stderr: {e}"));
        match &p.redirects.stderr_to {
            None => Ok(false),
            Some(StderrTo::File(path, append)) => {
                c.stderr(open_target(path, *append, p.span)?);
                Ok(true)
            }
            Some(StderrTo::Stdout) => {
                if let Some(f) = stdout_file {
                    c.stderr(f.try_clone().map_err(io)?);
                } else if let Some(w) = merged_pipe {
                    c.stderr(w.try_clone().map_err(io)?);
                } else {
                    c.stderr(Stdio::from(std::io::stdout()));
                }
                Ok(true)
            }
        }
    }
}

/// Open the file of a `>`, `>>`, `2>` or `2>>` redirect, creating its
/// directory.
fn open_target(path: &Path, append: bool, span: Span) -> R<std::fs::File> {
    {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .map_err(|e| {
                cmd_err(
                    span,
                    format!(
                        "cannot write {}: {}",
                        OkPath::from_native(path).display(),
                        io_reason(&e)
                    ),
                )
                .into()
            })
    }
}

impl Interp {
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
        let out_file = self.stdout_file(&p)?;
        self.stderr_setup(&mut c, &p, out_file.as_ref(), None)?;
        if let Some(f) = out_file {
            c.stdout(f);
        }
        let started = Instant::now();
        let mut child = self.spawn(&mut c, &p)?;
        drop(c);
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
        let out_file = self.stdout_file(&p)?;
        // With `2>&1` and no `>`, stdout and stderr share one pipe so the
        // captured `out` keeps their interleaving.
        let merged_pipe = if p.redirects.merged() && out_file.is_none() {
            Some(new_pipe(p.span)?)
        } else {
            None
        };
        let err_redirected = self.stderr_setup(
            &mut c,
            &p,
            out_file.as_ref(),
            merged_pipe.as_ref().map(|m| &m.1),
        )?;
        if !err_redirected {
            c.stderr(Stdio::piped());
        }
        let mut merged_reader = None;
        match (out_file, merged_pipe) {
            (Some(f), _) => {
                c.stdout(f);
            }
            (None, Some((reader, writer))) => {
                c.stdout(writer);
                merged_reader = Some(reader);
            }
            (None, None) => {
                c.stdout(Stdio::piped());
            }
        }
        let started = Instant::now();
        let mut child = self.spawn(&mut c, &p)?;
        // The command keeps copies of the pipe's write end; dropping it
        // lets the reader see the end of the output once the child exits.
        drop(c);
        let feeder = feed_stdin(&mut child, feed);
        let out_reader = match merged_reader {
            Some(r) => Some(std::thread::spawn(move || read_all(r))),
            None => child
                .stdout
                .take()
                .map(|s| std::thread::spawn(move || read_all(s))),
        };
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
            let tail = if p.redirects.merged() {
                Some(Tail::Output(&out))
            } else if err_redirected {
                None
            } else {
                Some(Tail::Stderr(&err))
            };
            return Err(failure(&p, code, tail).into());
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
        // With `2>&1`, stdout and stderr share one pipe that becomes the
        // pipeline's input.
        let merged_pipe = if p.redirects.merged() {
            Some(new_pipe(p.span)?)
        } else {
            None
        };
        let err_redirected =
            self.stderr_setup(&mut c, &p, None, merged_pipe.as_ref().map(|m| &m.1))?;
        if p.opts.quiet && !err_redirected {
            c.stderr(Stdio::null());
        }
        let merged_reader = match merged_pipe {
            Some((reader, writer)) => {
                c.stdout(writer);
                Some(reader)
            }
            None => {
                c.stdout(Stdio::piped());
                None
            }
        };
        let mut child = self.spawn(&mut c, &p)?;
        drop(c);
        let feeder = feed_stdin(&mut child, feed);
        let stdout = match merged_reader {
            Some(r) => Some(OutPipe::Merged(r)),
            None => child.stdout.take().map(OutPipe::Child),
        };
        let upstream = std::mem::take(&mut self.upstream);
        let stderr_file = p.redirects.stderr_file().cloned();
        Ok(Lazy::text(Box::new(ProcessOutput {
            child: Some(child),
            reader: stdout.map(BufReader::new),
            feeder,
            argv: p.argv,
            bang: p.bang,
            span: p.span,
            timeout: p.opts.timeout.map(|t| Instant::now() + t),
            upstream,
            stderr_file,
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

/// What a failed command's error shows of its output (SPEC.md 9.7).
enum Tail<'a> {
    /// Captured stderr.
    Stderr(&'a [u8]),
    /// Captured stdout and stderr together (`2>&1`).
    Output(&'a [u8]),
}

fn failure(p: &Prepared, code: i32, tail: Option<Tail>) -> OkError {
    let mut e = cmd_err(p.span, format!("command failed with exit code {code}"));
    e.exit_code = Some(code);
    e.command = Some(p.argv.clone());
    e.details
        .push(format!("command: {}", display_command(&p.argv)));
    let (bytes, label) = match tail {
        Some(Tail::Stderr(b)) => (Some(b), "stderr"),
        Some(Tail::Output(b)) => (Some(b), "output"),
        None => (None, ""),
    };
    if let Some(bytes) = bytes {
        let tail = stderr_tail(bytes);
        if !tail.is_empty() {
            let n = tail.lines().count();
            e.details.push(format!(
                "{label} (last {n} line{}):\n{tail}",
                if n == 1 { "" } else { "s" }
            ));
            e.stderr = Some(tail);
        }
    }
    if let Some(f) = p.redirects.stderr_file() {
        e.details.push(format!(
            "stderr was written to {}",
            OkPath::from_native(f).display()
        ));
    }
    e
}

fn new_pipe(span: Span) -> R<(std::io::PipeReader, std::io::PipeWriter)> {
    std::io::pipe().map_err(|e| cmd_err(span, format!("cannot create a pipe: {e}")).into())
}

/// The stdout of a piped command: the child's own pipe, or with `2>&1` a
/// pipe that stdout and stderr share.
enum OutPipe {
    Child(ChildStdout),
    Merged(std::io::PipeReader),
}

impl Read for OutPipe {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            OutPipe::Child(c) => c.read(buf),
            OutPipe::Merged(m) => m.read(buf),
        }
    }
}

impl From<OutPipe> for Stdio {
    fn from(p: OutPipe) -> Stdio {
        match p {
            OutPipe::Child(c) => Stdio::from(c),
            OutPipe::Merged(m) => Stdio::from(m),
        }
    }
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

/// Show an argument list as it would be typed, quoting where needed
/// (SPEC.md 9.8).
pub fn display_command(argv: &[String]) -> String {
    argv.iter()
        .map(|a| display_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One argument (or file-operation operand) as `--trace` shows it: as it
/// is when it is plain, else in double quotes with `\\`, `"` and every
/// character that is unsafe on a terminal escaped, so the line shows
/// exactly what the program gets and cannot rewrite the terminal. The
/// password of a URL (`https://user:secret@host`) is shown as `***`.
pub fn display_arg(a: &str) -> String {
    let a = redact_url(a);
    let plain = !a.is_empty()
        && a.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./=:@%+,^~".contains(c));
    if plain {
        return a.into_owned();
    }
    let mut out = String::with_capacity(a.len() + 2);
    out.push('"');
    for c in a.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            c if crate::error::is_unsafe_char(c) => crate::error::escape_char(c, &mut out),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A URL with the password in its user information replaced by `***`
/// (`https://user:***@host/`), so that traces, dry runs and errors do not
/// leak credentials into logs. Text that is not such a URL is unchanged.
pub fn redact_url(s: &str) -> std::borrow::Cow<'_, str> {
    let Some(scheme_end) = s.find("://") else {
        return s.into();
    };
    let scheme = &s[..scheme_end];
    if scheme.is_empty()
        || !scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
    {
        return s.into();
    }
    let auth_start = scheme_end + 3;
    let auth_end = s[auth_start..]
        .find(['/', '?', '#'])
        .map_or(s.len(), |i| auth_start + i);
    let authority = &s[auth_start..auth_end];
    let Some(at) = authority.rfind('@') else {
        return s.into();
    };
    let Some(colon) = authority[..at].find(':') else {
        return s.into();
    };
    format!(
        "{}{}:***{}",
        &s[..auth_start],
        &authority[..colon],
        &s[auth_start + at..]
    )
    .into()
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
    reader: Option<BufReader<OutPipe>>,
    feeder: Option<std::thread::JoinHandle<()>>,
    argv: Vec<String>,
    bang: bool,
    span: Span,
    timeout: Option<Instant>,
    upstream: Vec<Box<dyn Source>>,
    /// The file stderr was redirected to, named in a failure.
    stderr_file: Option<PathBuf>,
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
            redirects: Redirects {
                stderr_to: self.stderr_file.clone().map(|f| StderrTo::File(f, false)),
                ..Redirects::default()
            },
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

    fn take_child_stdout(&mut self) -> Option<Stdio> {
        self.reader.take().map(|r| Stdio::from(r.into_inner()))
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
    fn traced_arguments_escape_control_characters_and_hide_passwords() {
        let argv: Vec<String> = [
            "echo",
            "a\x1b]52;c;eA==\x07",
            "x\ry",
            "tab\there",
            "\u{202e}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            display_command(&argv),
            r#"echo "a\u{1b}]52;c;eA==\u{7}" "x\ry" "tab\there" "\u{202e}""#
        );
        assert_eq!(
            display_arg("https://bot:s3cr3t@example.com/x?y=1"),
            r#""https://bot:***@example.com/x?y=1""#
        );
        assert_eq!(
            display_arg("https://bot@example.com/a"),
            "https://bot@example.com/a"
        );
        assert_eq!(redact_url("https://h/p?u=a:b@c"), "https://h/p?u=a:b@c");
        assert_eq!(redact_url("user:pw@host"), "user:pw@host");
        assert_eq!(display_arg("my file.txt"), r#""my file.txt""#);
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
