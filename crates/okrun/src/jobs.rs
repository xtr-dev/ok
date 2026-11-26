//! Running a task's dependencies (SPEC.md 15.5): the plan of task instances
//! (a task together with the arguments a `needs` entry gives it), running
//! them one after another or in parallel, and the worker processes that run
//! the parallel ones.
//!
//! The interpreter is single-threaded (`Rc` scopes) and a running task owns
//! process-wide state: the working directory, stdout and the terminal. So a
//! dependency that may run at the same time as another one runs in its own
//! `ok` process, a *worker*, started with the hidden option
//! `--task-worker`. The parent prefixes every line a worker prints with the
//! task's name. Dependencies that can never overlap with another one, such
//! as a single chain `a -> b -> c`, keep running in this process exactly as
//! they always did.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc;

use crate::error::{ErrKind, OkError};
use crate::interp::{Args, Interp, R, Scope, check_type, flush_stdout};
use crate::project::{Project, TaskOpts};
use crate::source::Span;
use crate::value::{Value, equal};

/// The hidden global option that makes `ok` a worker for one dependency.
pub const WORKER_OPTION: &str = "--task-worker";

/// One run of a task in an invocation: the task and the arguments a
/// `needs` entry gives it. Two entries with the same task and the same
/// argument values are the same instance and run once.
pub struct Inst {
    pub task: usize,
    /// Given arguments in written order: (parameter, value, span of the
    /// value in the `needs` entry).
    pub args: Vec<(String, Value, Span)>,
    /// A `needs` entry that asks for this instance, as (declaring task,
    /// index in its list); a worker evaluates the arguments from it again.
    pub edge: (usize, usize),
    /// Instances that must finish first.
    pub deps: Vec<usize>,
    /// `build` or `build(release: true)`.
    pub label: String,
}

/// Every dependency of an invocation, `order` listing them depth first in
/// `needs` order (the order of a sequential run).
#[derive(Default)]
pub struct Plan {
    pub insts: Vec<Inst>,
    pub order: Vec<usize>,
}

fn same_args(a: &[(String, Value, Span)], b: &[(String, Value, Span)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(n, v, _)| b.iter().any(|(m, w, _)| m == n && equal(v, w)))
}

/// The task with each argument shown as a value: `build(release: true)`.
fn label(project: &Project, task: usize, args: &[(String, Value, Span)]) -> String {
    let name = project.tasks[task].qualified();
    if args.is_empty() {
        return name;
    }
    let shown: Vec<String> = args
        .iter()
        .map(|(n, v, _)| format!("{n}: {}", v.repr()))
        .collect();
    format!("{name}({})", shown.join(", "))
}

impl Interp {
    /// Evaluate and type-check the arguments of entry `k` of task `ti`'s
    /// `needs` (SPEC.md 15.5).
    fn need_args(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        ti: usize,
        k: usize,
    ) -> R<Vec<(String, Value, Span)>> {
        let t = &project.tasks[ti];
        let need = &t.decl.needs[k];
        let dep = &project.tasks[t.needs[k]];
        let mut out = Vec::new();
        for (name, _, expr) in &need.args {
            let v = self.eval(expr, &scopes[&t.file])?;
            let param = dep.decl.params.iter().find(|p| p.name == *name);
            let v = match param.and_then(|p| p.ty.as_ref()) {
                Some(ty) => check_type(v, ty).map_err(|got| {
                    OkError::at(
                        ErrKind::Type,
                        expr.span,
                        format!(
                            "argument `{name}` of task `{}` must be {}, got {got}",
                            dep.qualified(),
                            crate::interp::type_phrase(ty)
                        ),
                    )
                })?,
                None => v,
            };
            out.push((name.clone(), v, expr.span));
        }
        Ok(out)
    }

    /// The instances task `ti` needs, directly and indirectly.
    pub(crate) fn plan_needs(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        ti: usize,
    ) -> R<Plan> {
        let mut plan = Plan::default();
        self.plan_visit(project, scopes, ti, &mut plan)?;
        Ok(plan)
    }

    fn plan_visit(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        ti: usize,
        plan: &mut Plan,
    ) -> R<Vec<usize>> {
        let mut deps = Vec::new();
        for k in 0..project.tasks[ti].needs.len() {
            let task = project.tasks[ti].needs[k];
            let args = self.need_args(project, scopes, ti, k)?;
            if let Some(i) = plan
                .insts
                .iter()
                .position(|x| x.task == task && same_args(&x.args, &args))
            {
                if !deps.contains(&i) {
                    deps.push(i);
                }
                continue;
            }
            // Load-time cycle detection makes this recursion finite.
            let sub = self.plan_visit(project, scopes, task, plan)?;
            let i = plan.insts.len();
            plan.insts.push(Inst {
                task,
                label: label(project, task, &args),
                args,
                edge: (ti, k),
                deps: sub,
            });
            plan.order.push(i);
            deps.push(i);
        }
        Ok(deps)
    }

    /// Bind the arguments of a `needs` entry to the parameters of the task
    /// like a call would: flags by name, positional parameters by position
    /// (earlier positional parameters that are not given take their
    /// defaults).
    fn bind_need_args(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        inst: &Inst,
    ) -> R<Args> {
        let t = &project.tasks[inst.task];
        let span = project.tasks[inst.edge.0].decl.needs[inst.edge.1].span;
        let given = |name: &str| inst.args.iter().find(|(n, _, _)| n == name);
        let positional: Vec<&crate::ast::Param> = t
            .decl
            .params
            .iter()
            .filter(|p| !p.flag && !p.rest)
            .collect();
        let last = positional.iter().rposition(|p| given(&p.name).is_some());
        let mut pos = Vec::new();
        if let Some(last) = last {
            for p in &positional[..=last] {
                match given(&p.name) {
                    Some((_, v, _)) => pos.push(v.clone()),
                    None => match &p.default {
                        Some(d) => {
                            let scope = Scope::new(Some(scopes[&t.file].clone()));
                            pos.push(self.eval(d, &scope)?);
                        }
                        None => pos.push(Value::Null),
                    },
                }
            }
        }
        let named = t
            .decl
            .params
            .iter()
            .filter(|p| p.flag)
            .filter_map(|p| given(&p.name).map(|(n, v, s)| (n.clone(), v.clone(), *s)))
            .collect();
        Ok(Args { pos, named, span })
    }

    /// Run the planned dependencies: in parallel where the plan allows it
    /// and `--jobs` is above 1, else one after another in `plan.order`.
    pub(crate) fn run_needs(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        plan: &Plan,
        opts: &TaskOpts,
    ) -> R<()> {
        let n = plan.insts.len();
        // reach[i]: every instance that must finish before i starts.
        let mut reach: Vec<HashSet<usize>> = vec![HashSet::new(); n];
        for &i in &plan.order {
            let mut set = HashSet::new();
            for &d in &plan.insts[i].deps {
                set.insert(d);
                set.extend(reach[d].iter().copied());
            }
            reach[i] = set;
        }
        // An instance that is ordered against every other one can never
        // overlap with another, so it runs in this process.
        let alone: Vec<bool> = (0..n)
            .map(|i| (0..n).all(|j| j == i || reach[i].contains(&j) || reach[j].contains(&i)))
            .collect();
        let parallel = opts.jobs > 1 && !self.run_opts.dry_run && alone.iter().any(|a| !a);
        if !parallel {
            for &i in &plan.order {
                let args = self.bind_need_args(project, scopes, &plan.insts[i])?;
                self.run_one(project, scopes, plan.insts[i].task, args, !opts.force)?;
            }
            return Ok(());
        }

        let fingerprint = crate::trust::Inputs::of(project, opts.env.as_deref()).fingerprint()?;
        let width = (0..n)
            .filter(|&i| !alone[i])
            .map(|i| plan.insts[i].label.chars().count())
            .max()
            .unwrap_or(0);
        let (tx, rx) = mpsc::channel::<(usize, i32)>();
        let mut started = vec![false; n];
        let mut done = vec![false; n];
        let mut running = 0usize;
        let mut failures: Vec<(usize, i32)> = Vec::new();
        // An error in this process (not in a worker) stops starting tasks;
        // it is returned once the running workers have finished.
        let mut error = None;
        loop {
            let mut progressed = false;
            if failures.is_empty() && error.is_none() {
                for &i in &plan.order {
                    if started[i]
                        || running >= opts.jobs
                        || !plan.insts[i].deps.iter().all(|&d| done[d])
                    {
                        continue;
                    }
                    // Nothing else can be running when an instance that is
                    // ordered against all others is ready.
                    if alone[i] && running > 0 {
                        continue;
                    }
                    started[i] = true;
                    match self.start_inst(
                        project,
                        scopes,
                        plan,
                        i,
                        alone[i],
                        opts,
                        &fingerprint,
                        width,
                        &tx,
                    ) {
                        Ok(true) => running += 1,
                        Ok(false) => {
                            done[i] = true;
                            progressed = true;
                        }
                        Err(e) => {
                            error = Some(e);
                            break;
                        }
                    }
                }
            }
            if progressed && error.is_none() {
                continue;
            }
            if running == 0 {
                break;
            }
            let Ok((i, code)) = rx.recv() else { break };
            running -= 1;
            if code == 0 {
                done[i] = true;
            } else {
                failures.push((i, code));
            }
        }
        if let Some(e) = error {
            return Err(e);
        }
        let _ = std::env::set_current_dir(&project.root);
        if let Some(&(first, code)) = failures.first() {
            let mut e = OkError::new(
                ErrKind::Command,
                format!(
                    "task `{}` failed with exit code {code}",
                    plan.insts[first].label
                ),
            );
            e.exit_code = Some(code);
            if failures.len() > 1 {
                let others: Vec<String> = failures[1..]
                    .iter()
                    .map(|(i, c)| format!("{} (exit code {c})", plan.insts[*i].label))
                    .collect();
                e.details
                    .push(format!("also failed: {}", others.join(", ")));
            }
            e.details.push(format!(
                "its output is above, each line starting with `{} |`",
                plan.insts[first].label
            ));
            return Err(e.into());
        }
        Ok(())
    }

    /// Start instance `i`: run it here when it is `alone`, skip it when it is
    /// up to date, else start a worker. Returns true when a worker runs it.
    #[allow(clippy::too_many_arguments)]
    fn start_inst(
        &mut self,
        project: &Project,
        scopes: &HashMap<usize, Rc<Scope>>,
        plan: &Plan,
        i: usize,
        alone: bool,
        opts: &TaskOpts,
        fingerprint: &str,
        width: usize,
        tx: &mpsc::Sender<(usize, i32)>,
    ) -> R<bool> {
        let inst = &plan.insts[i];
        if alone {
            let args = self.bind_need_args(project, scopes, inst)?;
            self.run_one(project, scopes, inst.task, args, !opts.force)?;
            return Ok(false);
        }
        let t = &project.tasks[inst.task];
        let run_dir = &project.files[t.file].run_dir;
        if !opts.force && self.up_to_date(t, &scopes[&t.file], run_dir)? {
            eprintln!("{}: up to date", inst.label);
            return Ok(false);
        }
        let prefix = format!("{:<width$} | ", inst.label);
        self.start_worker(project, plan, i, fingerprint, opts, prefix, tx.clone())?;
        Ok(true)
    }

    /// Start a worker process for instance `i`. A thread forwards its output
    /// line by line with `prefix`, waits for it and sends `(i, exit code)`.
    #[allow(clippy::too_many_arguments)]
    fn start_worker(
        &self,
        project: &Project,
        plan: &Plan,
        i: usize,
        fingerprint: &str,
        opts: &TaskOpts,
        prefix: String,
        tx: mpsc::Sender<(usize, i32)>,
    ) -> R<()> {
        let inst = &plan.insts[i];
        let task = project.tasks[inst.task].qualified();
        // The `needs` entry is always passed so the worker calls the task
        // from that entry, as a sequential run does: its arguments and its
        // span (the "called at" note of an error) come from there.
        let (from, k) = inst.edge;
        let spec = format!(
            "{fingerprint}:{task}:{}:{k}",
            project.tasks[from].qualified()
        );
        let exe = std::env::current_exe().map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!(
                    "cannot find the ok program to run `{}` in parallel: {e}",
                    inst.label
                ),
            )
            .with_help("run with --jobs 1 to run dependencies one at a time")
        })?;
        let mut c = Command::new(exe);
        if self.run_opts.trace {
            c.arg("--trace");
        }
        if self.run_opts.yes {
            c.arg("--yes");
        }
        if opts.trust {
            c.arg("--trust");
        }
        if let Some(env) = &opts.env {
            c.arg("--env").arg(env);
        }
        c.arg("--color")
            .arg(if opts.color { "always" } else { "never" })
            .arg("-C")
            .arg(&project.root)
            .arg(WORKER_OPTION)
            .arg(spec)
            .current_dir(&self.invocation_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // Environment changes made so far (env files, earlier tasks) reach
        // the worker as real variables.
        for (k, v) in &self.env_overlay {
            match v {
                Some(v) => {
                    c.env(k, v);
                }
                None => {
                    c.env_remove(k);
                }
            }
        }
        flush_stdout();
        let mut child = c.spawn().map_err(|e| {
            OkError::new(
                ErrKind::Io,
                format!("cannot start a worker for `{}`: {e}", inst.label),
            )
            .with_help("run with --jobs 1 to run dependencies one at a time")
        })?;
        let out = child.stdout.take();
        let err = child.stderr.take();
        std::thread::spawn(move || {
            let p = prefix.clone();
            let out_thread = out.map(|o| std::thread::spawn(move || forward(o, &p, false)));
            let err_thread = err.map(|e| {
                let p = prefix.clone();
                std::thread::spawn(move || forward(e, &p, true))
            });
            let code = child
                .wait()
                .map(|s| crate::command::exit_code(&s))
                .unwrap_or(1);
            for t in [out_thread, err_thread].into_iter().flatten() {
                let _ = t.join();
            }
            let _ = tx.send((i, code));
        });
        Ok(())
    }

    /// The worker side: run the one instance that `spec` names. The parent
    /// asked for trust and confirmations; the worker checks trust again
    /// without asking (the parent's approval is in the store, or `--trust`
    /// is passed on) and refuses to run if the project files changed since
    /// the parent read them (their fingerprint differs).
    pub fn run_worker(&mut self, project: &Project, spec: &str, opts: &TaskOpts) -> R<Value> {
        let bad = || {
            OkError::new(
                ErrKind::Arg,
                format!("invalid {WORKER_OPTION} value {spec:?}"),
            )
        };
        let parts: Vec<&str> = spec.split(':').collect();
        let (fp, task, from, k) = match parts.as_slice() {
            [fp, task, from, k] => (*fp, *task, *from, k.parse::<usize>().map_err(|_| bad())?),
            _ => return Err(bad().into()),
        };
        crate::trust::ensure_trusted(project, opts.trust, opts.env.as_deref())?;
        if crate::trust::Inputs::of(project, opts.env.as_deref()).fingerprint()? != fp {
            return Err(OkError::new(
                ErrKind::Trust,
                "the project files changed while the task was running",
            )
            .with_help("run the task again")
            .into());
        }
        let find = |q: &str| project.tasks.iter().position(|t| t.qualified() == q);
        let ti = find(task).ok_or_else(bad)?;
        self.run_opts.parallel = true;
        let scopes = self.enter_project(project, opts)?;
        let from = find(from).ok_or_else(bad)?;
        if project.tasks[from].needs.get(k) != Some(&ti) {
            return Err(bad().into());
        }
        let args = self.need_args(project, &scopes, from, k)?;
        let inst = Inst {
            task: ti,
            label: String::new(),
            args,
            edge: (from, k),
            deps: Vec::new(),
        };
        let args = self.bind_need_args(project, &scopes, &inst)?;
        self.run_one(project, &scopes, ti, args, false)?;
        let _ = std::env::set_current_dir(&project.root);
        Ok(Value::Null)
    }
}

/// The longest piece of a worker's output line that `forward` holds; a
/// longer line (a progress bar without newlines, a minified file) goes out
/// in pieces of this size, each on its own prefixed line, instead of
/// growing ok's memory without bound.
const FORWARD_CHUNK: u64 = 64 * 1024;

/// Copy a worker's output to ok's stdout or stderr, one whole line at a
/// time with `prefix` in front, so lines of parallel tasks never mix.
fn forward(r: impl Read, prefix: &str, to_stderr: bool) {
    prefixed_lines(r, prefix, |line| {
        if to_stderr {
            let _ = std::io::stderr().lock().write_all(line);
        } else {
            let mut o = std::io::stdout().lock();
            let _ = o.write_all(line);
            let _ = o.flush();
        }
    });
}

/// Each line of `r` (at most [`FORWARD_CHUNK`] bytes of it at a time) with
/// `prefix` in front and a newline at the end.
fn prefixed_lines(r: impl Read, prefix: &str, mut emit: impl FnMut(&[u8])) {
    let mut reader = BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut reader)
            .take(FORWARD_CHUNK)
            .read_until(b'\n', &mut buf)
        {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if !buf.ends_with(b"\n") {
            buf.push(b'\n');
        }
        let mut line = Vec::with_capacity(prefix.len() + buf.len());
        line.extend_from_slice(prefix.as_bytes());
        line.extend_from_slice(&buf);
        emit(&line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_worker_lines_are_forwarded_in_pieces() {
        let long = "x".repeat(FORWARD_CHUNK as usize * 2 + 10);
        let input = format!("short\n{long}\nend");
        let mut got: Vec<Vec<u8>> = Vec::new();
        prefixed_lines(input.as_bytes(), "[t] ", |l| got.push(l.to_vec()));
        let sizes: Vec<usize> = got.iter().map(Vec::len).collect();
        let piece = 4 + FORWARD_CHUNK as usize + 1;
        assert_eq!(sizes, [10, piece, piece, 4 + 10 + 1, 8]);
        assert_eq!(got[0], b"[t] short\n");
        assert_eq!(got[4], b"[t] end\n");
    }
}
