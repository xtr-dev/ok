//! Project mode (SPEC.md section 15): discovery, namespaces, mounts,
//! dependencies, up-to-date checks, confirmations, the task listing and
//! completions.
//!
//! Loading a project only parses its files and checks the declaration-only
//! rule; nothing runs until a task is chosen, its arguments are parsed, the
//! project is trusted (section 16) and its confirmations are answered.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::ast::{FnDecl, Program, StmtKind, TaskDecl, TypeAnn};
use crate::error::{ErrKind, OkError, suggest};
use crate::interp::{Args, FileKind, Flow, Interp, R, Scope, check_items};
use crate::path::OkPath;
use crate::sig;
use crate::source::{FileId, Span};
use crate::value::Value;

/// First arguments that are never tasks (SPEC.md 15.3).
pub const RESERVED: &[&str] = &["trust", "untrust", "self"];

pub struct PFile {
    /// Path relative to the project root, with `/`.
    pub rel: String,
    pub path: PathBuf,
    pub id: FileId,
    pub prog: Program,
    /// The directory tasks of this file run in (the project root, or a
    /// mounted sub-project's directory).
    pub run_dir: PathBuf,
}

pub struct Task {
    pub ns: Option<String>,
    pub name: String,
    pub decl: Rc<TaskDecl>,
    pub func: Rc<FnDecl>,
    pub file: usize,
    /// Indexes of the tasks this one needs, in order.
    pub needs: Vec<usize>,
}

impl Task {
    /// `build` or `db.migrate`.
    pub fn qualified(&self) -> String {
        match &self.ns {
            Some(ns) => format!("{ns}.{}", self.name),
            None => self.name.clone(),
        }
    }

    /// `ok build` or `ok db migrate`.
    pub fn command(&self) -> String {
        match &self.ns {
            Some(ns) => format!("ok {ns} {}", self.name),
            None => format!("ok {}", self.name),
        }
    }

    pub fn summary(&self) -> String {
        self.decl
            .doc
            .as_deref()
            .and_then(|d| d.lines().next())
            .unwrap_or("")
            .to_string()
    }
}

pub struct Project {
    pub root: PathBuf,
    pub files: Vec<PFile>,
    pub tasks: Vec<Task>,
    /// Namespace names: files in `ok/` and mounts.
    pub namespaces: Vec<String>,
    pub env_files: Vec<String>,
    /// Every project file for trust: root, `ok/*.ok`, mounts and imports.
    pub trust_files: Vec<(String, PathBuf)>,
}

/// The nearest directory at or above `start` that holds an `ok.ok`.
pub fn discover(start: &Path) -> Option<PathBuf> {
    let mut dir = crate::path::canonical(start).ok()?;
    loop {
        if dir.join("ok.ok").is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .map(|r| OkPath::from_native(r).display())
        .unwrap_or_else(|_| OkPath::from_native(p).display())
}

fn valid_name(n: &str) -> bool {
    let mut chars = n.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !n.ends_with('-')
}

/// Compare `requires` with this ok's version: `"0.3"` means 0.3.0 or newer.
fn version_ok(required: &str) -> Option<bool> {
    let parse = |s: &str| -> Option<Vec<u64>> {
        s.trim()
            .trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    };
    let mut want = parse(required)?;
    let mut have = parse(crate::VERSION)?;
    want.resize(3, 0);
    have.resize(3, 0);
    Some(have >= want)
}

impl Interp {
    fn parse_project_file(
        &mut self,
        root: &Path,
        path: &Path,
        kind: FileKind,
    ) -> R<(FileId, Program)> {
        let rel = rel_of(root, path);
        let text = std::fs::read_to_string(path).map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!("cannot read {rel}: {}", crate::interp::io_reason(&e)),
            )
        })?;
        let id = self
            .sources
            .add(rel, Some(path.to_path_buf()), text.clone().into());
        let prog = crate::parser::parse(id, &text)?;
        check_items(&prog, kind)?;
        Ok((id, prog))
    }

    /// Load the project at `root` without running any of its code.
    pub fn load_project(&mut self, root: &Path) -> R<Project> {
        let root = crate::path::canonical(root).unwrap_or_else(|_| root.to_path_buf());
        let mut files: Vec<PFile> = Vec::new();
        let mut file_ns: Vec<Option<String>> = Vec::new();

        let root_path = root.join("ok.ok");
        let (id, prog) =
            self.parse_project_file(&root, &root_path, FileKind::Project { root: true })?;
        let mut env_files = Vec::new();
        let mut mounts: Vec<(String, String, Span)> = Vec::new();
        for stmt in &prog.stmts {
            match &stmt.kind {
                StmtKind::Requires { version } => match version_ok(version) {
                    Some(true) => {}
                    Some(false) => {
                        return Err(OkError::at(ErrKind::Value, stmt.span, format!("this project requires ok {version} or newer; this is ok {}", crate::VERSION))
                            .with_help("update ok, or use the project's ./ok wrapper, which downloads the pinned version")
                            .into());
                    }
                    None => {
                        return Err(OkError::at(
                            ErrKind::Syntax,
                            stmt.span,
                            format!("{version:?} is not a version like \"0.3\""),
                        )
                        .into());
                    }
                },
                StmtKind::EnvFile { path } => env_files.push(path.clone()),
                StmtKind::Import {
                    path,
                    alias,
                    path_span,
                } if OkPath::parse(path).name() == "ok.ok" => match alias {
                    Some((a, _)) => mounts.push((path.clone(), a.clone(), *path_span)),
                    None => {
                        return Err(OkError::at(
                            ErrKind::Syntax,
                            *path_span,
                            "mounting a sub-project needs a name",
                        )
                        .with_help(format!("write import \"{path}\" as name"))
                        .into());
                    }
                },
                _ => {}
            }
        }
        files.push(PFile {
            rel: "ok.ok".into(),
            path: root_path,
            id,
            prog,
            run_dir: root.clone(),
        });
        file_ns.push(None);

        // Namespaces: each *.ok directly in ok/, except names starting with _.
        let ok_dir = root.join("ok");
        let mut helper_files = Vec::new();
        if ok_dir.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&ok_dir)
                .map_err(|e| {
                    OkError::new(
                        ErrKind::Io,
                        format!("cannot read ok/: {}", crate::interp::io_reason(&e)),
                    )
                })?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "ok") && p.is_file())
                .collect();
            entries.sort();
            for p in entries {
                let canon = crate::path::canonical(&p).unwrap_or_else(|_| p.clone());
                let rel = rel_of(&root, &p);
                if !canon.starts_with(&root) {
                    return Err(OkError::new(
                        ErrKind::Trust,
                        format!("{rel} is a link to a file outside the project"),
                    )
                    .with_help("project files must live inside the project root")
                    .into());
                }
                let stem = p
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if stem.starts_with('_') {
                    helper_files.push((rel, canon));
                    continue;
                }
                if !valid_name(&stem) {
                    return Err(OkError::new(
                        ErrKind::Syntax,
                        format!("{rel}: `{stem}` is not a valid namespace name"),
                    )
                    .with_help("use letters, digits, `_` and `-`")
                    .into());
                }
                if RESERVED.contains(&stem.as_str()) {
                    return Err(OkError::new(
                        ErrKind::Syntax,
                        format!("{rel}: `{stem}` is reserved and cannot be a namespace"),
                    )
                    .with_help("rename the file")
                    .into());
                }
                let (id, prog) =
                    self.parse_project_file(&root, &canon, FileKind::Project { root: false })?;
                files.push(PFile {
                    rel,
                    path: canon,
                    id,
                    prog,
                    run_dir: root.clone(),
                });
                file_ns.push(Some(stem));
            }
        }

        // Mounted sub-projects: their root tasks become a namespace.
        for (path, alias, span) in mounts {
            let target = root.join(OkPath::parse(&path).native());
            let canon = crate::path::canonical(&target).map_err(|e| {
                OkError::at(
                    ErrKind::Io,
                    span,
                    format!("cannot mount \"{path}\": {}", crate::interp::io_reason(&e)),
                )
            })?;
            if !canon.starts_with(&root) {
                return Err(OkError::at(
                    ErrKind::Trust,
                    span,
                    format!("\"{path}\" is outside the project"),
                )
                .into());
            }
            if file_ns.iter().any(|n| n.as_deref() == Some(alias.as_str())) {
                return Err(OkError::at(
                    ErrKind::Name,
                    span,
                    format!("the mount name `{alias}` is already the namespace ok/{alias}.ok"),
                )
                .into());
            }
            let (id, prog) =
                self.parse_project_file(&root, &canon, FileKind::Project { root: true })?;
            let run_dir = canon
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| root.clone());
            files.push(PFile {
                rel: rel_of(&root, &canon),
                path: canon,
                id,
                prog,
                run_dir,
            });
            file_ns.push(Some(alias));
        }

        // Tasks.
        let mut tasks: Vec<Task> = Vec::new();
        for (fi, f) in files.iter().enumerate() {
            for stmt in &f.prog.stmts {
                let StmtKind::Task(decl) = &stmt.kind else {
                    continue;
                };
                let ns = file_ns[fi].clone();
                if RESERVED.contains(&decl.name.as_str()) && ns.is_none() {
                    return Err(OkError::at(
                        ErrKind::Syntax,
                        decl.name_span,
                        format!("`{}` is reserved and cannot be a task name", decl.name),
                    )
                    .into());
                }
                if tasks.iter().any(|t| t.ns == ns && t.name == decl.name) {
                    return Err(OkError::at(
                        ErrKind::Name,
                        decl.name_span,
                        format!("task `{}` is declared twice", decl.name),
                    )
                    .into());
                }
                if ns.is_none()
                    && file_ns
                        .iter()
                        .any(|n| n.as_deref() == Some(decl.name.as_str()))
                {
                    return Err(OkError::at(
                        ErrKind::Name,
                        decl.name_span,
                        format!("task `{}` has the same name as a namespace", decl.name),
                    )
                    .with_help("rename the task or the namespace file")
                    .into());
                }
                let func = Rc::new(FnDecl {
                    name: decl.name.clone(),
                    name_span: decl.name_span,
                    params: decl.params.clone(),
                    body: decl.body.clone(),
                    doc: decl.doc.clone(),
                    span: decl.span,
                });
                tasks.push(Task {
                    ns,
                    name: decl.name.clone(),
                    decl: decl.clone(),
                    func,
                    file: fi,
                    needs: Vec::new(),
                });
            }
        }

        // Dependencies.
        let mounted: HashSet<String> = file_ns
            .iter()
            .enumerate()
            .filter(|(i, n)| n.is_some() && files[*i].run_dir != root)
            .filter_map(|(_, n)| n.clone())
            .collect();
        for ti in 0..tasks.len() {
            let mut needs = Vec::new();
            for need in tasks[ti].decl.needs.clone() {
                let (name, span) = (need.name.clone(), need.name_span);
                let found = match name.split_once('.') {
                    Some((ns, n)) => tasks
                        .iter()
                        .position(|t| t.ns.as_deref() == Some(ns) && t.name == n),
                    None => {
                        let own = tasks[ti].ns.clone();
                        tasks
                            .iter()
                            .position(|t| t.ns == own && t.name == name)
                            .or_else(|| {
                                if own.as_ref().is_some_and(|o| mounted.contains(o)) {
                                    None
                                } else {
                                    tasks.iter().position(|t| t.ns.is_none() && t.name == name)
                                }
                            })
                    }
                };
                let Some(dep) = found else {
                    let names: Vec<String> = tasks.iter().map(Task::qualified).collect();
                    let help = suggest(&name, names.iter().map(String::as_str))
                        .map(|s| format!("did you mean `{s}`?"));
                    return Err(OkError::at(
                        ErrKind::Name,
                        span,
                        format!("unknown task `{name}` in needs"),
                    )
                    .maybe_help(help)
                    .into());
                };
                check_need_args(&need, &tasks[dep].decl)?;
                needs.push(dep);
            }
            tasks[ti].needs = needs;
        }
        detect_cycles(&tasks)?;

        let mut namespaces: Vec<String> = file_ns.iter().flatten().cloned().collect();
        namespaces.sort();
        namespaces.dedup();

        // Trust covers every file the project consists of, imports included.
        let mut trust_files: Vec<(String, PathBuf)> = files
            .iter()
            .map(|f| (f.rel.clone(), f.path.clone()))
            .collect();
        trust_files.extend(helper_files);
        let mut queue: Vec<PathBuf> = trust_files.iter().map(|(_, p)| p.clone()).collect();
        let mut seen: HashSet<PathBuf> = queue.iter().cloned().collect();
        while let Some(p) = queue.pop() {
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(prog) = crate::parser::parse(0, &text) else {
                continue;
            };
            let dir = p.parent().map(Path::to_path_buf).unwrap_or_default();
            for stmt in &prog.stmts {
                if let StmtKind::Import { path, .. } = &stmt.kind
                    && let Ok(c) = crate::path::canonical(&dir.join(OkPath::parse(path).native()))
                    && c.starts_with(&root)
                    && seen.insert(c.clone())
                {
                    trust_files.push((rel_of(&root, &c), c.clone()));
                    queue.push(c);
                }
            }
        }
        trust_files.sort();
        trust_files.dedup();
        Ok(Project {
            root,
            files,
            tasks,
            namespaces,
            env_files,
            trust_files,
        })
    }
}

/// Whether a task parameter must be given (SPEC.md 7.2).
fn is_required(p: &crate::ast::Param) -> bool {
    p.default.is_none()
        && !p.rest
        && !matches!(p.ty, Some(TypeAnn::Optional(_)))
        && !(p.flag && matches!(p.ty, Some(TypeAnn::Bool) | Some(TypeAnn::List(_))))
}

/// Check the arguments of one `needs` entry against the parameters of the
/// task it names (SPEC.md 15.5). Their values are checked when the
/// invocation is planned, because constants are only evaluated then.
fn check_need_args(need: &crate::ast::Need, dep: &TaskDecl) -> R<()> {
    for (arg, span, _) in &need.args {
        match dep.params.iter().find(|p| p.name == *arg) {
            Some(p) if p.rest => {
                return Err(OkError::at(
                    ErrKind::Arg,
                    *span,
                    format!(
                        "`{arg}` collects the extra arguments of task `{}` and cannot be given in needs",
                        need.name
                    ),
                )
                .into());
            }
            Some(_) => {}
            None => {
                let names: Vec<&str> = dep
                    .params
                    .iter()
                    .filter(|p| !p.rest)
                    .map(|p| p.name.as_str())
                    .collect();
                let help = suggest(arg, names.iter().copied())
                    .map(|s| format!("did you mean `{s}`?"))
                    .or_else(|| {
                        Some(if names.is_empty() {
                            format!("task `{}` takes no arguments", need.name)
                        } else {
                            format!("its parameters are {}", names.join(", "))
                        })
                    });
                return Err(OkError::at(
                    ErrKind::Arg,
                    *span,
                    format!("task `{}` has no parameter `{arg}`", need.name),
                )
                .maybe_help(help)
                .into());
            }
        }
    }
    if let Some(p) = dep
        .params
        .iter()
        .find(|p| is_required(p) && !need.args.iter().any(|(a, _, _)| *a == p.name))
    {
        return Err(OkError::at(
            ErrKind::Arg,
            need.span,
            format!("task `{}` needs the argument `{}`", need.name, p.name),
        )
        .with_help(format!("give it here: {}({}: ...)", need.name, p.name))
        .into());
    }
    Ok(())
}

fn detect_cycles(tasks: &[Task]) -> R<()> {
    // 0 = unvisited, 1 = on the stack, 2 = done.
    fn visit(
        i: usize,
        tasks: &[Task],
        state: &mut Vec<u8>,
        stack: &mut Vec<usize>,
    ) -> Result<(), Vec<usize>> {
        state[i] = 1;
        stack.push(i);
        for &d in &tasks[i].needs {
            match state[d] {
                1 => {
                    let start = stack.iter().position(|&s| s == d).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(d);
                    return Err(cycle);
                }
                0 => visit(d, tasks, state, stack)?,
                _ => {}
            }
        }
        stack.pop();
        state[i] = 2;
        Ok(())
    }
    let mut state = vec![0u8; tasks.len()];
    for i in 0..tasks.len() {
        if state[i] == 0 {
            let mut stack = Vec::new();
            if let Err(cycle) = visit(i, tasks, &mut state, &mut stack) {
                let names: Vec<String> = cycle.iter().map(|&c| tasks[c].qualified()).collect();
                return Err(OkError::at(
                    ErrKind::Name,
                    tasks[cycle[0]].decl.name_span,
                    format!("task dependency cycle: {}", names.join(" -> ")),
                )
                .into());
            }
        }
    }
    Ok(())
}

/// Options from the command line that affect running tasks.
#[derive(Default, Clone)]
pub struct TaskOpts {
    pub force: bool,
    /// How many dependencies may run at the same time (`--jobs`); 1 runs
    /// them one after another in this process.
    pub jobs: usize,
    /// Whether worker processes render errors in color.
    pub color: bool,
    pub env: Option<String>,
    pub trust: bool,
    pub all: bool,
}

impl Project {
    pub fn find(&self, ns: Option<&str>, name: &str) -> Option<usize> {
        self.tasks
            .iter()
            .position(|t| t.ns.as_deref() == ns && t.name == name)
    }

    fn display_name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into())
    }

    /// The task listing (SPEC.md 15.9).
    pub fn listing(&self, all: bool, only_ns: Option<&str>) -> String {
        let visible = |t: &&Task| all || !t.decl.hidden;
        let width = self
            .tasks
            .iter()
            .filter(visible)
            .map(|t| t.name.chars().count())
            .max()
            .unwrap_or(0)
            .max(10)
            + 3;
        let mut out = String::new();
        if only_ns.is_none() {
            out.push_str(&format!(
                "{} ({})\n",
                self.display_name(),
                OkPath::from_native(&self.root).display()
            ));
            let root_tasks: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| t.ns.is_none())
                .filter(visible)
                .collect();
            if !root_tasks.is_empty() {
                out.push('\n');
            }
            for t in root_tasks {
                out.push_str(format!("  {:<width$}{}\n", t.name, t.summary()).trim_end());
                out.push('\n');
            }
        }
        for ns in &self.namespaces {
            if only_ns.is_some_and(|o| o != ns) {
                continue;
            }
            let ts: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| t.ns.as_deref() == Some(ns.as_str()))
                .filter(visible)
                .collect();
            if ts.is_empty() {
                continue;
            }
            out.push_str(&format!("\n{ns}\n"));
            for t in ts {
                out.push_str(format!("  {:<width$}{}", t.name, t.summary()).trim_end());
                out.push('\n');
            }
        }
        if self.tasks.iter().filter(visible).count() == 0 {
            out.push_str("\n  no tasks yet: add one to ok.ok, like\n\n    # Say hello\n    task hello {\n      print(\"hello\")\n    }\n");
        }
        out
    }

    /// Completion candidates for the words after `ok` (SPEC.md 17).
    pub fn complete(&self, words: &[String]) -> Vec<String> {
        let (done, current) = match words.split_last() {
            Some((cur, done)) => (done, cur.as_str()),
            None => (&[][..], ""),
        };
        let mut cands: Vec<String> = Vec::new();
        let visible = |t: &&Task| !t.decl.hidden;
        let task_words: Vec<&String> = done.iter().filter(|w| !w.starts_with('-')).collect();
        match task_words.as_slice() {
            [] => {
                cands.extend(
                    self.tasks
                        .iter()
                        .filter(|t| t.ns.is_none())
                        .filter(visible)
                        .map(|t| t.name.clone()),
                );
                cands.extend(self.namespaces.iter().cloned());
                if current.starts_with('-') {
                    cands.extend(
                        [
                            "--trace",
                            "--dry-run",
                            "--yes",
                            "--trust",
                            "--force",
                            "--env",
                            "--jobs",
                            "--color",
                            "--help",
                            "--version",
                        ]
                        .map(String::from),
                    );
                }
            }
            [first, rest @ ..] => {
                let task = if self.namespaces.contains(first) {
                    match rest.first() {
                        None => {
                            cands.extend(
                                self.tasks
                                    .iter()
                                    .filter(|t| t.ns.as_deref() == Some(first.as_str()))
                                    .filter(visible)
                                    .map(|t| t.name.clone()),
                            );
                            None
                        }
                        Some(n) => self.find(Some(first), n),
                    }
                } else {
                    self.find(None, first)
                };
                if let Some(ti) = task {
                    // The words after the task name: skip up to and
                    // including the namespace and task words.
                    let name_words = if self.namespaces.contains(first) {
                        2
                    } else {
                        1
                    };
                    let after = done
                        .iter()
                        .enumerate()
                        .filter(|(_, w)| !w.starts_with('-'))
                        .nth(name_words - 1)
                        .map_or(done.len(), |(i, _)| i + 1);
                    cands.extend(crate::sig::complete_args(
                        &self.tasks[ti].decl.params,
                        &done[after..],
                        current,
                    ));
                }
            }
        }
        cands.retain(|c| c.starts_with(current));
        cands.sort();
        cands.dedup();
        cands
    }
}

impl Interp {
    /// Run a task from the command line: `args` are the words after ok's
    /// options, starting with the task or namespace name.
    pub fn run_task(&mut self, project: &Project, args: &[String], opts: &TaskOpts) -> R<Value> {
        let Some(first) = args.first() else {
            print!("{}", project.listing(opts.all, None));
            return Ok(Value::Null);
        };
        let (ti, rest) = if let Some(ti) = project.find(None, first) {
            (ti, &args[1..])
        } else if project.namespaces.contains(first) {
            match args.get(1) {
                None => {
                    print!("{}", project.listing(opts.all, Some(first)).trim_start());
                    return Ok(Value::Null);
                }
                Some(name) => match project.find(Some(first), name) {
                    Some(ti) => (ti, &args[2..]),
                    None => {
                        let names: Vec<&str> = project
                            .tasks
                            .iter()
                            .filter(|t| t.ns.as_deref() == Some(first.as_str()))
                            .map(|t| t.name.as_str())
                            .collect();
                        let help = suggest(name, names.iter().copied())
                            .map(|s| format!("did you mean `ok {first} {s}`?"));
                        let mut e = OkError::new(
                            ErrKind::Arg,
                            format!("namespace `{first}` has no task `{name}`"),
                        )
                        .maybe_help(help);
                        e.exit_code = Some(2);
                        return Err(e.into());
                    }
                },
            }
        } else {
            let mut names: Vec<String> = project
                .tasks
                .iter()
                .filter(|t| t.ns.is_none())
                .map(|t| t.name.clone())
                .collect();
            names.extend(project.namespaces.iter().cloned());
            let help = suggest(first, names.iter().map(String::as_str))
                .map(|s| format!("did you mean `ok {s}`?"))
                .or_else(|| Some("run `ok` to list the tasks".into()));
            let mut e =
                OkError::new(ErrKind::Arg, format!("unknown task `{first}`")).maybe_help(help);
            e.exit_code = Some(2);
            return Err(e.into());
        };

        // Arguments are parsed before anything runs.
        let task = &project.tasks[ti];
        let target = sig::Target {
            usage: task.command(),
            doc: task.decl.doc.as_deref(),
            params: &task.decl.params,
        };
        // Tasks run in the project root, so relative path arguments are made
        // absolute against the directory the user typed them in.
        let anchor = self.invocation_dir != project.files[task.file].run_dir;
        let parsed = match sig::parse(&target, rest, &self.invocation_dir, anchor)? {
            sig::Outcome::Help(_) => {
                let mut text = sig::help(&target, &self.sources);
                if !task.needs.is_empty() {
                    // Each entry as written, so `build(release: true)` shows
                    // its arguments.
                    let file = self.sources.file(task.decl.name_span.file);
                    let names: Vec<String> = task
                        .decl
                        .needs
                        .iter()
                        .zip(&task.needs)
                        .map(|(n, &d)| {
                            let written = &file.text[n.span.start as usize..n.span.end as usize];
                            match written.find('(') {
                                Some(i) => {
                                    format!("{}{}", project.tasks[d].qualified(), &written[i..])
                                }
                                None => project.tasks[d].qualified(),
                            }
                        })
                        .collect();
                    text.push_str(&format!("\nRuns first: {}\n", names.join(", ")));
                }
                if let Some(c) = &task.decl.confirm {
                    text.push_str(&format!("Asks first: {c}\n"));
                }
                print!("{text}");
                return Ok(Value::Null);
            }
            sig::Outcome::Run(p) => p,
        };

        // Every task the invocation may run, dependencies first; used for
        // the confirmations, which only depend on the tasks, not on the
        // arguments given to them.
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        fn plan(i: usize, p: &Project, seen: &mut HashSet<usize>, order: &mut Vec<usize>) {
            if !seen.insert(i) {
                return;
            }
            for &d in &p.tasks[i].needs {
                plan(d, p, seen, order);
            }
            order.push(i);
        }
        plan(ti, project, &mut seen, &mut order);

        crate::trust::ensure_trusted(project, opts.trust)?;

        // All confirmations are asked before anything runs (SPEC.md 15.7).
        for &i in &order {
            if let Some(text) = &project.tasks[i].decl.confirm {
                let question = format!("{text}. Continue?");
                let yes = crate::system::ask_yes_no(&question, false, self.run_opts.yes, None)
                    .map_err(|e| {
                        e.with_note(format!(
                            "task `{}` asks for confirmation",
                            project.tasks[i].qualified()
                        ))
                    })?;
                if !yes {
                    eprintln!("cancelled");
                    return Err(Flow::Exit(1));
                }
            }
        }

        let scopes = self.enter_project(project, opts)?;
        let deps = self.plan_needs(project, &scopes, ti)?;
        self.run_needs(project, &scopes, &deps, opts)?;
        let span = task.decl.name_span;
        let args = Args {
            pos: parsed.pos,
            named: parsed
                .named
                .into_iter()
                .map(|(n, v)| (n, v, span))
                .collect(),
            span,
        };
        self.run_one(project, &scopes, ti, args, !opts.force)?;
        let _ = std::env::set_current_dir(&project.root);
        Ok(Value::Null)
    }

    /// Prepare the process for running tasks: env files, the working
    /// directory, the `cwd` and `invocation_dir` globals, and the
    /// declarations of every file (fns, constant lets, imports). Returns the
    /// top-level scope of each file.
    pub(crate) fn enter_project(
        &mut self,
        project: &Project,
        opts: &TaskOpts,
    ) -> R<HashMap<usize, Rc<Scope>>> {
        self.load_env_files(project, opts)?;
        self.project_root = Some(project.root.clone());
        std::env::set_current_dir(&project.root).map_err(|e| {
            OkError::new(ErrKind::Io, format!("cannot enter the project root: {e}"))
        })?;
        self.set_global("cwd", Value::path(OkPath::from_native(&project.root)));
        self.set_global(
            "invocation_dir",
            Value::path(OkPath::from_native(&self.invocation_dir.clone())),
        );
        let mut scopes: HashMap<usize, Rc<Scope>> = HashMap::new();
        for (fi, f) in project.files.iter().enumerate() {
            let dir = f.path.parent().map(Path::to_path_buf).unwrap_or_default();
            let scope = self.file_scope(&dir);
            self.exec_file(&f.prog, &scope, &dir, f.id)?;
            scopes.insert(fi, scope);
        }
        Ok(scopes)
    }

    /// Run one task in this process: enter its directory, skip it when it
    /// is up to date (if `check`), else call its body with `args`.
    pub(crate) fn run_one(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        ti: usize,
        args: Args,
        check: bool,
    ) -> R<()> {
        let t = &project.tasks[ti];
        let run_dir = &project.files[t.file].run_dir;
        std::env::set_current_dir(run_dir).map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!(
                    "cannot enter {}: {e}",
                    OkPath::from_native(run_dir).display()
                ),
            )
        })?;
        self.set_global("cwd", Value::path(OkPath::from_native(run_dir)));
        if check && self.up_to_date(t, &scopes[&t.file], run_dir)? {
            eprintln!("{}: up to date", t.qualified());
            return Ok(());
        }
        // The task named on the command line is called with its own name
        // as the call span (a dependency with its `needs` entry); it was
        // started by ok, not called from source.
        let result = self.call_user(&t.func, &scopes[&t.file], args);
        crate::interp::started_by_ok(result, &t.func.name, t.decl.name_span)?;
        Ok(())
    }

    /// `env file` declarations and `--env NAME` (SPEC.md 15.7). Variables
    /// already set in the process environment win.
    fn load_env_files(&mut self, project: &Project, opts: &TaskOpts) -> R<()> {
        let mut files: Vec<(PathBuf, bool)> = project
            .env_files
            .iter()
            .map(|f| (project.root.join(OkPath::parse(f).native()), false))
            .collect();
        if let Some(name) = &opts.env {
            let extra: Vec<(PathBuf, bool)> = if project.env_files.is_empty() {
                vec![(project.root.join(format!(".env.{name}")), true)]
            } else {
                project
                    .env_files
                    .iter()
                    .map(|f| {
                        (
                            project
                                .root
                                .join(OkPath::parse(&format!("{f}.{name}")).native()),
                            true,
                        )
                    })
                    .collect()
            };
            files.extend(extra);
        }
        let mut merged: Vec<(String, String)> = Vec::new();
        for (path, required) in files {
            let rel = rel_of(&project.root, &path);
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(_) if !required => continue,
                Err(e) => {
                    return Err(OkError::new(
                        ErrKind::Io,
                        format!(
                            "cannot read {rel} (asked for with --env): {}",
                            crate::interp::io_reason(&e)
                        ),
                    )
                    .into());
                }
            };
            let Value::Record(r) = crate::formats::parse_document("env", &text, &rel)
                .map_err(|m| OkError::new(ErrKind::Format, m))?
            else {
                continue;
            };
            for (k, v) in r.iter() {
                merged.retain(|(mk, _)| mk != &**k);
                merged.push((k.to_string(), v.display()));
            }
        }
        for (k, v) in merged {
            if std::env::var_os(&k).is_none() {
                self.set_env(&k, Some(v));
            }
        }
        Ok(())
    }

    /// Whether a task with `outputs` can be skipped (SPEC.md 15.6).
    pub(crate) fn up_to_date(&mut self, t: &Task, scope: &Rc<Scope>, dir: &Path) -> R<bool> {
        let Some(outputs) = &t.decl.outputs else {
            return Ok(false);
        };
        let patterns = |interp: &mut Interp, e: &crate::ast::Expr| -> R<Vec<String>> {
            match interp.eval(e, scope)? {
                Value::List(items) => items
                    .iter()
                    .map(|v| match v {
                        Value::Str(s) => Ok(s.to_string()),
                        Value::Path(p) => Ok(p.display()),
                        other => Err(OkError::at(
                            ErrKind::Type,
                            e.span,
                            format!("glob patterns must be strings, got {}", other.a_type()),
                        )
                        .into()),
                    })
                    .collect(),
                other => Err(OkError::at(
                    ErrKind::Type,
                    e.span,
                    format!("expected a list of glob patterns, got {}", other.a_type()),
                )
                .into()),
            }
        };
        let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
        let mut oldest_output = None;
        for pat in patterns(self, outputs)? {
            let found = crate::files::glob_native(&pat, dir)
                .map_err(|m| OkError::at(ErrKind::Value, outputs.span, m))?;
            if found.is_empty() {
                return Ok(false);
            }
            for f in found {
                let Some(m) = mtime(&f) else { return Ok(false) };
                oldest_output = Some(oldest_output.map_or(m, |o: std::time::SystemTime| o.min(m)));
            }
        }
        let Some(oldest) = oldest_output else {
            return Ok(false);
        };
        if let Some(sources) = &t.decl.sources {
            for pat in patterns(self, sources)? {
                for f in crate::files::glob_native(&pat, dir)
                    .map_err(|m| OkError::at(ErrKind::Value, sources.span, m))?
                {
                    if mtime(&f).is_none_or(|m| m >= oldest) {
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
}

/// Completion scripts for `ok self completions SHELL`.
pub fn completion_script(shell: &str) -> Option<&'static str> {
    Some(match shell {
        "bash" => {
            "# ok completions for bash. Add to ~/.bashrc:  eval \"$(ok self completions bash)\"\n\
_ok_complete() {\n\
  local IFS=$'\\n'\n\
  COMPREPLY=($(ok self complete -- \"${COMP_WORDS[@]:1:COMP_CWORD}\" 2>/dev/null))\n\
}\n\
complete -o default -F _ok_complete ok\n"
        }
        "zsh" => {
            "#compdef ok\n# ok completions for zsh. Add to ~/.zshrc:  eval \"$(ok self completions zsh)\"\n\
_ok() {\n\
  local -a candidates\n\
  candidates=(\"${(@f)$(ok self complete -- \"${(@)words[2,CURRENT]}\" 2>/dev/null)}\")\n\
  compadd -a candidates\n\
}\n\
compdef _ok ok\n"
        }
        "fish" => {
            "# ok completions for fish. Save as ~/.config/fish/completions/ok.fish\n\
complete -c ok -f -a '(ok self complete -- (commandline -opc)[2..-1] (commandline -ct) 2>/dev/null)'\n"
        }
        "powershell" => {
            "# ok completions for PowerShell. Add to $PROFILE:  ok self completions powershell | Out-String | Invoke-Expression\n\
Register-ArgumentCompleter -Native -CommandName ok -ScriptBlock {\n\
    param($wordToComplete, $commandAst, $cursorPosition)\n\
    $words = @($commandAst.CommandElements | Select-Object -Skip 1 | ForEach-Object { $_.ToString() })\n\
    if ($wordToComplete -eq '') { $words += '' }\n\
    ok self complete -- @words 2>$null | ForEach-Object {\n\
        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)\n\
    }\n\
}\n"
        }
        _ => return None,
    })
}
