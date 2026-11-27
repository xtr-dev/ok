//! The `ok` command line (SPEC.md section 2).
//!
//! Global options come before the script or task name. Everything after the
//! name belongs to the script or task. [`main`] parses the options and then
//! runs one of: `-e` source, a task worker, `ok self`, `ok trust` or
//! `ok untrust`, a script file, or a task of the discovered project.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use crate::error::{Color, ErrKind, OkError, render, suggest};
use crate::interp::{Flow, Interp, RunOpts, flush_stdout};
use crate::source::SourceMap;
use crate::value::Value;

const USAGE: &str = "\
ok: a portable scripting language and task runner

Usage:
  ok [OPTIONS] SCRIPT [ARGS...]   run a script (a path ending in .ok, or containing /)
  ok [OPTIONS] -e SOURCE          evaluate SOURCE and print the result
  ok [OPTIONS]                    list the tasks of the project
  ok [OPTIONS] TASK [ARGS...]     run a project task (ok NAMESPACE TASK for .ok/NAMESPACE.ok)
  ok trust | untrust [DIR]        manage which projects may run tasks
  ok self COMMAND                 init, promote, completions, fmt, check, tasks, version

Options (before the script or task name):
  -C DIR          discover the project from DIR
  -e SOURCE       evaluate SOURCE
  --trace         print commands before running them
  --dry-run       print commands and file changes instead of performing them
  --yes           answer yes to confirmations
  --trust         run project tasks without a trust prompt
  --force         run tasks even when they are up to date
  --env NAME      also load .env.NAME
  -j, --jobs N    run at most N dependencies at the same time (1: one by one)
  --color WHEN    auto, always or never
  -h, --help      print this help
  --version       print the version
";

/// `ok self --help` (SPEC.md 17).
const SELF_USAGE: &str = "\
ok self: ok's built-in tools

Usage:
  ok self init [--wrappers]            write a starter .ok/ok.ok (and the ./ok wrappers)
  ok self promote                      move a .ok file to .ok/ok.ok and ok/ to .ok/
  ok self completions SHELL            print a completion script: bash, zsh, fish or powershell
  ok self complete -- WORDS...         print completion candidates (for completion scripts)
  ok self fmt [--check] [FILES...]     format files in the canonical layout
  ok self check [--strict] [FILES...]  check files without running them
  ok self tasks [--all]                list the tasks; --all includes hidden ones
  ok self version                      print the version
";

/// `ok trust --help` and `ok untrust --help` (SPEC.md 16.5).
pub(crate) const TRUST_USAGE: &str = "\
ok trust: manage which projects may run tasks

Usage:
  ok trust [DIR]          trust the current content of the project at DIR (default: discovered)
  ok trust --dir [DIR]    trust the directory, whatever its content (for your own projects)
  ok trust --list         list trusted directories and fingerprints
  ok untrust [DIR]        remove trust for the project at DIR
";

/// Global options (SPEC.md 2.2).
#[derive(Default)]
#[allow(dead_code)]
struct Opts {
    dir: Option<String>,
    eval: Option<String>,
    trace: bool,
    dry_run: bool,
    yes: bool,
    trust: bool,
    force: bool,
    env: Option<String>,
    color: Option<String>,
    jobs: Option<usize>,
    /// The hidden `--task-worker` value: run one dependency for a parent
    /// ok (SPEC.md 15.5).
    worker: Option<String>,
}

const OPTION_NAMES: &[&str] = &[
    "-C",
    "-e",
    "--trace",
    "--dry-run",
    "--yes",
    "--trust",
    "--force",
    "--env",
    "--color",
    "--jobs",
    "--help",
    "--version",
];

/// Run `ok` with the given arguments (without the program name) and return
/// the process exit code.
pub fn main(args: Vec<String>) -> i32 {
    let code = run(args);
    flush_stdout();
    code
}

fn usage_error(msg: &str, help: Option<String>) -> i32 {
    let mut e = OkError::new(ErrKind::Arg, msg);
    e.help = help;
    eprint!("{}", render(&e, &SourceMap::default(), Color::Never));
    eprintln!("run `ok --help` for usage");
    2
}

fn run(args: Vec<String>) -> i32 {
    let mut opts = Opts::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if !a.starts_with('-') || a == "-" {
            break;
        }
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a, None),
        };
        let value = |i: &mut usize| -> Option<String> {
            if let Some(v) = &inline {
                return Some(v.clone());
            }
            *i += 1;
            args.get(*i).cloned()
        };
        match flag {
            "-h" | "--help" => {
                print!("{USAGE}");
                return 0;
            }
            "--version" => {
                println!("ok {}", crate::VERSION);
                return 0;
            }
            "--trace" => opts.trace = true,
            "--dry-run" => opts.dry_run = true,
            "--yes" => opts.yes = true,
            "--trust" => opts.trust = true,
            "--force" => opts.force = true,
            _ if flag.starts_with("-j")
                && flag.len() > 2
                && flag[2..].bytes().all(|b| b.is_ascii_digit()) =>
            {
                match parse_jobs(&flag[2..]) {
                    Ok(n) => opts.jobs = Some(n),
                    Err(code) => return code,
                }
            }
            "-C" | "-e" | "--env" | "--color" | "-j" | "--jobs" | crate::jobs::WORKER_OPTION => {
                let Some(v) = value(&mut i) else {
                    return usage_error(&format!("option {flag} needs a value"), None);
                };
                match flag {
                    "-C" => opts.dir = Some(v),
                    "-e" => opts.eval = Some(v),
                    "--env" => opts.env = Some(v),
                    "-j" | "--jobs" => match parse_jobs(&v) {
                        Ok(n) => opts.jobs = Some(n),
                        Err(code) => return code,
                    },
                    crate::jobs::WORKER_OPTION => opts.worker = Some(v),
                    _ => {
                        if !matches!(v.as_str(), "auto" | "always" | "never") {
                            return usage_error(
                                &format!("--color must be auto, always or never, got {v}"),
                                None,
                            );
                        }
                        opts.color = Some(v);
                    }
                }
            }
            other => {
                let help = suggest(other, OPTION_NAMES.iter().copied())
                    .map(|s| format!("did you mean {s}?"));
                return usage_error(&format!("unknown option {other}"), help);
            }
        }
        i += 1;
    }
    let rest: Vec<String> = args[i.min(args.len())..].to_vec();
    let color = pick_color(opts.color.as_deref());

    if let Some(source) = opts.eval.clone() {
        let mut interp = new_interp(rest, &opts);
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = interp.run_source("<-e>", &source, base);
        return finish(&mut interp, result, color, true);
    }

    let start = match &opts.dir {
        Some(d) => PathBuf::from(d),
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    if opts.dir.as_ref().is_some_and(|d| !Path::new(d).is_dir()) {
        return usage_error(
            &format!(
                "-C {}: no such directory",
                opts.dir.as_deref().unwrap_or("")
            ),
            None,
        );
    }
    if let Some(spec) = &opts.worker {
        let Some(root) = crate::project::discover(&start) else {
            return usage_error("no project found for the task worker", None);
        };
        // The parent ok already warned about anything deprecated.
        crate::project::quiet_deprecations();
        let mut interp = new_interp(Vec::new(), &opts);
        let task_opts = task_opts(&opts, color);
        let result = interp
            .load_project(&root)
            .and_then(|p| interp.run_worker(&p, spec, &task_opts));
        return finish(&mut interp, result, color, false);
    }
    let Some(name) = rest.first().cloned() else {
        // `ok` alone: the task listing inside a project, usage outside.
        if let Some(root) = crate::project::discover(&start) {
            let mut interp = new_interp(Vec::new(), &opts);
            let result = interp.load_project(&root).map(|p| {
                print!("{}", p.listing(false, None));
                Value::Null
            });
            return finish(&mut interp, result, color, false);
        }
        print!("{USAGE}");
        return 2;
    };
    let script_args = rest[1..].to_vec();
    match name.as_str() {
        "self" => return self_command(&script_args, &opts, &start, color),
        "trust" | "untrust" => {
            let mut interp = new_interp(Vec::new(), &opts);
            let result = crate::trust::command(
                name == "untrust",
                opts.env.as_deref(),
                &script_args,
                &start,
                &mut interp,
            )
            .map(|_| Value::Null);
            return finish(&mut interp, result, color, false);
        }
        _ => {}
    }
    let looks_like_script =
        name.ends_with(".ok") || name.contains('/') || (cfg!(windows) && name.contains('\\'));
    let path = Path::new(&name);
    if looks_like_script {
        if !path.is_file() {
            let what = if path.is_dir() {
                "is a directory"
            } else {
                "does not exist"
            };
            return usage_error(&format!("script {name} {what}"), None);
        }
        let mut interp = new_interp(script_args, &opts);
        let result = interp.run_file(&name, path);
        return finish(&mut interp, result, color, false);
    }
    let Some(root) = crate::project::discover(&start) else {
        return usage_error(
            &format!(
                "no project found in {} or its parents ({}), so `{name}` is not a task",
                crate::path::OkPath::from_native(&start).display(),
                crate::project::ROOT_FORMS
            ),
            Some(format!(
                "to run a script, give its path: ok ./{name} or ok {name}.ok; to start a project, run ok self init"
            )),
        );
    };
    let mut interp = new_interp(Vec::new(), &opts);
    let task_opts = task_opts(&opts, color);
    let result = interp
        .load_project(&root)
        .and_then(|p| interp.run_task(&p, &rest, &task_opts));
    finish(&mut interp, result, color, false)
}

fn task_opts(opts: &Opts, color: Color) -> crate::project::TaskOpts {
    crate::project::TaskOpts {
        force: opts.force,
        env: opts.env.clone(),
        trust: opts.trust,
        all: false,
        jobs: opts.jobs.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        }),
        color: color == Color::Always,
    }
}

/// The value of `--jobs`, a whole number of at least 1. Anything else prints
/// a usage error, and the `Err` holds the exit code for it.
fn parse_jobs(v: &str) -> Result<usize, i32> {
    match v.parse::<usize>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(usage_error(
            &format!("--jobs needs a number of at least 1, got {v}"),
            Some("--jobs 1 runs dependencies one at a time".into()),
        )),
    }
}

fn self_command(args: &[String], opts: &Opts, start: &Path, color: Color) -> i32 {
    let rest = args.get(1..).unwrap_or(&[]);
    let mut interp = new_interp(Vec::new(), opts);
    let result: Result<Value, Flow> = match args.first().map(String::as_str) {
        Some("-h" | "--help") => {
            print!("{SELF_USAGE}");
            return 0;
        }
        Some("version") => {
            println!("ok {}", crate::VERSION);
            Ok(Value::Null)
        }
        Some("tasks") => {
            let all = rest.iter().any(|a| a == "--all");
            match crate::project::discover(start) {
                Some(root) => interp.load_project(&root).map(|p| {
                    print!("{}", p.listing(all, None));
                    Value::Null
                }),
                None => {
                    return usage_error(
                        &format!(
                            "no project found in this directory or its parents ({})",
                            crate::project::ROOT_FORMS
                        ),
                        None,
                    );
                }
            }
        }
        Some("completions") => match rest
            .first()
            .map(String::as_str)
            .and_then(crate::project::completion_script)
        {
            Some(script) => {
                print!("{script}");
                Ok(Value::Null)
            }
            None => {
                return usage_error(
                    "`ok self completions` needs a shell: bash, zsh, fish or powershell",
                    None,
                );
            }
        },
        Some("complete") => {
            let words: Vec<String> = rest.iter().skip_while(|w| *w == "--").cloned().collect();
            // Completion scripts show stderr in the user's prompt.
            crate::project::quiet_deprecations();
            let candidates = match crate::project::discover(start)
                .and_then(|root| interp.load_project(&root).ok())
            {
                Some(p) => p.complete(&words),
                None => {
                    let current = words.last().map(String::as_str).unwrap_or("");
                    ["self", "trust", "untrust"]
                        .iter()
                        .filter(|c| c.starts_with(current))
                        .map(|c| c.to_string())
                        .collect()
                }
            };
            for c in candidates {
                println!("{c}");
            }
            Ok(Value::Null)
        }
        Some("check") => crate::tools::check(&mut interp, rest, start),
        Some("fmt") => crate::tools::fmt(&mut interp, rest, start),
        Some("init") => crate::tools::init(rest, start),
        Some("promote") => crate::tools::promote(rest, start),
        Some(other) => {
            let names = [
                "init",
                "promote",
                "completions",
                "fmt",
                "check",
                "tasks",
                "version",
            ];
            let help = suggest(other, names).map(|s| format!("did you mean `ok self {s}`?"));
            return usage_error(&format!("unknown command `ok self {other}`"), help);
        }
        None => {
            return usage_error(
                "`ok self` needs a command: init, promote, completions, fmt, check, tasks or version",
                None,
            );
        }
    };
    finish(&mut interp, result, color, false)
}

fn pick_color(choice: Option<&str>) -> Color {
    match choice {
        Some("always") => Color::Always,
        Some("never") => Color::Never,
        _ => {
            let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
            if !no_color && std::io::stderr().is_terminal() {
                Color::Always
            } else {
                Color::Never
            }
        }
    }
}

fn new_interp(args: Vec<String>, opts: &Opts) -> Interp {
    let mut interp = Interp::new(args);
    interp.run_opts = RunOpts {
        trace: opts.trace,
        dry_run: opts.dry_run,
        yes: opts.yes,
        parallel: false,
    };
    interp
}

fn finish(
    interp: &mut Interp,
    result: Result<Value, Flow>,
    color: Color,
    print_value: bool,
) -> i32 {
    // The value of `-e` may be lazy and still read from temporary files, so
    // it is materialised before `cleanup` removes them.
    let result = match result {
        Ok(v) if print_value => {
            let span = crate::source::Span::default();
            crate::lazy::force(interp, v, span)
        }
        other => other,
    };
    interp.cleanup();
    match result {
        Ok(v) => {
            if print_value && !matches!(v, Value::Null) {
                println!("{}", v.display());
            }
            0
        }
        Err(Flow::Exit(code)) => code,
        Err(Flow::Err(e)) => {
            flush_stdout();
            eprint!("{}", render(&e, &interp.sources, color));
            e.exit_code.unwrap_or(1)
        }
        Err(Flow::Return(_) | Flow::Break | Flow::Continue) => 1,
    }
}
