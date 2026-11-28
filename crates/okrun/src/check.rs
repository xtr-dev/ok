//! The static checks of `ok self check` (SPEC.md 17): names that are not
//! defined, calls with the wrong arguments, and the swapped-`write` lint,
//! found without running anything.
//!
//! The checker walks the syntax tree with a model of the interpreter's
//! scopes (`interp.rs`): builtins and globals, the file's functions
//! (hoisted) and imports, `let`/`var` bindings in the order they run,
//! parameters, loop variables, match bindings and `catch` names. Its rule
//! is **no false positives**: a finding must be an error on every run that
//! reaches the code. Wherever the outcome depends on run-time values (a
//! function held in a variable, a record that might hold a function, code
//! inside `try` or `?` that turns errors into values) it stays silent.
//!
//! Code that runs later (function and closure bodies, tasks, parameter
//! defaults) sees every name its enclosing scopes declare anywhere, since
//! they may be declared by the time it is called. Code that runs in place
//! sees only the names declared before it in its own block.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::ast::*;
use crate::builtins::{self, BuiltinSig};
use crate::error::{ErrKind, OkError, suggest};
use crate::interp::FileKind;
use crate::path::OkPath;
use crate::source::Span;

/// What a name refers to, as far as the checker can tell.
#[derive(Clone)]
enum Sym {
    /// A function declared with `fn`, or a closure bound with `let`.
    User(Rc<Sig>),
    Builtin(&'static BuiltinSig),
    /// The global `env`.
    Env,
    /// An `import ... as name` namespace.
    Module(Rc<Module>),
    /// Any other value; it may hold a function.
    Value,
}

/// The parameters of a user function.
struct Sig {
    name: String,
    params: Vec<Param>,
    declared_at: Span,
    /// `file:line:col` of the declaration when it is in another file.
    origin: Option<String>,
}

struct Module {
    /// The import path as written, for messages.
    path: String,
    exports: HashMap<String, Sym>,
}

/// One scope: every name it declares anywhere (`all`) and the names
/// declared so far (`declared`).
#[derive(Default)]
struct Scope {
    all: HashMap<String, Sym>,
    declared: HashSet<String>,
}

impl Scope {
    fn declare(&mut self, name: &str, sym: Sym) {
        self.all.entry(name.to_string()).or_insert(sym);
        self.declared.insert(name.to_string());
    }
}

/// The result of looking a name up in the scopes.
enum Found {
    Sym(Sym),
    Missing,
}

/// Which line and column a byte offset is at, for notes.
fn line_col(text: &str, off: usize) -> (usize, usize) {
    let before = &text[..off.min(text.len())];
    let line = before.matches('\n').count() + 1;
    let col = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, col)
}

/// Check a script given as text, without reading any file: imports count
/// as unreadable. The fuzz target uses it.
pub fn check_source(src: &str) -> Vec<OkError> {
    match crate::parser::parse(0, src) {
        Ok(prog) => check(&prog, src, "<source>", None, FileKind::Script),
        Err(e) => vec![e],
    }
}

/// Check one parsed file. `name` is how messages refer to it, `path` is
/// where it is on disk (for imports). Returns errors and warnings, in
/// source order.
pub(crate) fn check_file(
    prog: &Program,
    text: &str,
    name: &str,
    path: &Path,
    kind: FileKind,
) -> Vec<OkError> {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    check(prog, text, name, Some(&dir), kind)
}

/// [`check_file`]; imports are read relative to `dir`, or not at all.
fn check(
    prog: &Program,
    text: &str,
    name: &str,
    dir: Option<&Path>,
    kind: FileKind,
) -> Vec<OkError> {
    let mut c = Checker {
        text,
        file_name: name.to_string(),
        scopes: Vec::new(),
        boundary: 0,
        quiet: 0,
        findings: Vec::new(),
        fn_fields: HashSet::new(),
        dynamic_fields: false,
    };
    // Globals: the builtins, `env`, the other global values.
    let mut globals = Scope::default();
    for n in builtins::names() {
        if let Some(s) = builtins::signature(n) {
            globals.declare(n, Sym::Builtin(s));
        }
    }
    for n in builtins::GLOBAL_VALUES {
        globals.declare(n, if *n == "env" { Sym::Env } else { Sym::Value });
    }
    c.scopes.push(globals);

    // The file scope: functions are hoisted and imports are bound before
    // any statement runs (interp.rs `exec_file`).
    let mut complete = true;
    let mut import_errors = Vec::new();
    let mut file = Scope::default();
    let mut seen = HashSet::new();
    collect_fn_fields(&prog.stmts, &mut c.fn_fields, &mut c.dynamic_fields);
    for stmt in &prog.stmts {
        let StmtKind::Import {
            path: raw,
            path_span,
            alias,
        } = &stmt.kind
        else {
            continue;
        };
        if crate::project::is_root_name(&OkPath::parse(raw).name()) {
            // A mount (SPEC.md 15.8) binds no name; elsewhere it fails.
            complete &= kind == FileKind::Project { root: true };
            continue;
        }
        let loaded = match dir {
            Some(d) => load_library(d, raw),
            None => Err(LoadError::Other),
        };
        match loaded {
            Ok((full, lib, lib_text)) => {
                library_fn_fields(&full, &lib, &mut seen, &mut c);
                // How the interpreter names the library (interp.rs `import`).
                let shown = OkPath::parse(name)
                    .parent()
                    .join(&OkPath::parse(raw))
                    .display();
                let exports = exports_of(&lib, Some((&shown, &lib_text)));
                match alias {
                    Some((a, _)) => {
                        for (n, s) in &exports {
                            if matches!(s, Sym::User(_)) {
                                c.fn_fields.insert(n.clone());
                            }
                        }
                        let m = Module {
                            path: raw.clone(),
                            exports,
                        };
                        file.declare(a, Sym::Module(Rc::new(m)));
                    }
                    None => {
                        for (n, s) in exports {
                            file.declare(&n, s);
                        }
                    }
                }
            }
            Err(LoadError::Missing(reason)) => {
                complete = false;
                import_errors.push(
                    OkError::at(
                        ErrKind::Io,
                        *path_span,
                        format!("cannot import \"{raw}\": {reason}"),
                    )
                    .with_help("the path is relative to the directory of this file"),
                );
            }
            Err(LoadError::Other) => complete = false,
        }
    }
    for stmt in &prog.stmts {
        match &stmt.kind {
            StmtKind::Fn(d) => file.declare(&d.name, Sym::User(sig_of(&d.name, d))),
            StmtKind::Let {
                pattern,
                value,
                mutable,
            } => {
                let sym = let_sym(pattern, value, *mutable);
                for (n, _) in names_of(pattern) {
                    file.all.entry(n).or_insert(sym.clone());
                }
            }
            _ => {}
        }
    }
    c.scopes.push(file);
    if !complete {
        // An import the checker cannot follow may define any name.
        return import_errors;
    }
    for stmt in &prog.stmts {
        c.stmt(stmt);
    }
    let mut out = import_errors;
    out.append(&mut c.findings);
    out.sort_by_key(|e| e.span.map(|s| s.start));
    out
}

enum LoadError {
    /// The file cannot be read. Holds the reason.
    Missing(String),
    /// It is not a library the checker can use (a syntax error, a URL).
    Other,
}

fn load_library(dir: &Path, raw: &str) -> Result<(PathBuf, Program, String), LoadError> {
    let p = OkPath::parse(raw);
    if raw.contains("://") || p.is_abs() || raw.starts_with('/') || raw.starts_with('\\') {
        return Err(LoadError::Other);
    }
    let full = dir.join(p.native());
    let text = std::fs::read_to_string(&full)
        .map_err(|e| LoadError::Missing(crate::interp::io_reason(&e)))?;
    let prog = crate::parser::parse(0, &text).map_err(|_| LoadError::Other)?;
    let full = crate::path::canonical(&full).unwrap_or(full);
    Ok((full, prog, text))
}

/// Record keys that may hold functions in an imported library and in
/// everything it imports.
fn library_fn_fields(full: &Path, lib: &Program, seen: &mut HashSet<PathBuf>, c: &mut Checker) {
    if !seen.insert(full.to_path_buf()) {
        return;
    }
    collect_fn_fields(&lib.stmts, &mut c.fn_fields, &mut c.dynamic_fields);
    let dir = full.parent().map(Path::to_path_buf).unwrap_or_default();
    for stmt in &lib.stmts {
        if let StmtKind::Import { path, alias, .. } = &stmt.kind {
            match load_library(&dir, path) {
                Ok((f, p, _)) => {
                    if alias.is_some() {
                        for (n, s) in exports_of(&p, None) {
                            if matches!(s, Sym::User(_)) {
                                c.fn_fields.insert(n);
                            }
                        }
                    }
                    library_fn_fields(&f, &p, seen, c);
                }
                // A library the checker cannot read may build records with
                // any function fields.
                Err(_) => c.dynamic_fields = true,
            }
        }
    }
}

/// The names a library exports: its top-level `fn` and `let` names that do
/// not start with `_` (SPEC.md 11). `origin` is the library's name and text,
/// for the notes that say where a function is declared.
fn exports_of(lib: &Program, origin: Option<(&str, &str)>) -> HashMap<String, Sym> {
    let mut out = HashMap::new();
    for stmt in &lib.stmts {
        match &stmt.kind {
            StmtKind::Fn(d) if !d.name.starts_with('_') => {
                let mut sig = Sig {
                    name: d.name.clone(),
                    params: d.params.clone(),
                    declared_at: d.name_span,
                    origin: None,
                };
                if let Some((shown, text)) = origin {
                    let (l, c) = line_col(text, d.name_span.start as usize);
                    sig.origin = Some(format!("{shown}:{l}:{c}"));
                }
                out.insert(d.name.clone(), Sym::User(Rc::new(sig)));
            }
            StmtKind::Let { pattern, .. } => {
                for (n, _) in names_of(pattern) {
                    if !n.starts_with('_') {
                        out.insert(n, Sym::Value);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn sig_of(name: &str, d: &FnDecl) -> Rc<Sig> {
    Rc::new(Sig {
        name: name.to_string(),
        params: d.params.clone(),
        declared_at: d.name_span,
        origin: None,
    })
}

/// What a `let` binds: a closure bound with `let` keeps its signature.
fn let_sym(pattern: &Pattern, value: &Expr, mutable: bool) -> Sym {
    match (pattern, &value.kind) {
        (Pattern::Name(n, _), ExprKind::Lambda(d)) if !mutable => Sym::User(sig_of(n, d)),
        _ => Sym::Value,
    }
}

fn names_of(p: &Pattern) -> Vec<(String, Span)> {
    let mut v = Vec::new();
    p.names(&mut v);
    v
}

/// Whether a record field holding this value could be a function.
fn may_be_fn(e: &Expr) -> bool {
    !matches!(
        e.kind,
        ExprKind::Null
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Duration(_)
            | ExprKind::Str(_)
            | ExprKind::List(_)
            | ExprKind::Record(_)
    )
}

/// Collect the record keys that may hold a function: keys of record
/// literals with such values, and fields assigned to. A computed key
/// (`r[k] = ...`, `from_entries`) could be any name.
fn collect_fn_fields(stmts: &[Stmt], out: &mut HashSet<String>, dynamic: &mut bool) {
    struct V<'a> {
        out: &'a mut HashSet<String>,
        dynamic: &'a mut bool,
    }
    impl V<'_> {
        fn expr(&mut self, e: &Expr) {
            match &e.kind {
                ExprKind::Record(entries) => {
                    for en in entries {
                        match en {
                            Entry::Field { key, value, .. } => {
                                if may_be_fn(value) {
                                    self.out.insert(key.clone());
                                }
                                self.expr(value);
                            }
                            Entry::Spread(x) => self.expr(x),
                        }
                    }
                }
                ExprKind::Ident(n) if n == "from_entries" => *self.dynamic = true,
                ExprKind::MethodCall { name, .. } if name == "from_entries" => {
                    *self.dynamic = true;
                    walk_children(e, self);
                }
                _ => walk_children(e, self),
            }
        }
        fn stmts(&mut self, stmts: &[Stmt]) {
            for s in stmts {
                if let StmtKind::Assign { target, .. } = &s.kind {
                    match target.path.last() {
                        Some(PlaceSeg::Field(n, _)) => {
                            self.out.insert(n.clone());
                        }
                        Some(PlaceSeg::Index(Expr {
                            kind: ExprKind::Str(pieces),
                            ..
                        })) if pieces.iter().all(|p| matches!(p, StrPiece::Lit(_))) => {
                            for p in pieces {
                                if let StrPiece::Lit(k) = p {
                                    self.out.insert(k.clone());
                                }
                            }
                        }
                        Some(PlaceSeg::Index(_)) => *self.dynamic = true,
                        None => {}
                    }
                }
                walk_stmt(s, self);
            }
        }
    }
    impl Visit for V<'_> {
        fn visit_expr(&mut self, e: &Expr) {
            self.expr(e);
        }
        fn visit_stmts(&mut self, s: &[Stmt]) {
            self.stmts(s);
        }
    }
    V { out, dynamic }.stmts(stmts);
}

/// A walk over the syntax tree.
trait Visit {
    fn visit_expr(&mut self, e: &Expr);
    fn visit_stmts(&mut self, s: &[Stmt]);
}

/// Visit every direct subexpression of `e` and every block of statements
/// directly inside it.
fn walk_children(e: &Expr, v: &mut dyn Visit) {
    let args = |a: &[Arg], v: &mut dyn Visit| {
        for x in a {
            match x {
                Arg::Pos(x) | Arg::Spread(x) | Arg::Named { value: x, .. } => v.visit_expr(x),
            }
        }
    };
    let decl = |d: &FnDecl, v: &mut dyn Visit| {
        for p in &d.params {
            if let Some(d) = &p.default {
                v.visit_expr(d);
            }
        }
        v.visit_stmts(&d.body.stmts);
    };
    match &e.kind {
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Duration(_)
        | ExprKind::Ident(_)
        | ExprKind::It => {}
        ExprKind::Str(pieces) => {
            for p in pieces {
                if let StrPiece::Interp { expr: x, .. } = p {
                    v.visit_expr(x);
                }
            }
        }
        ExprKind::List(elems) => {
            for el in elems {
                match el {
                    Elem::Item(x) | Elem::Spread(x) => v.visit_expr(x),
                }
            }
        }
        ExprKind::Record(entries) => {
            for en in entries {
                match en {
                    Entry::Field { value, .. } => v.visit_expr(value),
                    Entry::Spread(x) => v.visit_expr(x),
                }
            }
        }
        ExprKind::Group(x) | ExprKind::Unary(_, x) | ExprKind::Catch(x) => v.visit_expr(x),
        ExprKind::Binary(_, a, b) => {
            v.visit_expr(a);
            v.visit_expr(b);
        }
        ExprKind::Call { callee, args: a } => {
            v.visit_expr(callee);
            args(a, v);
        }
        ExprKind::MethodCall { recv, args: a, .. } => {
            v.visit_expr(recv);
            args(a, v);
        }
        ExprKind::Field { expr: x, .. } => v.visit_expr(x),
        ExprKind::Index { expr: x, index } => {
            v.visit_expr(x);
            v.visit_expr(index);
        }
        ExprKind::If {
            cond,
            then,
            otherwise,
        } => {
            v.visit_expr(cond);
            v.visit_stmts(&then.stmts);
            if let Some(o) = otherwise {
                v.visit_expr(o);
            }
        }
        ExprKind::Block(b) => v.visit_stmts(&b.stmts),
        ExprKind::Match { subject, arms } => {
            v.visit_expr(subject);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    v.visit_expr(g);
                }
                v.visit_expr(&arm.body);
            }
        }
        ExprKind::Try {
            body,
            handler,
            finally,
            ..
        } => {
            v.visit_stmts(&body.stmts);
            if let Some(h) = handler {
                v.visit_stmts(&h.stmts);
            }
            if let Some(f) = finally {
                v.visit_stmts(&f.stmts);
            }
        }
        ExprKind::Lambda(d) => decl(d, v),
        ExprKind::Pipe { lhs, stage } => {
            v.visit_expr(lhs);
            match stage {
                Stage::Call { callee, args: a } => {
                    v.visit_expr(callee);
                    args(a, v);
                }
                Stage::Method { recv, args: a, .. } => {
                    v.visit_expr(recv);
                    args(a, v);
                }
                Stage::Value(x) | Stage::Run(x) => v.visit_expr(x),
            }
        }
        ExprKind::Run(cmd) => {
            for w in cmd.words.iter().chain(cmd.redirects.iter().map(|(_, w)| w)) {
                for p in &w.parts {
                    match p {
                        CmdPart::Interp { expr: x, .. } | CmdPart::Spread(x) => v.visit_expr(x),
                        CmdPart::Lit(_) => {}
                    }
                }
            }
            if let Some(o) = &cmd.options {
                v.visit_expr(o);
            }
        }
        ExprKind::RunFn { args: a, .. } => args(a, v),
    }
}

/// Like [`walk_children`], for a statement.
fn walk_stmt(s: &Stmt, v: &mut dyn Visit) {
    let params = |ps: &[Param], v: &mut dyn Visit| {
        for p in ps {
            if let Some(d) = &p.default {
                v.visit_expr(d);
            }
        }
    };
    match &s.kind {
        StmtKind::Let { value, .. } => v.visit_expr(value),
        StmtKind::Fn(d) => {
            params(&d.params, v);
            v.visit_stmts(&d.body.stmts);
        }
        StmtKind::Task(t) => {
            params(&t.params, v);
            for e in t.sources.iter().chain(t.outputs.iter()) {
                v.visit_expr(e);
            }
            v.visit_stmts(&t.body.stmts);
        }
        StmtKind::Assign { target, value, .. } => {
            for seg in &target.path {
                if let PlaceSeg::Index(e) = seg {
                    v.visit_expr(e);
                }
            }
            v.visit_expr(value);
        }
        StmtKind::For { iter, body, .. } => {
            v.visit_expr(iter);
            v.visit_stmts(&body.stmts);
        }
        StmtKind::While { cond, body } => {
            v.visit_expr(cond);
            v.visit_stmts(&body.stmts);
        }
        StmtKind::Return(Some(e)) | StmtKind::Expr(e) => v.visit_expr(e),
        _ => {}
    }
}

/// The arguments of a call, normalised: the receiver of a method call or
/// the input of a pipeline stage counts as the first positional argument.
struct CallArgs<'a> {
    /// Positional arguments that are expressions in the source.
    pos: Vec<&'a Expr>,
    /// The number of positional arguments, or `None` when a spread makes it
    /// unknown.
    count: Option<usize>,
    named: Vec<(&'a str, Span)>,
}

impl<'a> CallArgs<'a> {
    fn new(first: Option<&'a Expr>, args: &'a [Arg]) -> CallArgs<'a> {
        let mut pos: Vec<&Expr> = first.into_iter().collect();
        let mut count = Some(pos.len());
        let mut named = Vec::new();
        for a in args {
            match a {
                Arg::Pos(e) => {
                    pos.push(e);
                    count = count.map(|c| c + 1);
                }
                Arg::Spread(_) => count = None,
                Arg::Named {
                    name, name_span, ..
                } => named.push((name.as_str(), *name_span)),
            }
        }
        CallArgs { pos, count, named }
    }
}

impl Visit for Checker<'_> {
    fn visit_expr(&mut self, e: &Expr) {
        self.expr(e);
    }
    fn visit_stmts(&mut self, s: &[Stmt]) {
        self.stmts(s);
    }
}

struct Checker<'a> {
    text: &'a str,
    file_name: String,
    scopes: Vec<Scope>,
    /// Scopes below this index are seen whole: the code being checked runs
    /// later, when all their names may be declared.
    boundary: usize,
    /// Inside `try { }` or a `?` chain errors become values, so nothing is
    /// reported there.
    quiet: usize,
    findings: Vec<OkError>,
    /// Record keys that may hold functions somewhere in the program.
    fn_fields: HashSet<String>,
    /// Records may get function fields under computed keys.
    dynamic_fields: bool,
}

impl Checker<'_> {
    fn report(&mut self, e: OkError) {
        if self.quiet == 0 {
            self.findings.push(e);
        }
    }

    fn lookup(&self, name: &str) -> Found {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if let Some(sym) = s.all.get(name)
                && (i < self.boundary || s.declared.contains(name))
            {
                return Found::Sym(sym.clone());
            }
        }
        Found::Missing
    }

    /// Every name that may be visible, for suggestions.
    fn visible_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .scopes
            .iter()
            .flat_map(|s| s.all.keys().cloned())
            // Internal names, such as the implicit closure parameter
            // (`ast::IT`), start with `·` and are never suggested.
            .filter(|n| !n.starts_with('·'))
            .collect();
        v.sort();
        v.dedup();
        v
    }

    fn location(&self, span: Span) -> String {
        let (l, c) = line_col(self.text, span.start as usize);
        format!("{}:{l}:{c}", self.file_name)
    }

    /// Push a scope for `stmts`, with `entry` names bound on entry and the
    /// block's functions hoisted (interp.rs `exec_stmts`).
    fn push_scope(&mut self, stmts: &[Stmt], entry: Vec<(String, Sym)>) {
        let mut s = Scope::default();
        for (n, sym) in entry {
            s.declare(&n, sym);
        }
        add_decls(&mut s, stmts, true);
        self.scopes.push(s);
    }

    fn block(&mut self, b: &Block) {
        self.push_scope(&b.stmts, Vec::new());
        self.stmts(&b.stmts);
        self.scopes.pop();
    }

    fn stmts(&mut self, stmts: &[Stmt]) {
        for s in stmts {
            self.stmt(s);
        }
    }

    fn declare_here(&mut self, name: &str, sym: Sym) {
        if let Some(s) = self.scopes.last_mut() {
            s.declare(name, sym);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let {
                pattern,
                value,
                mutable,
            } => {
                self.expr(value);
                let sym = let_sym(pattern, value, *mutable);
                for (n, _) in names_of(pattern) {
                    self.declare_here(&n, sym.clone());
                }
            }
            StmtKind::Fn(d) => self.function(d),
            StmtKind::Task(t) => {
                let saved = self.boundary;
                self.boundary = self.scopes.len();
                for e in t.sources.iter().chain(t.outputs.iter()) {
                    self.expr(e);
                }
                self.boundary = saved;
                self.body(&t.params, &t.body);
            }
            StmtKind::Import { .. } | StmtKind::Requires { .. } | StmtKind::EnvFile { .. } => {}
            StmtKind::Assign { target, value, .. } => {
                if let Found::Missing = self.lookup(&target.root) {
                    self.unknown_name(&target.root, target.root_span);
                }
                for seg in &target.path {
                    if let PlaceSeg::Index(e) = seg {
                        self.expr(e);
                    }
                }
                self.expr(value);
            }
            StmtKind::For {
                first,
                second,
                iter,
                body,
            } => {
                self.expr(iter);
                let mut entry: Vec<(String, Sym)> = names_of(first)
                    .into_iter()
                    .map(|(n, _)| (n, Sym::Value))
                    .collect();
                if let Some(p) = second {
                    entry.extend(names_of(p).into_iter().map(|(n, _)| (n, Sym::Value)));
                }
                self.push_scope(&body.stmts, entry);
                self.stmts(&body.stmts);
                self.scopes.pop();
            }
            StmtKind::While { cond, body } => {
                self.expr(cond);
                self.block(body);
            }
            StmtKind::Return(Some(e)) | StmtKind::Expr(e) => self.expr(e),
            StmtKind::Return(None) | StmtKind::Break | StmtKind::Continue => {}
        }
    }

    fn function(&mut self, d: &FnDecl) {
        self.body(&d.params, &d.body);
    }

    /// The parameters and body of a function, closure or task. It runs when
    /// called, so the enclosing scopes are seen whole.
    fn body(&mut self, params: &[Param], body: &Block) {
        let saved = self.boundary;
        self.boundary = self.scopes.len();
        let mut s = Scope::default();
        for p in params {
            s.all.entry(p.name.clone()).or_insert(Sym::Value);
        }
        add_decls(&mut s, &body.stmts, false);
        self.scopes.push(s);
        // Defaults are evaluated in order, each seeing the parameters
        // before it; the body's functions are hoisted after that.
        for p in params {
            if let Some(d) = &p.default {
                self.expr(d);
            }
            self.declare_here(&p.name, Sym::Value);
        }
        for st in &body.stmts {
            if let StmtKind::Fn(d) = &st.kind {
                self.declare_here(&d.name, Sym::User(sig_of(&d.name, d)));
            }
        }
        self.stmts(&body.stmts);
        self.scopes.pop();
        self.boundary = saved;
    }

    fn unknown_name(&mut self, name: &str, span: Span) {
        let help = match name {
            "None" | "nil" | "undefined" | "NULL" => Some("ok spells it `null`".to_string()),
            "True" => Some("ok spells it `true`".to_string()),
            "False" => Some("ok spells it `false`".to_string()),
            "run" => Some("`run` must be followed by a command, like run cargo build, or called as run(\"prog\", arg)".to_string()),
            _ if self.scopes.iter().any(|s| s.all.contains_key(name)) => Some(format!(
                "`{name}` is declared further down; this line runs before that declaration"
            )),
            _ => crate::interp::habit(name).or_else(|| {
                let names = self.visible_names();
                suggest(name, names.iter().map(String::as_str))
                    .map(|s| format!("did you mean `{s}`?"))
            }),
        };
        self.report(
            OkError::at(ErrKind::Name, span, format!("unknown name `{name}`"))
                .with_label("not defined here")
                .maybe_help(help),
        );
    }

    fn args(&mut self, args: &[Arg]) {
        for a in args {
            match a {
                Arg::Pos(e) | Arg::Spread(e) | Arg::Named { value: e, .. } => self.expr(e),
            }
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Ident(name) => {
                if let Found::Missing = self.lookup(name) {
                    self.unknown_name(name, e.span);
                }
            }
            ExprKind::Str(pieces) => {
                for p in pieces {
                    if let StrPiece::Interp { expr, spec, span } = p {
                        let before = self.findings.len();
                        self.expr(expr);
                        for f in &mut self.findings[before..] {
                            crate::interp::brace_hint(f, expr, spec.is_some(), *span);
                        }
                    }
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                self.args(args);
                if let ExprKind::Ident(name) = &callee.kind
                    && let Found::Sym(sym) = self.lookup(name)
                {
                    let a = CallArgs::new(None, args);
                    self.check_call(&sym, &a, e.span);
                }
            }
            ExprKind::MethodCall {
                recv,
                name,
                name_span,
                args,
            } => {
                self.expr(recv);
                self.args(args);
                self.method_call(recv, name, *name_span, args, e.span);
            }
            ExprKind::Field {
                expr: inner,
                name,
                name_span,
            } => {
                self.expr(inner);
                if let Some(m) = self.module_of(inner)
                    && !m.exports.contains_key(name)
                {
                    self.missing_export(inner, &m, name, *name_span);
                }
            }
            ExprKind::Catch(inner) => {
                self.quiet += 1;
                self.expr(inner);
                self.quiet -= 1;
            }
            ExprKind::If {
                cond,
                then,
                otherwise,
            } => {
                self.expr(cond);
                self.block(then);
                if let Some(o) = otherwise {
                    self.expr(o);
                }
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::Match { subject, arms } => {
                self.expr(subject);
                for arm in arms {
                    let mut names = Vec::new();
                    if let Some(p) = arm.pats.first() {
                        p.names(&mut names);
                    }
                    let mut s = Scope::default();
                    for (n, _) in names {
                        s.declare(&n, Sym::Value);
                    }
                    self.scopes.push(s);
                    if let Some(g) = &arm.guard {
                        self.expr(g);
                    }
                    self.expr(&arm.body);
                    self.scopes.pop();
                }
            }
            ExprKind::Try {
                body,
                binding,
                handler,
                finally,
            } => {
                self.quiet += 1;
                self.block(body);
                self.quiet -= 1;
                if let Some(h) = handler {
                    let entry = binding.iter().map(|n| (n.clone(), Sym::Value)).collect();
                    self.push_scope(&h.stmts, entry);
                    self.stmts(&h.stmts);
                    self.scopes.pop();
                }
                if let Some(f) = finally {
                    self.block(f);
                }
            }
            ExprKind::Lambda(d) => self.function(d),
            ExprKind::Pipe { lhs, stage } => {
                self.expr(lhs);
                self.stage(lhs, stage, e.span);
            }
            ExprKind::Run(cmd) => {
                self.backslash_before_blanks(cmd);
                walk_children(e, self);
            }
            _ => walk_children(e, self),
        }
    }

    fn stage(&mut self, lhs: &Expr, stage: &Stage, pipe: Span) {
        match stage {
            Stage::Call { callee, args } => {
                match &callee.kind {
                    ExprKind::Ident(name)
                        if crate::command::is_run(lhs)
                            && matches!(self.lookup(name), Found::Missing) =>
                    {
                        let stage_text = &self.text[callee.span.start as usize..pipe.end as usize];
                        let e = crate::interp::program_stage_error(name, stage_text, callee.span);
                        self.report(e);
                        // The arguments are most likely the program's words, so
                        // they are not reported as unknown names as well.
                        return;
                    }
                    _ => self.expr(callee),
                }
                self.args(args);
                let a = CallArgs::new(Some(lhs), args);
                match &callee.kind {
                    ExprKind::Ident(name) => {
                        if let Found::Sym(sym) = self.lookup(name) {
                            self.check_call(&sym, &a, callee.span);
                        }
                    }
                    ExprKind::Field {
                        expr: inner, name, ..
                    } => {
                        if let Some(m) = self.module_of(inner)
                            && let Some(sym) = m.exports.get(name)
                        {
                            let sym = sym.clone();
                            self.check_call(&sym, &a, callee.span);
                        }
                    }
                    _ => {}
                }
            }
            Stage::Method { recv, name, args } => {
                self.expr(recv);
                self.args(args);
                if let Some(m) = self.module_of(recv) {
                    match m.exports.get(name) {
                        Some(sym) => {
                            let sym = sym.clone();
                            let a = CallArgs::new(Some(lhs), args);
                            self.check_call(&sym, &a, recv.span);
                        }
                        None => self.missing_export(recv, &m, name, recv.span),
                    }
                }
            }
            Stage::Value(x) | Stage::Run(x) => self.expr(x),
        }
    }

    /// The module a name refers to, when `e` is the name of an import.
    fn module_of(&self, e: &Expr) -> Option<Rc<Module>> {
        match &e.kind {
            ExprKind::Ident(n) => match self.lookup(n) {
                Found::Sym(Sym::Module(m)) => Some(m),
                _ => None,
            },
            _ => None,
        }
    }

    fn missing_export(&mut self, recv: &Expr, m: &Module, name: &str, span: Span) {
        let ExprKind::Ident(alias) = &recv.kind else {
            return;
        };
        let help = suggest(name, m.exports.keys().map(String::as_str))
            .map(|s| format!("did you mean `{alias}.{s}`?"));
        self.report(
            OkError::at(
                ErrKind::Name,
                span,
                format!("`{alias}` (\"{}\") has no `{name}`", m.path),
            )
            .with_label("not exported by the library")
            .maybe_help(help.or_else(|| {
                Some(
                    "a library exports its top-level fn and let names that do not start with _"
                        .into(),
                )
            })),
        );
    }

    /// `recv.name(args)` (SPEC.md 6.4): a function field of a record, or else
    /// the function `name` called with `recv` first.
    fn method_call(&mut self, recv: &Expr, name: &str, name_span: Span, args: &[Arg], span: Span) {
        if let Some(m) = self.module_of(recv) {
            if let Some(sym) = m.exports.get(name) {
                let sym = sym.clone();
                self.check_call(&sym, &CallArgs::new(None, args), span);
                return;
            }
            // Not exported: called as `name(module, args)`, like any value.
            match self.lookup(name) {
                Found::Sym(sym) => self.check_call(&sym, &CallArgs::new(Some(recv), args), span),
                Found::Missing => self.missing_export(recv, &m, name, name_span),
            }
            return;
        }
        if let ExprKind::Ident(n) = &recv.kind
            && n == "env"
            && let Found::Sym(Sym::Env) = self.lookup(n)
        {
            self.env_method(name, name_span, args, span);
            return;
        }
        if self.may_have_fn_field(recv, name) {
            return;
        }
        let a = CallArgs::new(Some(recv), args);
        match self.lookup(name) {
            Found::Sym(sym) => self.check_call(&sym, &a, span),
            Found::Missing => {
                let names = self.visible_names();
                let help = crate::interp::habit(name).or_else(|| {
                    suggest(name, names.iter().map(String::as_str))
                        .map(|s| format!("did you mean `{s}`?"))
                });
                self.report(
                    OkError::at(
                        ErrKind::Name,
                        name_span,
                        format!("unknown function `{name}`"),
                    )
                    .with_label("not defined here")
                    .maybe_help(help),
                );
            }
        }
    }

    /// Whether `recv` might be a record with a function field `name`, which
    /// a method call would call instead of the function `name`.
    fn may_have_fn_field(&self, recv: &Expr, name: &str) -> bool {
        match &recv.kind {
            ExprKind::Null
            | ExprKind::Bool(_)
            | ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Duration(_)
            | ExprKind::Str(_)
            | ExprKind::List(_) => false,
            ExprKind::Record(entries) => entries.iter().any(|en| match en {
                Entry::Field { key, .. } => key == name,
                Entry::Spread(_) => true,
            }),
            ExprKind::Group(inner) => self.may_have_fn_field(inner, name),
            // A value that may be `env` (`let e = env`) has env's methods.
            _ => {
                self.dynamic_fields
                    || self.fn_fields.contains(name)
                    || builtins::ENV_METHODS.iter().any(|(m, _, _)| *m == name)
            }
        }
    }

    fn env_method(&mut self, name: &str, name_span: Span, args: &[Arg], span: Span) {
        let Some(&(_, min, max)) = builtins::ENV_METHODS.iter().find(|(m, _, _)| *m == name) else {
            let help = suggest(name, builtins::ENV_METHODS.iter().map(|(m, _, _)| *m))
                .map(|s| format!("did you mean env.{s}?"))
                .unwrap_or_else(|| {
                    format!("to read a variable named {name}, write env.{name} without parentheses")
                });
            self.report(
                OkError::at(
                    ErrKind::Name,
                    name_span,
                    format!("`env` has no method `{name}`"),
                )
                .with_help(help),
            );
            return;
        };
        let a = CallArgs::new(None, args);
        let fname = format!("env.{name}");
        if let Some((n, s)) = a.named.first() {
            self.report(OkError::at(
                ErrKind::Arg,
                *s,
                format!("`{fname}` has no parameter `{n}`"),
            ));
            return;
        }
        if let Some(n) = a.count
            && (n < min || n > max)
        {
            self.report(OkError::at(
                ErrKind::Arg,
                span,
                format!("`{fname}` takes {}, got {n}", count_phrase(min, max)),
            ));
        }
    }

    /// Check the arguments of a call of `sym`. Values may hold any function,
    /// so calls of them are not checked.
    fn check_call(&mut self, sym: &Sym, a: &CallArgs, span: Span) {
        match sym {
            Sym::Builtin(s) => self.check_builtin(s, a, span),
            Sym::User(sig) => self.check_user(sig, a, span),
            Sym::Env | Sym::Module(_) | Sym::Value => {}
        }
    }

    fn check_builtin(&mut self, s: &BuiltinSig, a: &CallArgs, span: Span) {
        // Builtins check named arguments first (list.rs `named`).
        for (n, nspan) in &a.named {
            if !s.named.contains(n) {
                let list = if s.named.is_empty() {
                    "no named arguments".to_string()
                } else {
                    format!("named arguments {}", s.named.join(", "))
                };
                let help =
                    suggest(n, s.named.iter().copied()).map(|x| format!("did you mean `{x}`?"));
                self.report(
                    OkError::at(
                        ErrKind::Arg,
                        *nspan,
                        format!("`{}` has no parameter `{n}`", s.name),
                    )
                    .with_note(format!("`{}` takes {list}", s.name))
                    .maybe_help(help),
                );
                return;
            }
        }
        if let Some(n) = a.count
            && (n < s.min || n > s.max)
        {
            self.report(OkError::at(
                ErrKind::Arg,
                span,
                format!("`{}` takes {}, got {n}", s.name, count_phrase(s.min, s.max)),
            ));
            return;
        }
        if s.name == "write" {
            self.swapped_write(a, span);
        }
    }

    fn check_user(&mut self, sig: &Sig, a: &CallArgs, span: Span) {
        let fname = format!("`{}`", sig.name);
        let at = match &sig.origin {
            Some(o) => o.clone(),
            None => self.location(sig.declared_at),
        };
        let note = format!("{fname} is declared at {at}");
        let positional: Vec<&Param> = sig.params.iter().filter(|p| !p.flag && !p.rest).collect();
        let has_rest = sig.params.iter().any(|p| p.rest);
        // The same order as the interpreter (interp.rs `call_user_inner`):
        // too many positional arguments, then named ones, then missing ones.
        if let Some(n) = a.count
            && !has_rest
            && n > positional.len()
        {
            let k = positional.len();
            self.report(
                OkError::at(
                    ErrKind::Arg,
                    span,
                    format!(
                        "{fname} takes {k} positional argument{}, got {n}",
                        if k == 1 { "" } else { "s" }
                    ),
                )
                .with_note(note),
            );
            return;
        }
        for (n, nspan) in &a.named {
            match sig.params.iter().find(|p| p.name == *n) {
                Some(p) if p.flag => {}
                Some(_) => {
                    self.report(
                        OkError::at(
                            ErrKind::Arg,
                            *nspan,
                            format!(
                                "`{n}` is a positional parameter of {fname}; pass it by position"
                            ),
                        )
                        .with_note(note),
                    );
                    return;
                }
                None => {
                    let flags = sig
                        .params
                        .iter()
                        .filter(|p| p.flag)
                        .map(|p| p.name.as_str());
                    let help = suggest(n, flags).map(|s| format!("did you mean `{s}`?"));
                    self.report(
                        OkError::at(
                            ErrKind::Arg,
                            *nspan,
                            format!("{fname} has no parameter `{n}`"),
                        )
                        .with_note(note)
                        .maybe_help(help),
                    );
                    return;
                }
            }
        }
        let given = |p: &Param| a.named.iter().any(|(n, _)| *n == p.name);
        let missing = sig.params.iter().enumerate().find(|(_, p)| {
            if !required(p) {
                return false;
            }
            if p.flag {
                return !given(p);
            }
            let index = positional.iter().position(|q| std::ptr::eq(*q, *p));
            matches!((a.count, index), (Some(n), Some(i)) if i >= n)
        });
        if let Some((_, p)) = missing {
            let how = if p.flag {
                format!(" (pass it as {}: ...)", p.name)
            } else {
                String::new()
            };
            self.report(
                OkError::at(
                    ErrKind::Arg,
                    span,
                    format!("missing argument `{}` for {fname}{how}", p.name),
                )
                .with_note(note),
            );
        }
    }

    /// The lint for a `\\` that is meant to continue a command and does not,
    /// because blanks follow it before the end of the line (SPEC.md 9.2
    /// rule 11). The blanks are invisible in an editor, and with them the
    /// command runs with `\\` as an argument and without the next line.
    fn backslash_before_blanks(&mut self, cmd: &Command) {
        let Some(end) = cmd
            .words
            .iter()
            .chain(cmd.redirects.iter().map(|(_, w)| w))
            .map(|w| w.span)
            .max_by_key(|s| s.end)
        else {
            return;
        };
        let (before, after) = self.text.split_at(end.end as usize);
        let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
        let rest = &after[blanks..];
        // A quoted word ends in its quote, so a word that ends in `\\` in the
        // source ends in an unquoted one.
        if !before.ends_with('\\')
            || blanks == 0
            || !(rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n"))
        {
            return;
        }
        let span = Span {
            start: end.end - 1,
            end: end.end + blanks as u32,
            ..end
        };
        let e = OkError::at(
            ErrKind::Syntax,
            span,
            "this `\\` does not continue the command: spaces follow it",
        )
        .with_note("the `\\` is passed to the program as an argument, and the command ends on this line")
        .with_help("remove the spaces after `\\` to continue on the next line, or write \"\\\\\" to pass a backslash")
        .as_warning();
        self.report(e);
    }

    /// The lint for `write(path, value)` (SPEC.md 12.1): `write` takes the
    /// value first.
    fn swapped_write(&mut self, a: &CallArgs, span: Span) {
        let (Some(first), Some(second)) = (a.pos.first(), a.pos.get(1)) else {
            return;
        };
        if !looks_like_file(first) || looks_like_file(second) {
            return;
        }
        let src = |e: &Expr| &self.text[e.span.start as usize..e.span.end as usize];
        let help = format!(
            "the value comes first and the path second: write({}, {})",
            src(second),
            src(first)
        );
        let e = OkError::at(
            ErrKind::Arg,
            span,
            format!(
                "the arguments of `write` look swapped: {} looks like a path",
                src(first)
            ),
        )
        .with_note("write(value, path) writes the value to the file at path")
        .with_help(help)
        .as_warning();
        self.report(e);
    }
}

fn count_phrase(min: usize, max: usize) -> String {
    let want = if min == max {
        format!("{min}")
    } else if max == usize::MAX {
        format!("at least {min}")
    } else {
        format!("{min} to {max}")
    };
    format!("{want} argument{}", if want == "1" { "" } else { "s" })
}

/// Whether a parameter must be given (SPEC.md 7.2).
fn required(p: &Param) -> bool {
    p.default.is_none()
        && !p.rest
        && !matches!(p.ty, Some(TypeAnn::Optional(_)))
        && !(p.flag && matches!(p.ty, Some(TypeAnn::Bool) | Some(TypeAnn::List(_))))
}

/// Whether an argument is a string or `path(...)` literal that looks like
/// a file name: no spaces or newlines, and an extension that `write`
/// knows (the same test `write` makes at run time).
fn looks_like_file(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Str(pieces) => {
            let mut s = String::new();
            for p in pieces {
                match p {
                    StrPiece::Lit(t) => s.push_str(t),
                    StrPiece::Interp { .. } => s.push('x'),
                }
            }
            crate::formats::looks_like_file_name(&s)
        }
        ExprKind::Call { callee, args } => {
            matches!(&callee.kind, ExprKind::Ident(n) if n == "path")
                && matches!(args.as_slice(), [Arg::Pos(x)] if looks_like_file(x))
        }
        _ => false,
    }
}

/// Add a block's declarations to a scope: every `let`/`var` name and
/// function, with functions declared from the start when `hoist` is set.
fn add_decls(s: &mut Scope, stmts: &[Stmt], hoist: bool) {
    for st in stmts {
        match &st.kind {
            StmtKind::Fn(d) => {
                let sym = Sym::User(sig_of(&d.name, d));
                if hoist {
                    s.declare(&d.name, sym);
                } else {
                    s.all.entry(d.name.clone()).or_insert(sym);
                }
            }
            StmtKind::Let {
                pattern,
                value,
                mutable,
            } => {
                let sym = let_sym(pattern, value, *mutable);
                for (n, _) in names_of(pattern) {
                    s.all.entry(n).or_insert(sym.clone());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The findings for a script, as `kind: message` lines.
    fn findings(src: &str) -> Vec<String> {
        let prog = crate::parser::parse(0, src).expect("parses");
        check_file(&prog, src, "t.ok", Path::new("t.ok"), FileKind::Script)
            .into_iter()
            .map(|e| {
                let w = if e.warning { "warning " } else { "" };
                format!("{w}{}: {}", e.kind.name(), e.message)
            })
            .collect()
    }

    fn clean(src: &str) {
        assert_eq!(findings(src), Vec::<String>::new(), "in:\n{src}");
    }

    /// Exactly one finding containing `want`; unless it is a warning, the
    /// interpreter must fail on the same code with an error of that kind.
    fn one(src: &str, want: &str) {
        let f = findings(src);
        assert!(
            f.len() == 1 && f[0].contains(want),
            "expected one finding with {want:?} in:\n{src}\ngot {f:?}"
        );
        if f[0].starts_with("warning") {
            return;
        }
        let mut interp = crate::interp::Interp::new(Vec::new());
        let r = interp.run_source("t.ok", src, std::env::temp_dir());
        match r {
            Err(crate::interp::Flow::Err(e)) => {
                assert!(
                    f[0].starts_with(e.kind.name()),
                    "{src}: run time gave {}",
                    e.message
                )
            }
            _ => panic!("{src}: runs without the error the checker reported"),
        }
    }

    #[test]
    fn names_follow_the_interpreter_scopes() {
        clean("print(f())\nfn f() { 1 }\n");
        clean("fn g() { x }\nlet x = 1\nprint(g())\n");
        clean("fn f() {\n  let g = () => y\n  let y = 2\n  g()\n}\n");
        clean("let xs = [[1, 2]]\nfor i, [a, b] in xs { print(i, a, b) }\n");
        clean(
            "let r = {a: 1, b: {c: 2}}\nlet {a, b: {c}} = r\nlet [h, ...t] = [1, 2]\nprint(a, c, h, t)\n",
        );
        clean(
            "let x = [1]\nprint(match x {\n  [a, ...rest] => a\n  {k: 1 | 2, v} if v > 0 => v\n  other => other\n})\n",
        );
        clean("let v = try { fail(\"x\") } catch e { e.message }\nprint(v)\n");
        clean("fn f(a, b = a, --dry-run: bool) { [a, b, dry_run] }\nprint(f(1, dry_run: true))\n");
        clean("var n = 0\nn += 1\nprint(\"{n}\")\n");
        clean("let users = [{active: true}]\nprint(users | where .active | map(.name?))\n");
        clean("let f = \"x\"\nrun ok-fixture echo {f} {...[f]} > {f}\n");
        clean("print(args, os.name, env.HOME?, cwd, home, invocation_dir, script_dir)\n");
        clean("let script_dir = 1\nprint(script_dir)\n");
        clean("fn main(input: path, --verbose, -v: bool) { print(input, verbose) }\n");
        clean("let fact = n => if n < 2 { 1 } else { n * fact(n - 1) }\nprint(fact(3))\n");
    }

    #[test]
    fn undefined_names_are_reported() {
        one("let name = 1\nprint(nmae)\n", "unknown name `nmae`");
        one("print(y)\nlet y = 1\n", "unknown name `y`");
        one("if true { print(z) }\nlet z = 1\n", "unknown name `z`");
        one("for x in [1] { }\nprint(x)\n", "unknown name `x`");
        one("nope = 1\n", "unknown name `nope`");
        one("print(\"{nope}\")\n", "unknown name `nope`");
        one("run ok-fixture echo {nope}\n", "unknown name `nope`");
        one(
            "fn f() { if true { print(w) }\n  let w = 1 }\nf()\n",
            "unknown name `w`",
        );
        one("print(\"a\".lenght())\n", "unknown function `lenght`");
        one("print(env.gte(\"A\"))\n", "`env` has no method `gte`");
    }

    #[test]
    fn errors_turned_into_values_are_not_reported() {
        clean("let v = nope?\nprint(v)\n");
        clean("let v = try { nope() } catch { 0 }\nprint(v)\n");
        clean("let v = len(1, 2)? ?? 0\nprint(v)\n");
    }

    #[test]
    fn argument_counts_and_names() {
        one("print(len(1, 2))\n", "`len` takes 1 argument, got 2");
        one("print(\"x\".trim(1))\n", "`trim` takes 1 argument, got 2");
        one("print([1] | take)\n", "`take` takes 2 arguments, got 1");
        one(
            "print(sort([1], desc: true, reverse: true))\n",
            "`sort` has no parameter `reverse`",
        );
        one(
            "fn f(a) { a }\nprint(f())\n",
            "missing argument `a` for `f`",
        );
        one(
            "fn f(a) { a }\nprint(f(1, 2))\n",
            "`f` takes 1 positional argument, got 2",
        );
        one(
            "fn f(--x: int) { x }\nprint(f())\n",
            "missing argument `x` for `f` (pass it as x: ...)",
        );
        one(
            "fn f(a) { a }\nprint(f(a: 1))\n",
            "`a` is a positional parameter of `f`",
        );
        one(
            "fn f(--loud: bool) { loud }\nprint(f(lound: true))\n",
            "`f` has no parameter `lound`",
        );
        one(
            "let g = x => x\nprint(g(1, 2))\n",
            "`g` takes 1 positional argument, got 2",
        );
        one(
            "print(env.get())\n",
            "`env.get` takes 1 to 2 arguments, got 0",
        );
        clean("fn f(a, b: int? , ...rest) { a }\nprint(f(1), f(1, 2, 3, 4))\n");
        clean("fn f(--xs: list, --b: bool, --o: int?) { xs }\nprint(f())\n");
        clean("let xs = [1, 2]\nprint(max(...xs), range(...xs))\n");
        clean("fn count(a, b, c) { a }\nprint(count(1, 2, 3))\n");
        clean("print(env.get(\"A\", \"b\"), \"x\".match('x'), now().format(\"%Y\"))\n");
    }

    #[test]
    fn records_may_hold_functions() {
        clean("let tools = {trim: x => x}\nprint(tools.trim(1, 2, 3))\n");
        clean("var r = {}\nr.split = x => x\nprint(r.split(1, 2, 3))\n");
        clean("let r = from_entries([])\nprint(r.anything(1))\n");
        clean("let e = env\nprint(e.set(\"A\", \"1\"))\n");
        // A literal receiver is never a record with functions.
        one(
            "print(\"x\".split(\",\", 1))\n",
            "`split` takes 1 to 2 arguments, got 3",
        );
    }

    #[test]
    fn backslash_before_blanks_is_a_warning() {
        let want = "warning syntax: this `\\` does not continue the command";
        one("run cargo build \\ \n", want);
        one("run cargo build \\\t\r\nprint(1)\n", want);
        one("run cargo build\\  ", want);
        one("run cat < in\\ \n", want);
        // A continuation, a quoted backslash, and a `\\` inside the line.
        clean("run cargo build \\\n  --release\n");
        clean("run echo \"\\\\\" \n");
        clean("run echo '\\' \n");
        clean("run dir C:\\ x \n");
        clean("run echo a\\ | upper\n");
    }

    #[test]
    fn swapped_write_is_a_warning() {
        one(
            "let data = 1\nwrite(\"out.json\", data)\n",
            "warning arg: the arguments of `write` look swapped",
        );
        one(
            "let data = 1\nwrite(path(\"out/a.csv\"), data)\n",
            "warning arg",
        );
        one("let data = 1\n\"out.json\".write(data)\n", "warning arg");
        clean("let data = 1\nwrite(data, \"out.json\")\n");
        clean("let data = 1\ndata | write(\"out.json\")\n");
        clean("write(\"notes.txt\", \"copy.txt\")\n");
        clean("write(\"hello world.txt\", \"x\")\n");
    }
}
