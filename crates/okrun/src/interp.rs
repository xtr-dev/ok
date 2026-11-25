//! The tree-walking evaluator (SPEC.md sections 5 to 8 and 11).
//!
//! Scopes are reference-counted chains of hash maps, so closures capture
//! their defining scope by reference (SPEC.md 5.10). Control flow that leaves
//! an expression early (errors, `return`, `break`, `continue`, `exit`) travels
//! as the `Err` side of [`R`], which lets `?` operators in Rust do the
//! plumbing.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use crate::ast::*;
use crate::builtins;
use crate::error::{ErrKind, Frame, MAX_TRACE, OkError, suggest};
use crate::format;
use crate::lazy;
use crate::parser;
use crate::path::OkPath;
use crate::sig;
use crate::source::{SourceMap, Span};
use crate::value::{Func, Record, Value, compare, equal};

/// Maximum depth of nested ok function calls.
const MAX_CALL_DEPTH: usize = 2000;

/// Ways evaluation can leave the normal path.
pub enum Flow {
    Err(Box<OkError>),
    Return(Value),
    Break,
    Continue,
    /// `exit(code)`.
    Exit(i32),
}

impl From<OkError> for Flow {
    fn from(e: OkError) -> Flow {
        Flow::Err(Box::new(e))
    }
}

pub type R<T> = Result<T, Flow>;

fn err<T>(kind: ErrKind, span: Span, msg: impl Into<String>) -> R<T> {
    Err(OkError::at(kind, span, msg).into())
}

// ----- scopes -----------------------------------------------------------------

#[derive(Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

pub struct Scope {
    vars: RefCell<HashMap<Rc<str>, Binding>>,
    parent: Option<Rc<Scope>>,
}

impl Scope {
    pub fn new(parent: Option<Rc<Scope>>) -> Rc<Scope> {
        Rc::new(Scope {
            vars: RefCell::new(HashMap::new()),
            parent,
        })
    }

    pub fn lookup(&self, name: &str) -> Option<Value> {
        if let Some(b) = self.vars.borrow().get(name) {
            return Some(b.value.clone());
        }
        self.parent.as_ref().and_then(|p| p.lookup(name))
    }

    /// Declare a name in this scope. Returns false if it already exists here.
    pub fn declare(&self, name: &str, value: Value, mutable: bool) -> bool {
        let mut vars = self.vars.borrow_mut();
        if vars.contains_key(name) {
            return false;
        }
        vars.insert(name.into(), Binding { value, mutable });
        true
    }

    pub fn has_own(&self, name: &str) -> bool {
        self.vars.borrow().contains_key(name)
    }

    /// The scope in the chain that declares `name`.
    fn owner(self: &Rc<Scope>, name: &str) -> Option<Rc<Scope>> {
        if self.vars.borrow().contains_key(name) {
            return Some(self.clone());
        }
        self.parent.as_ref().and_then(|p| p.owner(name))
    }

    /// Every visible name, for "did you mean" suggestions.
    fn all_names(&self, out: &mut Vec<String>) {
        out.extend(self.vars.borrow().keys().map(|k| k.to_string()));
        if let Some(p) = &self.parent {
            p.all_names(out);
        }
    }
}

// ----- builtin calling convention -------------------------------------------

/// Arguments of a call after evaluation.
pub struct Args {
    pub pos: Vec<Value>,
    pub named: Vec<(String, Value, Span)>,
    /// The span of the whole call, for errors.
    pub span: Span,
}

pub struct Builtin {
    pub name: &'static str,
    pub f: fn(&mut Interp, Args) -> R<Value>,
    /// Whether the builtin accepts lazy (streaming) arguments. All others
    /// receive their arguments materialised.
    pub lazy: bool,
}

// ----- the interpreter -----------------------------------------------------------

pub struct Interp {
    pub sources: SourceMap,
    globals: Rc<Scope>,
    /// Exports of each loaded library, by canonical path.
    modules: HashMap<PathBuf, Value>,
    /// Libraries being loaded, for cycle detection.
    loading: Vec<(PathBuf, String)>,
    depth: usize,
    /// Caught error records, so `fail(e)` can re-raise the original error.
    caught: Vec<(Weak<Record>, OkError)>,
    /// The script's command-line arguments, parsed against `main`.
    pub script_argv: Vec<String>,
    pub run_opts: RunOpts,
    /// Temporary files and directories to remove when the run ends.
    pub temps: Vec<PathBuf>,
    /// Commands whose stdout was connected straight into the command being
    /// started; finished after it.
    pub(crate) upstream: Vec<Box<dyn lazy::Source>>,
    /// Where the user ran `ok` (SPEC.md 12.2).
    pub invocation_dir: PathBuf,
    /// The project root in project mode; `remove` refuses to delete it.
    pub project_root: Option<PathBuf>,
    /// Environment changes made by the script (`env.set`, `env.remove`,
    /// `env file`): `None` removes a variable. Applied to every child
    /// process, so ok never mutates its own process environment.
    pub env_overlay: Vec<(String, Option<String>)>,
}

/// Global options that change how commands and file operations behave.
#[derive(Clone, Copy, Default)]
pub struct RunOpts {
    pub trace: bool,
    pub dry_run: bool,
    pub yes: bool,
    /// This process is a worker running one task in parallel with others
    /// (SPEC.md 15.5): it has no terminal for questions.
    pub parallel: bool,
}

/// What kind of file is being loaded; decides which top-level items are
/// allowed (SPEC.md 4.1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileKind {
    Script,
    Library,
    /// A project file (SPEC.md 15.2); `root` for the root `ok.ok`.
    Project {
        root: bool,
    },
}

impl Interp {
    pub fn new(script_args: Vec<String>) -> Interp {
        let globals = Scope::new(None);
        let script_argv = script_args.clone();
        builtins::install(&globals, script_args);
        Interp {
            sources: SourceMap::default(),
            globals,
            modules: HashMap::new(),
            loading: Vec::new(),
            depth: 0,
            caught: Vec::new(),
            script_argv,
            run_opts: RunOpts::default(),
            temps: Vec::new(),
            upstream: Vec::new(),
            invocation_dir: std::env::current_dir().unwrap_or_default(),
            project_root: None,
            env_overlay: Vec::new(),
        }
    }

    /// Replace a global value (used by project mode for `cwd` and
    /// `invocation_dir`).
    pub fn set_global(&mut self, name: &str, value: Value) {
        self.globals.vars.borrow_mut().insert(
            name.into(),
            Binding {
                value,
                mutable: false,
            },
        );
    }

    /// The top-level scope of one file: a child of a scope that holds the
    /// file's `script_dir`, so user code may still declare that name.
    pub(crate) fn file_scope(&self, dir: &Path) -> Rc<Scope> {
        let abs = if dir.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_default()
        } else {
            std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
        };
        let holder = Scope::new(Some(self.globals.clone()));
        holder.declare("script_dir", Value::path(OkPath::from_native(&abs)), false);
        Scope::new(Some(holder))
    }

    /// The value of an environment variable, after the script's changes.
    /// Names are case-insensitive on Windows.
    pub fn env_var(&self, name: &str) -> Option<String> {
        let same = |a: &str| {
            if cfg!(windows) {
                a.eq_ignore_ascii_case(name)
            } else {
                a == name
            }
        };
        if let Some((_, v)) = self.env_overlay.iter().rev().find(|(k, _)| same(k)) {
            return v.clone();
        }
        std::env::var(name).ok()
    }

    pub fn set_env(&mut self, name: &str, value: Option<String>) {
        let same = |a: &str| {
            if cfg!(windows) {
                a.eq_ignore_ascii_case(name)
            } else {
                a == name
            }
        };
        self.env_overlay.retain(|(k, _)| !same(k));
        self.env_overlay.push((name.to_string(), value));
    }

    /// Remove temporary files and directories created with `tempdir()` and
    /// `tempfile()`. Called when a run ends, also after an error.
    pub fn cleanup(&mut self) {
        for p in std::mem::take(&mut self.temps).into_iter().rev() {
            if p.is_dir() {
                let _ = std::fs::remove_dir_all(&p);
            } else {
                let _ = std::fs::remove_file(&p);
            }
        }
    }

    /// Announce a file-system or network change for `--trace` and
    /// `--dry-run`. Returns false when the change must be skipped.
    pub fn mutation(&self, what: &str) -> bool {
        if self.run_opts.dry_run {
            eprintln!("~ {what}");
            return false;
        }
        if self.run_opts.trace {
            eprintln!("+ {what}");
        }
        true
    }

    /// Evaluate a `run` expression, or a pipeline ending in one, in `mode`.
    fn eval_run(&mut self, e: &Expr, scope: &Rc<Scope>, mode: crate::command::Mode) -> R<Value> {
        use crate::command::{Mode, is_run};
        match &e.kind {
            ExprKind::Run(cmd) => self.run_command(cmd, e.span, mode, None, scope),
            ExprKind::RunFn { bang, args } => {
                self.run_function(args, *bang, e.span, mode, None, scope)
            }
            ExprKind::Pipe {
                lhs,
                stage: Stage::Run(inner),
            } => {
                let left = if is_run(lhs) {
                    self.eval_run(lhs, scope, Mode::Piped)?
                } else {
                    self.eval_lazy(lhs, scope)?
                };
                match &inner.kind {
                    ExprKind::Run(cmd) => {
                        self.run_command(cmd, inner.span, mode, Some(left), scope)
                    }
                    ExprKind::RunFn { bang, args } => {
                        self.run_function(args, *bang, inner.span, mode, Some(left), scope)
                    }
                    _ => Err(OkError::at(ErrKind::Syntax, inner.span, "expected a command").into()),
                }
            }
            _ => self.eval_lazy(e, scope),
        }
    }

    /// Run a script file. `name` is how errors refer to it.
    pub fn run_file(&mut self, name: &str, path: &Path) -> R<Value> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!("cannot read {name}: {}", io_reason(&e)),
            )
        })?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let file = self
            .sources
            .add(name.to_string(), Some(path.to_path_buf()), text.into());
        self.run_program(file, dir)
    }

    /// Run source text (`ok -e`). Imports resolve against `base_dir`.
    pub fn run_source(&mut self, name: &str, text: &str, base_dir: PathBuf) -> R<Value> {
        let file = self.sources.add(name.to_string(), None, text.into());
        self.run_program(file, base_dir)
    }

    fn run_program(&mut self, file: u32, dir: PathBuf) -> R<Value> {
        let text = self.sources.file(file).text.clone();
        let prog = parser::parse(file, &text)?;
        check_items(&prog, FileKind::Script)?;
        let main = prog.stmts.iter().find_map(|s| match &s.kind {
            StmtKind::Fn(d) if d.name == "main" => Some(d.clone()),
            _ => None,
        });
        let name = self.sources.file(file).name.clone();
        let usage = if name == "<-e>" {
            "ok -e SOURCE".to_string()
        } else {
            format!("ok {name}")
        };
        let argv = self.script_argv.clone();
        // Arguments are checked before any code runs (SPEC.md 14).
        let parsed = match &main {
            Some(d) => {
                let target = sig::Target {
                    usage,
                    doc: d.doc.as_deref(),
                    params: &d.params,
                };
                match sig::parse(&target, &argv, &self.invocation_dir, false)? {
                    sig::Outcome::Help(_) => {
                        print!("{}", sig::help(&target, &self.sources));
                        return Err(Flow::Exit(0));
                    }
                    sig::Outcome::Run(p) => Some(p),
                }
            }
            None => {
                let wants_help = argv
                    .iter()
                    .take_while(|a| *a != "--")
                    .any(|a| a == "-h" || a == "--help");
                if wants_help {
                    if let Some(doc) = &prog.doc {
                        println!("{}\n", doc.trim_end());
                    }
                    println!("Usage: {usage} [ARGS...]");
                    return Err(Flow::Exit(0));
                }
                None
            }
        };
        let scope = self.file_scope(&dir);
        let last = self.exec_file(&prog, &scope, &dir, file)?;
        if let (Some(d), Some(p)) = (main, parsed) {
            let Some(Value::Func(f)) = scope.lookup("main") else {
                return Ok(last);
            };
            let Func::User { env, .. } = &*f else {
                return Ok(last);
            };
            let span = d.name_span;
            let named = p.named.into_iter().map(|(n, v)| (n, v, span)).collect();
            self.call_user(
                &d,
                env,
                Args {
                    pos: p.pos,
                    named,
                    span,
                },
            )?;
            return Ok(Value::Null);
        }
        Ok(last)
    }

    /// Hoist functions and imports, then run the statements of a file.
    /// Returns the value of the last statement if it is an expression.
    pub(crate) fn exec_file(
        &mut self,
        prog: &Program,
        scope: &Rc<Scope>,
        dir: &Path,
        file: u32,
    ) -> R<Value> {
        self.hoist(&prog.stmts, scope)?;
        for stmt in &prog.stmts {
            if let StmtKind::Import {
                path,
                path_span,
                alias,
            } = &stmt.kind
            {
                self.import(path, *path_span, alias.as_ref(), scope, dir, file)?;
            }
        }
        let mut last = Value::Null;
        for stmt in &prog.stmts {
            if matches!(stmt.kind, StmtKind::Import { .. } | StmtKind::Fn(_)) {
                continue;
            }
            let v = self.exec_stmt(stmt, scope)?;
            // A live `run` already showed its output; `ok -e` prints nothing more.
            last = match &stmt.kind {
                StmtKind::Expr(e) if !crate::command::is_run(e) => v,
                _ => Value::Null,
            };
        }
        Ok(last)
    }

    pub(crate) fn hoist(&mut self, stmts: &[Stmt], scope: &Rc<Scope>) -> R<()> {
        for stmt in stmts {
            if let StmtKind::Fn(decl) = &stmt.kind {
                let f = Value::Func(Rc::new(Func::User {
                    decl: decl.clone(),
                    env: scope.clone(),
                }));
                if !scope.declare(&decl.name, f, false) {
                    return err(
                        ErrKind::Name,
                        decl.name_span,
                        format!("`{}` is already declared in this scope", decl.name),
                    );
                }
            }
        }
        Ok(())
    }

    // ----- modules ----------------------------------------------------------------

    fn import(
        &mut self,
        raw: &str,
        span: Span,
        alias: Option<&(String, Span)>,
        scope: &Rc<Scope>,
        dir: &Path,
        file: u32,
    ) -> R<()> {
        if raw.contains("://") {
            return err(
                ErrKind::Name,
                span,
                "imports cannot be URLs: ok has no remote imports",
            );
        }
        let as_path = OkPath::parse(raw);
        if as_path.is_abs() || raw.starts_with('/') || raw.starts_with('\\') {
            return err(ErrKind::Name, span, "imports must be relative paths").map_err(|f| {
                with_help(f, "write the path relative to this file, like \"./lib.ok\"")
            });
        }
        if as_path.name() == "ok.ok" && self.project_root.is_some() {
            // Mounts are handled by the project loader (SPEC.md 15.8).
            return Ok(());
        }
        if as_path.name() == "ok.ok" {
            return err(
                ErrKind::Name,
                span,
                "importing an ok.ok file mounts a sub-project, which only a project's root ok.ok can do",
            );
        }
        let native = dir.join(as_path.native());
        let full = std::fs::canonicalize(&native).map_err(|e| {
            OkError::at(
                ErrKind::Io,
                span,
                format!("cannot import \"{raw}\": {}", io_reason(&e)),
            )
        })?;
        if let Some(root) = &self.project_root {
            // Imports may not leave the project, whether by `..` or a
            // symlink (SPEC.md 16.6).
            let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
            if !full.starts_with(&root) {
                return Err(OkError::at(
                    ErrKind::Trust,
                    span,
                    format!("\"{raw}\" is outside the project"),
                )
                .with_note(format!(
                    "it resolves to {}",
                    OkPath::from_native(&full).display()
                ))
                .with_help("project files may only import files inside the project root")
                .into());
            }
        }
        let importer = self.sources.file(file).name.clone();
        let display = OkPath::parse(&importer).parent().join(&as_path).display();
        if let Some(i) = self.loading.iter().position(|(p, _)| *p == full) {
            let mut chain: Vec<String> = self.loading[i..].iter().map(|(_, n)| n.clone()).collect();
            chain.push(display.clone());
            return err(
                ErrKind::Name,
                span,
                format!("import cycle: {}", chain.join(" -> ")),
            );
        }
        if !self.modules.contains_key(&full) {
            let exports = self.load_library(&full, &display, span)?;
            self.modules.insert(full.clone(), exports);
        }
        let exports = self.modules[&full].clone();
        match alias {
            Some((name, name_span)) => {
                if !scope.declare(name, exports, false) {
                    return err(
                        ErrKind::Name,
                        *name_span,
                        format!("`{name}` is already declared in this file"),
                    );
                }
            }
            None => {
                let Value::Record(r) = &exports else {
                    return Ok(());
                };
                for (k, v) in r.iter() {
                    if !scope.declare(k, v.clone(), false) {
                        return Err(OkError::at(
                            ErrKind::Name,
                            span,
                            format!(
                                "`{k}` from {display} clashes with a name already declared here"
                            ),
                        )
                        .with_help(format!(
                            "import it under a name instead: import \"{raw}\" as lib"
                        ))
                        .into());
                    }
                }
            }
        }
        Ok(())
    }

    fn load_library(&mut self, full: &Path, display: &str, span: Span) -> R<Value> {
        let text = std::fs::read_to_string(full).map_err(|e| {
            OkError::at(
                ErrKind::Io,
                span,
                format!("cannot read {display}: {}", io_reason(&e)),
            )
        })?;
        let file = self.sources.add(
            display.to_string(),
            Some(full.to_path_buf()),
            text.clone().into(),
        );
        let prog = parser::parse(file, &text)?;
        check_items(&prog, FileKind::Library)?;
        let dir = full.parent().map(Path::to_path_buf).unwrap_or_default();
        let scope = self.file_scope(&dir);
        self.loading.push((full.to_path_buf(), display.to_string()));
        let result = self.exec_file(&prog, &scope, &dir, file);
        self.loading.pop();
        result?;
        let mut exports = Record::new();
        for stmt in &prog.stmts {
            let mut names = Vec::new();
            match &stmt.kind {
                StmtKind::Fn(d) => names.push((d.name.clone(), d.name_span)),
                StmtKind::Let { pattern, .. } => pattern.names(&mut names),
                _ => {}
            }
            for (n, _) in names {
                if !n.starts_with('_')
                    && let Some(v) = scope.lookup(&n)
                {
                    exports.insert(n.as_str(), v);
                }
            }
        }
        Ok(Value::record(exports))
    }

    // ----- statements ---------------------------------------------------------

    fn exec_block(&mut self, block: &Block, parent: &Rc<Scope>) -> R<Value> {
        let scope = Scope::new(Some(parent.clone()));
        self.exec_stmts(&block.stmts, &scope)
    }

    fn exec_stmts(&mut self, stmts: &[Stmt], scope: &Rc<Scope>) -> R<Value> {
        self.hoist(stmts, scope)?;
        let mut last = Value::Null;
        for stmt in stmts {
            let v = self.exec_stmt(stmt, scope)?;
            last = if matches!(stmt.kind, StmtKind::Expr(_)) {
                v
            } else {
                Value::Null
            };
        }
        Ok(last)
    }

    fn exec_stmt(&mut self, stmt: &Stmt, scope: &Rc<Scope>) -> R<Value> {
        match &stmt.kind {
            StmtKind::Expr(e) if crate::command::is_run(e) => {
                self.eval_run(e, scope, crate::command::Mode::Live)
            }
            StmtKind::Expr(e) => self.eval(e, scope),
            StmtKind::Let {
                pattern,
                value,
                mutable,
            } => {
                let v = self.eval(value, scope)?;
                self.bind(pattern, v, *mutable, scope)?;
                Ok(Value::Null)
            }
            StmtKind::Assign { target, op, value } => {
                self.assign(target, *op, value, scope)?;
                Ok(Value::Null)
            }
            StmtKind::For {
                first,
                second,
                iter,
                body,
            } => {
                self.exec_for(first, second.as_ref(), iter, body, scope)?;
                Ok(Value::Null)
            }
            StmtKind::While { cond, body } => {
                loop {
                    let c = self.eval(cond, scope)?;
                    if !self.truth(&c, cond.span, "`while`")? {
                        break;
                    }
                    match self.exec_block(body, scope) {
                        Ok(_) | Err(Flow::Continue) => {}
                        Err(Flow::Break) => break,
                        Err(other) => return Err(other),
                    }
                }
                Ok(Value::Null)
            }
            StmtKind::Break => Err(Flow::Break),
            StmtKind::Continue => Err(Flow::Continue),
            StmtKind::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e, scope)?,
                    None => Value::Null,
                };
                Err(Flow::Return(v))
            }
            StmtKind::Fn(_)
            | StmtKind::Import { .. }
            | StmtKind::Task(_)
            | StmtKind::Requires { .. }
            | StmtKind::EnvFile { .. } => Ok(Value::Null),
        }
    }

    fn truth(&self, v: &Value, span: Span, what: &str) -> R<bool> {
        match v {
            Value::Bool(b) => Ok(*b),
            other => {
                let help = match other {
                    Value::List(_) => "to test for an empty list use `len(xs) > 0` or `xs != []`",
                    Value::Str(_) => "to test for an empty string use `s != \"\"`",
                    Value::Null => "to test for null use `x != null`",
                    Value::Int(_) | Value::Float(_) => {
                        "compare explicitly, like `n > 0` or `n != 0`"
                    }
                    Value::Record(_) => "to test for an empty record use `len(r) > 0`",
                    _ => "use a comparison that gives true or false",
                };
                Err(OkError::at(
                    ErrKind::Type,
                    span,
                    format!("{what} needs a bool, got {}", other.a_type()),
                )
                .with_help(help)
                .into())
            }
        }
    }

    fn bind(&mut self, pattern: &Pattern, v: Value, mutable: bool, scope: &Rc<Scope>) -> R<()> {
        match pattern {
            Pattern::Name(name, span) => {
                if !scope.declare(name, v, mutable) {
                    return Err(OkError::at(
                        ErrKind::Name,
                        *span,
                        format!("`{name}` is already declared in this scope"),
                    )
                    .with_help(format!(
                        "use a different name, or assign with `{name} = ...` if it is a var"
                    ))
                    .into());
                }
                Ok(())
            }
            Pattern::List { items, rest, span } => {
                let Value::List(list) = &v else {
                    return err(
                        ErrKind::Type,
                        *span,
                        format!("cannot destructure {} with a list pattern", v.a_type()),
                    );
                };
                let ok_len = if rest.is_some() {
                    list.len() >= items.len()
                } else {
                    list.len() == items.len()
                };
                if !ok_len {
                    let want = if rest.is_some() {
                        format!("at least {}", items.len())
                    } else {
                        items.len().to_string()
                    };
                    return err(
                        ErrKind::Value,
                        *span,
                        format!("list pattern expects {want} elements, got {}", list.len()),
                    );
                }
                for (p, item) in items.iter().zip(list.iter()) {
                    self.bind(p, item.clone(), mutable, scope)?;
                }
                if let Some((name, s)) = rest {
                    let tail = Value::list(list[items.len()..].to_vec());
                    self.bind(&Pattern::Name(name.clone(), *s), tail, mutable, scope)?;
                }
                Ok(())
            }
            Pattern::Record { fields, span } => {
                let Value::Record(r) = &v else {
                    return err(
                        ErrKind::Type,
                        *span,
                        format!("cannot destructure {} with a record pattern", v.a_type()),
                    );
                };
                for (key, key_span, p) in fields {
                    let Some(field) = r.get(key) else {
                        let help = suggest(key, r.keys().map(|k| &**k))
                            .map(|s| format!("did you mean `{s}`?"));
                        return Err(OkError::at(
                            ErrKind::Key,
                            *key_span,
                            format!("the record has no field `{key}`"),
                        )
                        .maybe_help(help)
                        .into());
                    };
                    self.bind(p, field.clone(), mutable, scope)?;
                }
                Ok(())
            }
        }
    }

    fn assign(&mut self, place: &Place, op: AssignOp, value: &Expr, scope: &Rc<Scope>) -> R<()> {
        let Some(owner) = scope.owner(&place.root) else {
            return Err(self
                .unknown_name(&place.root, place.root_span, scope)
                .into());
        };
        let is_mut = owner
            .vars
            .borrow()
            .get(place.root.as_str())
            .is_some_and(|b| b.mutable);
        if !is_mut {
            return Err(OkError::at(
                ErrKind::Name,
                place.root_span,
                format!(
                    "cannot assign to `{}`: it was declared with let",
                    place.root
                ),
            )
            .with_help(format!(
                "declare it with `var {} = ...` to change it",
                place.root
            ))
            .into());
        }
        let mut keys = Vec::new();
        for seg in &place.path {
            keys.push(match seg {
                PlaceSeg::Field(n, s) => (Value::str(n.as_str()), *s, true),
                PlaceSeg::Index(e) => (self.eval(e, scope)?, e.span, false),
            });
        }
        let rhs = self.eval(value, scope)?;
        let new_value = if op == AssignOp::Set {
            rhs
        } else {
            let mut cur = owner.lookup(&place.root).unwrap_or(Value::Null);
            for (k, s, is_field) in &keys {
                cur = if *is_field {
                    self.field(&cur, &k.display(), *s)?
                } else {
                    self.index(&cur, k, *s)?
                };
            }
            let bop = if op == AssignOp::Add {
                BinOp::Add
            } else {
                BinOp::Sub
            };
            self.binop(bop, cur, rhs, value.span)?
        };
        // Take the value out of its binding so copy-on-write does not copy it.
        let mut root = {
            let mut vars = owner.vars.borrow_mut();
            let b = vars.get_mut(place.root.as_str()).expect("binding exists");
            std::mem::replace(&mut b.value, Value::Null)
        };
        let result = set_path(&mut root, &keys, new_value);
        owner
            .vars
            .borrow_mut()
            .get_mut(place.root.as_str())
            .expect("binding exists")
            .value = root;
        result
    }

    fn exec_for(
        &mut self,
        first: &Pattern,
        second: Option<&Pattern>,
        iter: &Expr,
        body: &Block,
        scope: &Rc<Scope>,
    ) -> R<()> {
        let v = self.eval_lazy(iter, scope)?;
        if let Value::Lazy(l) = &v
            && !l.text
        {
            return self.exec_for_stream(first, second, v, iter.span, body, scope);
        }
        let v = lazy::force(self, v, iter.span)?;
        let pairs: Vec<(Value, Option<Value>)> = match &v {
            Value::List(items) => {
                if second.is_some() {
                    items
                        .iter()
                        .enumerate()
                        .map(|(i, x)| (Value::Int(i as i64), Some(x.clone())))
                        .collect()
                } else {
                    items.iter().map(|x| (x.clone(), None)).collect()
                }
            }
            Value::Record(r) => r
                .iter()
                .map(|(k, x)| {
                    (
                        Value::str(k.clone()),
                        if second.is_some() {
                            Some(x.clone())
                        } else {
                            None
                        },
                    )
                })
                .collect(),
            Value::Str(_) => {
                return Err(OkError::at(
                    ErrKind::Type,
                    iter.span,
                    "cannot loop over a string directly",
                )
                .with_help("loop over s.chars() for characters or s.lines() for lines")
                .into());
            }
            other => {
                return err(
                    ErrKind::Type,
                    iter.span,
                    format!("cannot loop over {}", other.a_type()),
                );
            }
        };
        for (a, b) in pairs {
            let iter_scope = Scope::new(Some(scope.clone()));
            self.bind(first, a, false, &iter_scope)?;
            if let (Some(p), Some(b)) = (second, b) {
                self.bind(p, b, false, &iter_scope)?;
            }
            match self.exec_stmts(&body.stmts, &iter_scope) {
                Ok(_) | Err(Flow::Continue) => {}
                Err(Flow::Break) => break,
                Err(other) => return Err(other),
            }
        }
        Ok(())
    }

    /// A `for` loop over a lazy list pulls one element per iteration.
    fn exec_for_stream(
        &mut self,
        first: &Pattern,
        second: Option<&Pattern>,
        v: Value,
        span: Span,
        body: &Block,
        scope: &Rc<Scope>,
    ) -> R<()> {
        let mut src = lazy::cursor(self, v, span, "`for`")?;
        let mut index = 0i64;
        while let Some(item) = lazy::at_span(src.next(self), span)? {
            let iter_scope = Scope::new(Some(scope.clone()));
            match second {
                Some(p) => {
                    self.bind(first, Value::Int(index), false, &iter_scope)?;
                    self.bind(p, item, false, &iter_scope)?;
                }
                None => self.bind(first, item, false, &iter_scope)?,
            }
            index += 1;
            match self.exec_stmts(&body.stmts, &iter_scope) {
                Ok(_) | Err(Flow::Continue) => {}
                Err(Flow::Break) => break,
                Err(other) => return Err(other),
            }
        }
        Ok(())
    }

    // ----- expressions ----------------------------------------------------------

    fn unknown_name(&self, name: &str, span: Span, scope: &Rc<Scope>) -> OkError {
        let help = match name {
            "None" | "nil" | "undefined" | "NULL" => Some("ok spells it `null`".to_string()),
            "True" => Some("ok spells it `true`".to_string()),
            "False" => Some("ok spells it `false`".to_string()),
            "run" => Some("`run` must be followed by a command, like run cargo build, or called as run(\"prog\", arg)".to_string()),
            _ => habit(name).or_else(|| {
                let mut names = Vec::new();
                scope.all_names(&mut names);
                names.retain(|n| !n.starts_with('·'));
                suggest(name, names.iter().map(String::as_str)).map(|s| format!("did you mean `{s}`?"))
            }),
        };
        OkError::at(ErrKind::Name, span, format!("unknown name `{name}`"))
            .with_label("not defined here")
            .maybe_help(help)
    }

    /// Evaluate to a materialised value: lazy results are collected
    /// (SPEC.md 13.2). Almost everything uses this.
    pub fn eval(&mut self, e: &Expr, scope: &Rc<Scope>) -> R<Value> {
        let v = self.eval_lazy(e, scope)?;
        if lazy::is_lazy(&v) {
            lazy::force(self, v, e.span)
        } else {
            Ok(v)
        }
    }

    /// Evaluate, keeping a lazy result lazy. Used where streaming is allowed:
    /// pipeline inputs, call arguments (forced again unless the callee
    /// streams) and `for` loops.
    fn eval_lazy(&mut self, e: &Expr, scope: &Rc<Scope>) -> R<Value> {
        match &e.kind {
            ExprKind::Null => Ok(Value::Null),
            ExprKind::Bool(b) => Ok(Value::Bool(*b)),
            ExprKind::Int(i) => Ok(Value::Int(*i)),
            ExprKind::Float(f) => Ok(Value::Float(*f)),
            ExprKind::Duration(d) => Ok(Value::Duration(*d)),
            ExprKind::Str(pieces) => {
                let mut out = String::new();
                for p in pieces {
                    match p {
                        StrPiece::Lit(s) => out.push_str(s),
                        StrPiece::Interp { expr, spec } => {
                            let v = self.eval(expr, scope)?;
                            match spec {
                                None => out.push_str(&v.display()),
                                Some(spec) => {
                                    out.push_str(&self.format_value(&v, spec, expr.span)?)
                                }
                            }
                        }
                    }
                }
                Ok(Value::str(out))
            }
            ExprKind::Ident(name) => scope
                .lookup(name)
                .ok_or_else(|| self.unknown_name(name, e.span, scope).into()),
            ExprKind::It => scope.lookup(IT).ok_or_else(|| {
                OkError::at(
                    ErrKind::Name,
                    e.span,
                    "`.name` shorthand used outside of an argument",
                )
                .into()
            }),
            ExprKind::List(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for el in elems {
                    match el {
                        Elem::Item(x) => out.push(self.eval(x, scope)?),
                        Elem::Spread(x) => match self.eval(x, scope)? {
                            Value::List(items) => out.extend(items.iter().cloned()),
                            other => {
                                return err(
                                    ErrKind::Type,
                                    x.span,
                                    format!(
                                        "can only spread a list into a list, got {}",
                                        other.a_type()
                                    ),
                                );
                            }
                        },
                    }
                }
                Ok(Value::list(out))
            }
            ExprKind::Record(entries) => {
                let mut r = Record::new();
                for en in entries {
                    match en {
                        Entry::Field { key, value, .. } => {
                            let v = self.eval(value, scope)?;
                            r.insert(key.as_str(), v);
                        }
                        Entry::Spread(x) => match self.eval(x, scope)? {
                            Value::Record(src) => {
                                for (k, v) in src.iter() {
                                    r.insert(k.clone(), v.clone());
                                }
                            }
                            other => {
                                return err(
                                    ErrKind::Type,
                                    x.span,
                                    format!(
                                        "can only spread a record into a record, got {}",
                                        other.a_type()
                                    ),
                                );
                            }
                        },
                    }
                }
                Ok(Value::record(r))
            }
            ExprKind::Group(inner) => self.eval_lazy(inner, scope),
            ExprKind::Unary(op, inner) => {
                let v = self.eval(inner, scope)?;
                match (op, v) {
                    (UnOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                    (UnOp::Not, other) => {
                        let _ = self.truth(&other, inner.span, "`not`")?;
                        unreachable_value()
                    }
                    (UnOp::Neg, Value::Int(i)) => i
                        .checked_neg()
                        .map(Value::Int)
                        .ok_or_else(|| overflow(e.span)),
                    (UnOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
                    (UnOp::Neg, Value::Duration(d)) => d
                        .checked_neg()
                        .map(Value::Duration)
                        .ok_or_else(|| overflow(e.span)),
                    (UnOp::Neg, other) => err(
                        ErrKind::Type,
                        e.span,
                        format!("cannot negate {}", other.a_type()),
                    ),
                }
            }
            ExprKind::Binary(op, l, r) => match op {
                BinOp::And | BinOp::Or => {
                    let what = if *op == BinOp::And { "`and`" } else { "`or`" };
                    let a = self.eval(l, scope)?;
                    let a = self.truth(&a, l.span, what)?;
                    if (*op == BinOp::And && !a) || (*op == BinOp::Or && a) {
                        return Ok(Value::Bool(a));
                    }
                    let b = self.eval(r, scope)?;
                    Ok(Value::Bool(self.truth(&b, r.span, what)?))
                }
                BinOp::Coalesce => match self.eval(l, scope)? {
                    Value::Null => self.eval(r, scope),
                    v => Ok(v),
                },
                _ => {
                    let a = self.eval(l, scope)?;
                    let b = self.eval(r, scope)?;
                    self.binop(*op, a, b, e.span)
                }
            },
            ExprKind::Call { callee, args } => {
                let f = self.eval(callee, scope)?;
                let args = self.eval_args(args, scope, e.span)?;
                self.call_value(&f, args, e.span, Some(callee))
            }
            ExprKind::MethodCall {
                recv,
                name,
                name_span,
                args,
            } => {
                let rv = self.eval_lazy(recv, scope)?;
                let args = self.eval_args(args, scope, e.span)?;
                self.method_call(rv, name, *name_span, args, scope)
            }
            ExprKind::Field {
                expr,
                name,
                name_span,
            } => {
                let v = self.eval(expr, scope)?;
                self.field(&v, name, *name_span)
            }
            ExprKind::Index { expr, index } => {
                let v = self.eval(expr, scope)?;
                let i = self.eval(index, scope)?;
                self.index(&v, &i, index.span)
            }
            ExprKind::Catch(inner) => match self.eval(inner, scope) {
                Ok(v) => Ok(v),
                Err(Flow::Err(_)) => Ok(Value::Null),
                Err(other) => Err(other),
            },
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                let c = self.eval(cond, scope)?;
                if self.truth(&c, cond.span, "`if`")? {
                    self.exec_block(then, scope)
                } else if let Some(o) = otherwise {
                    self.eval(o, scope)
                } else {
                    Ok(Value::Null)
                }
            }
            ExprKind::Block(b) => self.exec_block(b, scope),
            ExprKind::Match { subject, arms } => self.eval_match(subject, arms, scope, e.span),
            ExprKind::Try {
                body,
                binding,
                handler,
                finally,
            } => {
                let outcome = match (self.exec_block(body, scope), handler) {
                    (Err(Flow::Err(error)), Some(handler)) => {
                        let s = Scope::new(Some(scope.clone()));
                        if let Some(name) = binding {
                            let rec = self.error_record(&error);
                            s.declare(name, rec, false);
                        }
                        self.exec_stmts(&handler.stmts, &s)
                    }
                    (other, _) => other,
                };
                let Some(finally) = finally else {
                    return outcome;
                };
                // The finally block runs however the try and catch blocks
                // ended; its own value is discarded (SPEC.md 6.7).
                match self.exec_block(finally, scope) {
                    Ok(_) => outcome,
                    Err(Flow::Err(mut e)) => {
                        if let Err(Flow::Err(pending)) = &outcome {
                            e.notes.push(format!(
                                "this error in a finally block replaced the error: {}",
                                pending.message
                            ));
                        }
                        Err(Flow::Err(e))
                    }
                    Err(other) => Err(other),
                }
            }
            ExprKind::Lambda(decl) => Ok(Value::Func(Rc::new(Func::User {
                decl: decl.clone(),
                env: scope.clone(),
            }))),
            ExprKind::Pipe { lhs, stage } => {
                if let Stage::Run(_) = stage {
                    return self.eval_run(e, scope, crate::command::Mode::Captured);
                }
                let left = if crate::command::is_run(lhs) {
                    self.eval_run(lhs, scope, crate::command::Mode::Piped)?
                } else {
                    self.eval_lazy(lhs, scope)?
                };
                self.eval_stage(left, stage, scope, e.span)
            }
            ExprKind::Run(_) | ExprKind::RunFn { .. } => {
                self.eval_run(e, scope, crate::command::Mode::Captured)
            }
        }
    }

    fn format_value(&self, v: &Value, spec: &str, span: Span) -> R<String> {
        let parsed = format::parse_spec(spec).map_err(|m| OkError::at(ErrKind::Format, span, m))?;
        format::apply(v, &parsed).map_err(|m| OkError::at(ErrKind::Format, span, m).into())
    }

    fn eval_args(&mut self, args: &[Arg], scope: &Rc<Scope>, span: Span) -> R<Args> {
        let mut out = Args {
            pos: Vec::with_capacity(args.len()),
            named: Vec::new(),
            span,
        };
        for a in args {
            match a {
                Arg::Pos(e) => out.pos.push(self.eval_lazy(e, scope)?),
                Arg::Spread(e) => match self.eval(e, scope)? {
                    Value::List(items) => out.pos.extend(items.iter().cloned()),
                    other => {
                        return err(
                            ErrKind::Type,
                            e.span,
                            format!(
                                "can only spread a list into arguments, got {}",
                                other.a_type()
                            ),
                        );
                    }
                },
                Arg::Named {
                    name,
                    name_span,
                    value,
                } => {
                    if out.named.iter().any(|(n, _, _)| n == name) {
                        return err(
                            ErrKind::Arg,
                            *name_span,
                            format!("argument `{name}` is given twice"),
                        );
                    }
                    let v = self.eval_lazy(value, scope)?;
                    out.named.push((name.clone(), v, *name_span));
                }
            }
        }
        Ok(out)
    }

    fn eval_stage(
        &mut self,
        left: Value,
        stage: &Stage,
        scope: &Rc<Scope>,
        span: Span,
    ) -> R<Value> {
        match stage {
            Stage::Call { callee, args } => {
                let f = self.eval(callee, scope)?;
                let mut a = self.eval_args(args, scope, span)?;
                a.pos.insert(0, left);
                self.call_value(&f, a, callee.span.to(span), Some(callee))
            }
            Stage::Method { recv, name, args } => {
                let rv = self.eval(recv, scope)?;
                let mut a = self.eval_args(args, scope, span)?;
                a.pos.insert(0, left);
                if let Value::Record(r) = &rv
                    && let Some(f @ Value::Func(_)) = r.get(name)
                {
                    let f = f.clone();
                    return self.call_value(&f, a, span, None);
                }
                err(
                    ErrKind::Type,
                    recv.span,
                    format!("`{name}` is not a function of {}", rv.a_type()),
                )
            }
            Stage::Value(e) => {
                let f = self.eval(e, scope)?;
                let a = Args {
                    pos: vec![left],
                    named: Vec::new(),
                    span,
                };
                self.call_value(&f, a, e.span, Some(e))
            }
            Stage::Run(e) => {
                Err(OkError::unimplemented(e.span, "piping into commands (`run`)", "M3").into())
            }
        }
    }

    fn eval_match(
        &mut self,
        subject: &Expr,
        arms: &[Arm],
        scope: &Rc<Scope>,
        span: Span,
    ) -> R<Value> {
        let v = self.eval(subject, scope)?;
        let mut bound: Vec<(String, Value)> = Vec::new();
        for arm in arms {
            for pat in &arm.pats {
                bound.clear();
                if !self.pattern_matches(pat, &v, scope, &mut bound)? {
                    continue;
                }
                let s = Scope::new(Some(scope.clone()));
                for (name, value) in bound.drain(..) {
                    s.declare(&name, value, false);
                }
                if let Some(g) = &arm.guard {
                    let gv = self.eval(g, &s)?;
                    if !self.truth(&gv, g.span, "a match guard")? {
                        continue;
                    }
                }
                return self.eval(&arm.body, &s);
            }
        }
        err(
            ErrKind::Value,
            span,
            format!("no match arm matches {}", v.repr()),
        )
        .map_err(|f| with_help(f, "add a `_ => ...` arm for all other values"))
    }

    /// Whether `v` matches `pat` (SPEC.md 6.6). Bindings are appended to
    /// `bound`; when the result is `false`, `bound` is left as it was.
    fn pattern_matches(
        &mut self,
        pat: &MatchPat,
        v: &Value,
        scope: &Rc<Scope>,
        bound: &mut Vec<(String, Value)>,
    ) -> R<bool> {
        let mark = bound.len();
        let hit = match pat {
            MatchPat::Wildcard(_) => true,
            MatchPat::Bind(name, _) => {
                bound.push((name.clone(), v.clone()));
                true
            }
            MatchPat::Lit(lit) => {
                let lv = self.eval(lit, scope)?;
                equal(&lv, v)
            }
            MatchPat::List { items, rest, .. } => match v {
                Value::List(list) => {
                    let len_ok = if rest.is_some() {
                        list.len() >= items.len()
                    } else {
                        list.len() == items.len()
                    };
                    let mut ok = len_ok;
                    if ok {
                        for (p, item) in items.iter().zip(list.iter()) {
                            if !self.pattern_matches(p, item, scope, bound)? {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok && let Some(ListRest::Bind(name)) = rest {
                        bound.push((name.clone(), Value::list(list[items.len()..].to_vec())));
                    }
                    ok
                }
                _ => false,
            },
            MatchPat::Record { fields, .. } => match v {
                Value::Record(r) => {
                    let mut ok = true;
                    for (key, p) in fields {
                        let matched = match r.get(key) {
                            Some(field) => self.pattern_matches(p, field, scope, bound)?,
                            None => false,
                        };
                        if !matched {
                            ok = false;
                            break;
                        }
                    }
                    ok
                }
                _ => false,
            },
            MatchPat::Alt(alts, _) => {
                let mut ok = false;
                for alt in alts {
                    if self.pattern_matches(alt, v, scope, bound)? {
                        ok = true;
                        break;
                    }
                }
                ok
            }
        };
        if !hit {
            bound.truncate(mark);
        }
        Ok(hit)
    }

    fn error_record(&mut self, e: &OkError) -> Value {
        let mut r = Record::new();
        r.insert("kind", Value::str(e.kind.name()));
        r.insert("message", Value::str(e.message.as_str()));
        if let Some(span) = e.span {
            let f = self.sources.file(span.file);
            let (line, col) = f.line_col(span.start as usize);
            r.insert("file", Value::str(f.name.as_str()));
            r.insert("line", Value::Int(line as i64));
            r.insert("column", Value::Int(col as i64));
        }
        if let Some(code) = e.exit_code {
            r.insert("code", Value::Int(code as i64));
        }
        if let Some(cmd) = &e.command {
            r.insert(
                "cmd",
                Value::list(cmd.iter().map(|a| Value::str(a.as_str())).collect()),
            );
        }
        if let Some(stderr) = &e.stderr {
            r.insert("stderr", Value::str(stderr.as_str()));
        }
        if let Some(status) = e.status {
            r.insert("status", Value::Int(status as i64));
        }
        let rc = Rc::new(r);
        self.caught.retain(|(w, _)| w.strong_count() > 0);
        self.caught.push((Rc::downgrade(&rc), e.clone()));
        Value::Record(rc)
    }

    /// The original error behind a caught error record, for `fail(e)`.
    pub fn original_error(&self, r: &Rc<Record>) -> Option<OkError> {
        self.caught
            .iter()
            .find(|(w, _)| w.upgrade().is_some_and(|x| Rc::ptr_eq(&x, r)))
            .map(|(_, e)| e.clone())
    }

    // ----- calls --------------------------------------------------------------

    pub fn call_value(
        &mut self,
        f: &Value,
        args: Args,
        span: Span,
        callee: Option<&Expr>,
    ) -> R<Value> {
        let Value::Func(func) = f else {
            let what = match callee.map(|c| &c.kind) {
                Some(ExprKind::Ident(n)) => format!("`{n}` is {}, not a function", f.a_type()),
                _ => format!("{} is not a function", f.a_type()),
            };
            return err(ErrKind::Type, callee.map_or(span, |c| c.span), what);
        };
        match &**func {
            Func::Builtin(b) => {
                let args = if b.lazy { args } else { self.force_args(args)? };
                (b.f)(self, args)
            }
            Func::User { decl, env } => {
                let (decl, env) = (decl.clone(), env.clone());
                self.call_user(&decl, &env, args)
            }
        }
    }

    /// Call any function value with positional arguments (used by builtins
    /// that take callbacks).
    pub fn call(&mut self, f: &Value, pos: Vec<Value>, span: Span) -> R<Value> {
        self.call_value(
            f,
            Args {
                pos,
                named: Vec::new(),
                span,
            },
            span,
            None,
        )
    }

    pub(crate) fn call_user(&mut self, decl: &Rc<FnDecl>, env: &Rc<Scope>, args: Args) -> R<Value> {
        if self.depth >= MAX_CALL_DEPTH {
            return err(
                ErrKind::Value,
                args.span,
                format!("too much recursion: more than {MAX_CALL_DEPTH} nested calls"),
            );
        }
        self.depth += 1;
        let r = self.call_user_inner(decl, env, args);
        self.depth -= 1;
        r
    }

    /// Materialise every argument (for functions that do not stream).
    fn force_args(&mut self, mut args: Args) -> R<Args> {
        let span = args.span;
        for v in args
            .pos
            .iter_mut()
            .chain(args.named.iter_mut().map(|(_, v, _)| v))
        {
            if lazy::is_lazy(v) {
                *v = lazy::force(self, std::mem::replace(v, Value::Null), span)?;
            }
        }
        Ok(args)
    }

    fn call_user_inner(&mut self, decl: &Rc<FnDecl>, env: &Rc<Scope>, args: Args) -> R<Value> {
        let args = self.force_args(args)?;
        let call_span = args.span;
        let scope = Scope::new(Some(env.clone()));
        let fname = display_fn_name(&decl.name);
        let positional: Vec<&Param> = decl.params.iter().filter(|p| !p.flag && !p.rest).collect();
        let rest = decl.params.iter().find(|p| p.rest);
        let mut pos_iter = args.pos.into_iter();
        let mut provided: HashMap<&str, Value> = HashMap::new();
        for p in &positional {
            match pos_iter.next() {
                Some(v) => {
                    provided.insert(p.name.as_str(), v);
                }
                None => break,
            }
        }
        let extra: Vec<Value> = pos_iter.collect();
        if let Some(r) = rest {
            provided.insert(r.name.as_str(), Value::list(extra));
        } else if !extra.is_empty() {
            let n = positional.len();
            return err(
                ErrKind::Arg,
                call_span,
                format!(
                    "{fname} takes {} positional argument{}, got {}",
                    n,
                    if n == 1 { "" } else { "s" },
                    n + extra.len()
                ),
            );
        }
        for (name, v, nspan) in args.named {
            match decl.params.iter().find(|p| p.name == name) {
                Some(p) if p.flag => {
                    provided.insert(p.name.as_str(), v);
                }
                Some(_) => {
                    return err(
                        ErrKind::Arg,
                        nspan,
                        format!(
                            "`{name}` is a positional parameter of {fname}; pass it by position"
                        ),
                    );
                }
                None => {
                    let flags: Vec<&str> = decl
                        .params
                        .iter()
                        .filter(|p| p.flag)
                        .map(|p| p.name.as_str())
                        .collect();
                    let help = suggest(&name, flags.iter().copied())
                        .map(|s| format!("did you mean `{s}`?"));
                    return Err(OkError::at(
                        ErrKind::Arg,
                        nspan,
                        format!("{fname} has no parameter `{name}`"),
                    )
                    .maybe_help(help)
                    .into());
                }
            }
        }
        for p in &decl.params {
            let v = match provided.remove(p.name.as_str()) {
                Some(v) => v,
                None => match &p.default {
                    Some(d) => self.eval(d, &scope)?,
                    None if p.flag && matches!(p.ty, Some(TypeAnn::Bool)) => Value::Bool(false),
                    None if p.flag && matches!(p.ty, Some(TypeAnn::List(_))) => {
                        Value::list(Vec::new())
                    }
                    None if matches!(p.ty, Some(TypeAnn::Optional(_))) => Value::Null,
                    None => {
                        let how = if p.flag {
                            format!(" (pass it as {}: ...)", p.name)
                        } else {
                            String::new()
                        };
                        return err(
                            ErrKind::Arg,
                            call_span,
                            format!("missing argument `{}` for {fname}{how}", p.name),
                        );
                    }
                },
            };
            // A rest parameter's annotation applies to each element (SPEC.md 7.2).
            let rest_ty;
            let ty = match &p.ty {
                Some(t) if p.rest => {
                    rest_ty = TypeAnn::List(Some(Box::new(t.clone())));
                    Some(&rest_ty)
                }
                other => other.as_ref(),
            };
            let v = match ty {
                Some(ty) => match check_type(v, ty) {
                    Ok(v) => v,
                    Err(got) => {
                        return Err(OkError::at(
                            ErrKind::Type,
                            call_span,
                            format!(
                                "argument `{}` of {fname} must be {}, got {}",
                                p.name,
                                type_phrase(ty),
                                got
                            ),
                        )
                        .with_note(format!(
                            "{fname} is declared at {}",
                            self.location(decl.name_span)
                        ))
                        .into());
                    }
                },
                None => v,
            };
            scope.declare(&p.name, v, false);
        }
        match self.exec_stmts(&decl.body.stmts, &scope) {
            Ok(v) => Ok(v),
            Err(Flow::Return(v)) => Ok(v),
            Err(Flow::Err(mut e)) => {
                if !decl.name.starts_with('<') && e.trace.len() < MAX_TRACE {
                    e.trace.push(Frame {
                        function: decl.name.clone(),
                        call_site: call_span,
                    });
                }
                Err(Flow::Err(e))
            }
            Err(other) => Err(other),
        }
    }

    fn location(&self, span: Span) -> String {
        let f = self.sources.file(span.file);
        let (l, c) = f.line_col(span.start as usize);
        format!("{}:{l}:{c}", f.name)
    }

    fn method_call(
        &mut self,
        rv: Value,
        name: &str,
        name_span: Span,
        mut args: Args,
        scope: &Rc<Scope>,
    ) -> R<Value> {
        if let Value::Env = rv {
            return crate::system::env_method(self, name, name_span, args);
        }
        if let Value::Record(r) = &rv
            && let Some(f @ Value::Func(_)) = r.get(name)
        {
            let f = f.clone();
            let span = args.span;
            return self.call_value(&f, args, span, None);
        }
        match scope.lookup(name) {
            Some(f @ Value::Func(_)) => {
                args.pos.insert(0, rv);
                self.call_value(&f, args, name_span, None)
            }
            Some(other) => err(
                ErrKind::Type,
                name_span,
                format!("`{name}` is {}, not a function", other.a_type()),
            ),
            None => {
                let mut names = Vec::new();
                scope.all_names(&mut names);
                if let Value::Record(r) = &rv {
                    names.extend(r.keys().map(|k| k.to_string()));
                }
                let help = habit(name).or_else(|| {
                    suggest(name, names.iter().map(String::as_str))
                        .map(|s| format!("did you mean `{s}`?"))
                });
                Err(OkError::at(
                    ErrKind::Name,
                    name_span,
                    format!("no function `{name}` to call on {}", rv.a_type()),
                )
                .maybe_help(help)
                .into())
            }
        }
    }

    pub fn field(&self, v: &Value, name: &str, span: Span) -> R<Value> {
        match v {
            Value::Record(r) => match r.get(name) {
                Some(x) => Ok(x.clone()),
                None => {
                    let help = suggest(name, r.keys().map(|k| &**k))
                        .map(|s| format!("did you mean `{s}`?"))
                        .or_else(|| {
                            if r.is_empty() {
                                None
                            } else {
                                let keys: Vec<&str> = r.keys().take(8).map(|k| &**k).collect();
                                Some(format!("it has {}", keys.join(", ")))
                            }
                        });
                    Err(OkError::at(
                        ErrKind::Key,
                        span,
                        format!("the record has no field `{name}`"),
                    )
                    .maybe_help(help)
                    .with_note(format!("use `.{name}?` to get null instead of an error"))
                    .into())
                }
            },
            Value::Path(p) => match name {
                "parent" => Ok(Value::path(p.parent())),
                "name" => Ok(Value::str(p.name())),
                "stem" => Ok(Value::str(p.stem())),
                "ext" => Ok(Value::str(p.ext())),
                _ => no_property(v, name, span, &["parent", "name", "stem", "ext"]),
            },
            Value::Duration(d) => match name {
                "seconds" => Ok(Value::Float(*d as f64 / 1e9)),
                "millis" => Ok(Value::Int(d / 1_000_000)),
                _ => no_property(v, name, span, &["seconds", "millis"]),
            },
            Value::Env => match self.env_var(name) {
                Some(v) => Ok(Value::str(v)),
                None => Err(OkError::at(ErrKind::Key, span, format!("the environment variable {name} is not set"))
                    .with_help(format!("give a default with env.get(\"{name}\", \"...\"), or test it with env.has(\"{name}\")"))
                    .into()),
            },
            Value::Time(t) => match crate::system::time_field(*t, name) {
                Some(v) => Ok(v),
                None => no_property(v, name, span, &["unix", "year", "month", "day", "hour", "minute", "second"]),
            },
            other => {
                let mut e = OkError::at(
                    ErrKind::Type,
                    span,
                    format!("{} has no field `{name}`", other.a_type()),
                );
                if matches!(other, Value::Str(_) | Value::List(_)) {
                    e = e.with_help(format!("to call a function, add parentheses: .{name}()"));
                }
                Err(e.into())
            }
        }
    }

    pub fn index(&self, v: &Value, i: &Value, span: Span) -> R<Value> {
        match (v, i) {
            (Value::List(items), Value::Int(n)) => match resolve_index(*n, items.len()) {
                Some(k) => Ok(items[k].clone()),
                None => index_error(*n, items.len(), "list", span),
            },
            (Value::Str(s), Value::Int(n)) => {
                let count = s.chars().count();
                match resolve_index(*n, count) {
                    Some(k) => Ok(Value::str(
                        s.chars().nth(k).map(String::from).unwrap_or_default(),
                    )),
                    None => index_error(*n, count, "string", span),
                }
            }
            (Value::Bytes(b), Value::Int(n)) => match resolve_index(*n, b.len()) {
                Some(k) => Ok(Value::Int(b[k] as i64)),
                None => index_error(*n, b.len(), "bytes value", span),
            },
            (Value::Record(_), Value::Str(k)) => self.field(v, k, span),
            (Value::List(_) | Value::Str(_) | Value::Bytes(_), other) => err(
                ErrKind::Type,
                span,
                format!("an index must be an int, got {}", other.a_type()),
            ),
            (Value::Record(_), other) => err(
                ErrKind::Type,
                span,
                format!("a record key must be a string, got {}", other.a_type()),
            ),
            (other, _) => err(
                ErrKind::Type,
                span,
                format!("cannot index {}", other.a_type()),
            ),
        }
    }

    pub fn binop(&self, op: BinOp, a: Value, b: Value, span: Span) -> R<Value> {
        use Value::*;
        let finite = |f: f64| {
            if f.is_finite() {
                Ok(Float(f))
            } else {
                err(ErrKind::Value, span, "the result is not a finite number")
            }
        };
        let mismatch = |a: &Value, b: &Value| -> R<Value> {
            let mut e = OkError::at(
                ErrKind::Type,
                span,
                format!(
                    "cannot use `{}` on {} and {}",
                    op.symbol(),
                    a.a_type(),
                    b.a_type()
                ),
            );
            match (op, a, b) {
                (BinOp::Add, Str(_), _) | (BinOp::Add, _, Str(_)) => {
                    e = e.with_help("build strings with interpolation: \"{a}{b}\"")
                }
                (BinOp::Div, Str(_), Str(_)) => {
                    e = e.with_help("to join paths, start with a path: path(\"build\") / \"out\"")
                }
                (BinOp::Add, Path(_), _) => {
                    e = e.with_help("join paths with `/`: dir / \"file.txt\"")
                }
                _ => {}
            }
            Err(e.into())
        };
        match op {
            BinOp::Add => match (&a, &b) {
                (Int(x), Int(y)) => x.checked_add(*y).map(Int).ok_or_else(|| overflow(span)),
                (Int(_) | Float(_), Int(_) | Float(_)) => finite(num(&a) + num(&b)),
                (Str(x), Str(y)) => Ok(Value::str(format!("{x}{y}"))),
                (List(x), List(y)) => Ok(Value::list(x.iter().chain(y.iter()).cloned().collect())),
                (Duration(x), Duration(y)) => x
                    .checked_add(*y)
                    .map(Duration)
                    .ok_or_else(|| overflow(span)),
                (Time(t), Duration(d)) | (Duration(d), Time(t)) => {
                    t.checked_add(*d).map(Time).ok_or_else(|| overflow(span))
                }
                _ => mismatch(&a, &b),
            },
            BinOp::Sub => match (&a, &b) {
                (Int(x), Int(y)) => x.checked_sub(*y).map(Int).ok_or_else(|| overflow(span)),
                (Int(_) | Float(_), Int(_) | Float(_)) => finite(num(&a) - num(&b)),
                (Duration(x), Duration(y)) => x
                    .checked_sub(*y)
                    .map(Duration)
                    .ok_or_else(|| overflow(span)),
                (Time(t), Duration(d)) => t.checked_sub(*d).map(Time).ok_or_else(|| overflow(span)),
                (Time(x), Time(y)) => x
                    .checked_sub(*y)
                    .map(Duration)
                    .ok_or_else(|| overflow(span)),
                _ => mismatch(&a, &b),
            },
            BinOp::Mul => match (&a, &b) {
                (Int(x), Int(y)) => x.checked_mul(*y).map(Int).ok_or_else(|| overflow(span)),
                (Int(_) | Float(_), Int(_) | Float(_)) => finite(num(&a) * num(&b)),
                (Duration(d), Int(_) | Float(_)) | (Int(_) | Float(_), Duration(d)) => {
                    let n = if let Duration(_) = a {
                        num(&b)
                    } else {
                        num(&a)
                    };
                    scale_duration(*d, n, span)
                }
                _ => mismatch(&a, &b),
            },
            BinOp::Div => match (&a, &b) {
                (Int(_) | Float(_), Int(_) | Float(_)) => {
                    if num(&b) == 0.0 {
                        return err(ErrKind::Value, span, "division by zero");
                    }
                    finite(num(&a) / num(&b))
                }
                (Duration(d), Int(_) | Float(_)) => {
                    if num(&b) == 0.0 {
                        return err(ErrKind::Value, span, "division by zero");
                    }
                    scale_duration(*d, 1.0 / num(&b), span)
                }
                (Duration(x), Duration(y)) => {
                    if *y == 0 {
                        return err(ErrKind::Value, span, "division by zero");
                    }
                    finite(*x as f64 / *y as f64)
                }
                (Path(p), Str(s)) => Ok(Value::path(p.join_str(s))),
                (Path(p), Path(q)) => Ok(Value::path(p.join(q))),
                _ => mismatch(&a, &b),
            },
            BinOp::Rem => match (&a, &b) {
                (Int(_), Int(0)) => err(ErrKind::Value, span, "division by zero in `%`"),
                (Int(x), Int(y)) => x.checked_rem(*y).map(Int).ok_or_else(|| overflow(span)),
                (Int(_) | Float(_), Int(_) | Float(_)) => {
                    if num(&b) == 0.0 {
                        return err(ErrKind::Value, span, "division by zero in `%`");
                    }
                    finite(num(&a) % num(&b))
                }
                _ => mismatch(&a, &b),
            },
            BinOp::Eq => Ok(Bool(equal(&a, &b))),
            BinOp::Ne => Ok(Bool(!equal(&a, &b))),
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => match compare(&a, &b) {
                Some(ord) => Ok(Bool(match op {
                    BinOp::Lt => ord.is_lt(),
                    BinOp::Le => ord.is_le(),
                    BinOp::Gt => ord.is_gt(),
                    _ => ord.is_ge(),
                })),
                None => err(
                    ErrKind::Type,
                    span,
                    format!(
                        "cannot compare {} with {} using `{}`",
                        a.a_type(),
                        b.a_type(),
                        op.symbol()
                    ),
                ),
            },
            BinOp::In | BinOp::NotIn => {
                let found = match (&a, &b) {
                    (x, List(items)) => items.iter().any(|i| equal(i, x)),
                    (Str(k), Record(r)) => r.contains(k),
                    (Str(sub), Str(s)) => s.contains(&**sub),
                    (x, Record(_)) => {
                        return err(
                            ErrKind::Type,
                            span,
                            format!("record keys are strings; cannot look up {}", x.a_type()),
                        );
                    }
                    (x, Str(_)) => {
                        return err(
                            ErrKind::Type,
                            span,
                            format!("can only look for a string in a string, got {}", x.a_type()),
                        );
                    }
                    (_, other) => {
                        return err(
                            ErrKind::Type,
                            span,
                            format!(
                                "`in` needs a list, record or string on the right, got {}",
                                other.a_type()
                            ),
                        );
                    }
                };
                Ok(Bool(if op == BinOp::In { found } else { !found }))
            }
            BinOp::And | BinOp::Or | BinOp::Coalesce => unreachable_value(),
        }
    }
}

/// Help for function names people bring from other languages
/// (SPEC.md 10.3).
fn habit(name: &str) -> Option<String> {
    let fix = match name {
        "length" | "lenght" | "size" | "count_of" => "use `len(x)`",
        "toUpperCase" | "uppercase" | "to_upper" => "use `upper(s)`",
        "toLowerCase" | "lowercase" | "to_lower" => "use `lower(s)`",
        "strip" => "use `trim(s)`",
        "startswith" | "startsWith" => "use `starts_with(s, prefix)`",
        "endswith" | "endsWith" => "use `ends_with(s, suffix)`",
        "includes" => "use `contains(x, item)` or `item in x`",
        "push" | "append" => "lists are values: write `xs += [item]` on a var",
        "println" | "echo" | "puts" | "console" => "use `print(...)`",
        "filter" => "use `where(xs, pred)`",
        "toString" | "to_string" => "use `str(x)`",
        "parseInt" => "use `int(s)`",
        _ => return None,
    };
    Some(fix.to_string())
}

fn unreachable_value() -> R<Value> {
    Err(OkError::new(ErrKind::Value, "internal error: unexpected operator").into())
}

fn with_help(f: Flow, help: &str) -> Flow {
    match f {
        Flow::Err(mut e) => {
            e.help = Some(help.to_string());
            Flow::Err(e)
        }
        other => other,
    }
}

fn overflow(span: Span) -> Flow {
    OkError::at(
        ErrKind::Value,
        span,
        "integer overflow: the result does not fit in 64 bits",
    )
    .into()
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Int(i) => *i as f64,
        Value::Float(f) => *f,
        _ => f64::NAN,
    }
}

fn scale_duration(d: i64, n: f64, span: Span) -> R<Value> {
    let r = d as f64 * n;
    if !r.is_finite() || r.abs() > i64::MAX as f64 {
        return Err(overflow(span));
    }
    Ok(Value::Duration(r.round() as i64))
}

fn resolve_index(n: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let k = if n < 0 { len + n } else { n };
    if (0..len).contains(&k) {
        Some(k as usize)
    } else {
        None
    }
}

fn index_error(n: i64, len: usize, what: &str, span: Span) -> R<Value> {
    Err(OkError::at(
        ErrKind::Index,
        span,
        format!("index {n} is out of range for a {what} of length {len}"),
    )
    .with_note("use `xs[i]?` to get null instead of an error")
    .into())
}

fn no_property(v: &Value, name: &str, span: Span, props: &[&str]) -> R<Value> {
    let help = suggest(name, props.iter().copied())
        .map(|s| format!("did you mean `{s}`?"))
        .unwrap_or_else(|| format!("its properties are {}", props.join(", ")));
    Err(OkError::at(
        ErrKind::Type,
        span,
        format!("{} has no property `{name}`", v.a_type()),
    )
    .with_help(help)
    .into())
}

fn display_fn_name(name: &str) -> String {
    if name.starts_with('<') {
        "the closure".to_string()
    } else {
        format!("`{name}`")
    }
}

pub(crate) fn type_phrase(ty: &TypeAnn) -> String {
    match ty {
        TypeAnn::Optional(inner) => format!("{} or null", type_phrase(inner)),
        TypeAnn::List(Some(inner)) => format!("a list of {}", type_phrase(inner)),
        TypeAnn::Any => "any value".into(),
        TypeAnn::Choice(cs) => format!(
            "one of {}",
            cs.iter()
                .map(|c| format!("{c:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => crate::value::with_article(&other.describe()),
    }
}

/// Check a value against a type annotation, applying the conversions of
/// SPEC.md 5.11. On failure returns a description of what was found.
pub fn check_type(v: Value, ty: &TypeAnn) -> Result<Value, String> {
    let ok = match (ty, &v) {
        (TypeAnn::Any, _) => true,
        (TypeAnn::Null, Value::Null) => true,
        (TypeAnn::Bool, Value::Bool(_)) => true,
        (TypeAnn::Int, Value::Int(_)) => true,
        (TypeAnn::Float, Value::Float(_)) => true,
        (TypeAnn::Float, Value::Int(i)) => return Ok(Value::Float(*i as f64)),
        (TypeAnn::String, Value::Str(_)) => true,
        (TypeAnn::Bytes, Value::Bytes(_)) => true,
        (TypeAnn::Path, Value::Path(_)) => true,
        (TypeAnn::Path, Value::Str(s)) => return Ok(Value::path(OkPath::parse(s))),
        (TypeAnn::Duration, Value::Duration(_)) => true,
        (TypeAnn::Time, Value::Time(_)) => true,
        (TypeAnn::Record, Value::Record(_)) => true,
        (TypeAnn::Fn, Value::Func(_)) => true,
        (TypeAnn::List(None), Value::List(_)) => true,
        (TypeAnn::List(Some(inner)), Value::List(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                match check_type(item.clone(), inner) {
                    Ok(x) => out.push(x),
                    Err(got) => return Err(format!("a list whose element {i} is {got}")),
                }
            }
            return Ok(Value::list(out));
        }
        (TypeAnn::Optional(_), Value::Null) => true,
        (TypeAnn::Optional(inner), _) => return check_type(v, inner),
        (TypeAnn::Choice(cs), Value::Str(s)) => {
            if cs.iter().any(|c| **c == **s) {
                true
            } else {
                return Err(v.repr());
            }
        }
        _ => false,
    };
    if ok { Ok(v) } else { Err(v.a_type()) }
}

fn set_path(target: &mut Value, keys: &[(Value, Span, bool)], new_value: Value) -> R<()> {
    let Some(((key, span, is_field), rest)) = keys.split_first() else {
        *target = new_value;
        return Ok(());
    };
    match (target, key) {
        (Value::Record(r), Value::Str(k)) => {
            let r = Rc::make_mut(r);
            if rest.is_empty() {
                r.insert(k.clone(), new_value);
                return Ok(());
            }
            match r.get_mut(k) {
                Some(inner) => set_path(inner, rest, new_value),
                None => err(
                    ErrKind::Key,
                    *span,
                    format!("the record has no field `{k}`"),
                ),
            }
        }
        (Value::List(items), Value::Int(n)) => {
            let len = items.len();
            let Some(i) = resolve_index(*n, len) else {
                return index_error(*n, len, "list", *span).map(|_| ());
            };
            set_path(&mut Rc::make_mut(items)[i], rest, new_value)
        }
        (t, k) => {
            if *is_field {
                err(
                    ErrKind::Type,
                    *span,
                    format!("cannot set field `{}` on {}", k.display(), t.a_type()),
                )
            } else {
                err(
                    ErrKind::Type,
                    *span,
                    format!("cannot set index {} on {}", k.repr(), t.a_type()),
                )
            }
        }
    }
}

/// Plain-language reason for an I/O error, without the OS error code.
pub fn io_reason(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "no such file".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        std::io::ErrorKind::InvalidData => "the file is not valid UTF-8 text".into(),
        _ => {
            let s = e.to_string();
            match s.find(" (os error") {
                Some(i) => s[..i].to_string(),
                None => s,
            }
        }
    }
}

/// Check which top-level items a file may contain (SPEC.md 4.1).
pub(crate) fn check_items(prog: &Program, kind: FileKind) -> Result<(), OkError> {
    let mut consts: HashSet<String> = HashSet::new();
    for stmt in &prog.stmts {
        if let StmtKind::Import {
            alias: Some((n, _)),
            ..
        } = &stmt.kind
        {
            consts.insert(n.clone());
        }
    }
    let project = matches!(kind, FileKind::Project { .. });
    let root = matches!(kind, FileKind::Project { root: true });
    for stmt in &prog.stmts {
        match (&stmt.kind, kind) {
            (StmtKind::Task(_), FileKind::Project { .. }) => {}
            (StmtKind::Task(t), _) => {
                return Err(OkError::at(
                    ErrKind::Syntax,
                    t.name_span,
                    "tasks can only be declared in project files (ok.ok and ok/*.ok)",
                )
                .with_help("use fn for a function in a script"));
            }
            (StmtKind::Requires { .. } | StmtKind::EnvFile { .. }, _) if root => {}
            (StmtKind::Requires { .. } | StmtKind::EnvFile { .. }, _) => {
                return Err(OkError::at(
                    ErrKind::Syntax,
                    stmt.span,
                    "this declaration is only allowed in a project's root ok.ok",
                ));
            }
            (_, FileKind::Script) => {}
            (StmtKind::Fn(d), _) => {
                consts.insert(d.name.clone());
            }
            (StmtKind::Import { .. }, _) => {}
            (
                StmtKind::Let {
                    pattern,
                    value,
                    mutable: false,
                },
                _,
            ) => {
                if let Some(span) = non_const(value, &consts) {
                    let (msg, help) = if project {
                        (
                            "a project file's top-level `let` must be a constant",
                            "loading a project never runs code; compute the value inside the task that needs it",
                        )
                    } else {
                        (
                            "a library's top-level `let` must be a constant",
                            "libraries never run code when imported; compute the value inside a function instead",
                        )
                    };
                    return Err(OkError::at(ErrKind::Syntax, span, msg).with_help(help));
                }
                let mut names = Vec::new();
                pattern.names(&mut names);
                consts.extend(names.into_iter().map(|(n, _)| n));
            }
            (_, FileKind::Project { .. }) => {
                return Err(OkError::at(
                    ErrKind::Syntax,
                    stmt.span,
                    "project files cannot run code at the top level",
                )
                .with_help("move it into a task, or compute it inside the task that needs it"));
            }
            (_, FileKind::Library) => {
                return Err(OkError::at(
                    ErrKind::Syntax,
                    stmt.span,
                    "an imported file can only declare fn, constant let and import",
                )
                .with_help("importing never runs code; move this into a function"));
            }
        }
    }
    // Arguments of dependencies are constant expressions (SPEC.md 15.5),
    // so planning an invocation never runs project code.
    for stmt in &prog.stmts {
        let StmtKind::Task(t) = &stmt.kind else {
            continue;
        };
        for need in &t.needs {
            for (_, _, value) in &need.args {
                if let Some(span) = non_const(value, &consts) {
                    return Err(OkError::at(
                        ErrKind::Syntax,
                        span,
                        "an argument of a dependency must be a constant",
                    )
                    .with_help(
                        "use literals, constant lets, path(...) and os.name/arch/exe; compute other values inside the task",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The span of the first part of `e` that is not a constant expression
/// (SPEC.md 15.2), or `None` if all of it is.
pub fn non_const(e: &Expr, known: &HashSet<String>) -> Option<Span> {
    match &e.kind {
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Duration(_) => None,
        ExprKind::Str(pieces) => pieces.iter().find_map(|p| match p {
            StrPiece::Interp { expr, .. } => non_const(expr, known),
            StrPiece::Lit(_) => None,
        }),
        ExprKind::Ident(n) if known.contains(n) => None,
        ExprKind::List(elems) => elems.iter().find_map(|el| match el {
            Elem::Item(x) | Elem::Spread(x) => non_const(x, known),
        }),
        ExprKind::Record(entries) => entries.iter().find_map(|en| match en {
            Entry::Field { value, .. } => non_const(value, known),
            Entry::Spread(x) => non_const(x, known),
        }),
        ExprKind::Group(x) | ExprKind::Unary(_, x) => non_const(x, known),
        ExprKind::Binary(_, a, b) => non_const(a, known).or_else(|| non_const(b, known)),
        ExprKind::Call { callee, args } if matches!(&callee.kind, ExprKind::Ident(n) if n == "path") => {
            args.iter().find_map(|a| match a {
                Arg::Pos(x) => non_const(x, known),
                _ => Some(e.span),
            })
        }
        ExprKind::Field { expr, name, .. }
            if matches!(&expr.kind, ExprKind::Ident(n) if n == "os")
                && matches!(name.as_str(), "name" | "arch" | "exe") =>
        {
            None
        }
        _ => Some(e.span),
    }
}

/// Flush stdout, ignoring errors (a closed pipe at exit is not a failure).
pub fn flush_stdout() {
    let _ = std::io::stdout().flush();
}
