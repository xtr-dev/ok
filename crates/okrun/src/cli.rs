//! The `ok` command line (SPEC.md section 2).
//!
//! Global options come before the script or task name; everything after the
//! name belongs to the script. Project mode, trust and most `ok self`
//! commands arrive in later milestones and say so when used.

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};

use crate::error::{Color, ErrKind, OkError, render, suggest};
use crate::interp::{Flow, Interp, flush_stdout};
use crate::source::SourceMap;
use crate::value::Value;

const USAGE: &str = "\
ok: a portable scripting language and task runner

Usage:
  ok [OPTIONS] SCRIPT [ARGS...]   run a script (a path ending in .ok, or containing /)
  ok [OPTIONS] -e SOURCE          evaluate SOURCE and print the result
  ok [OPTIONS] TASK [ARGS...]     run a project task (not implemented yet: milestone M5)
  ok self version                 print the version

Options (before the script or task name):
  -C DIR          discover the project from DIR
  -e SOURCE       evaluate SOURCE
  --trace         print commands before running them
  --dry-run       print commands and file changes instead of performing them
  --yes           answer yes to confirmations
  --trust         run project tasks without a trust prompt
  --force         run tasks even when they are up to date
  --env NAME      also load .env.NAME
  --color WHEN    auto, always or never
  -h, --help      print this help
  --version       print the version
";

/// Global options (SPEC.md 2.2). Several only take effect in later
/// milestones; they are parsed now so the command line is stable.
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
            "-C" | "-e" | "--env" | "--color" => {
                let Some(v) = value(&mut i) else {
                    return usage_error(&format!("option {flag} needs a value"), None);
                };
                match flag {
                    "-C" => opts.dir = Some(v),
                    "-e" => opts.eval = Some(v),
                    "--env" => opts.env = Some(v),
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
        let mut interp = Interp::new(rest);
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let result = interp.run_source("<-e>", &source, base);
        return finish(&interp, result, color, true);
    }

    let Some(name) = rest.first().cloned() else {
        print!("{USAGE}");
        return 2;
    };
    let script_args = rest[1..].to_vec();
    match name.as_str() {
        "self" => return self_command(&script_args),
        "trust" | "untrust" => {
            eprintln!("error: `ok {name}` is not implemented yet (milestone M6)");
            return 1;
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
        let mut interp = Interp::new(script_args);
        let result = interp.run_file(&name, path);
        return finish(&interp, result, color, false);
    }
    usage_error(
        &format!(
            "`{name}` is not a script, and running project tasks is not implemented yet (milestone M5)"
        ),
        Some(format!(
            "to run a script, give its path: ok ./{name} or ok {name}.ok"
        )),
    )
}

fn self_command(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("version") => {
            println!("ok {}", crate::VERSION);
            0
        }
        Some(cmd @ ("init" | "completions" | "complete" | "fmt" | "check" | "tasks")) => {
            let milestone = match cmd {
                "init" => "M7",
                "fmt" | "check" => "M5",
                _ => "M5",
            };
            eprintln!("error: `ok self {cmd}` is not implemented yet (milestone {milestone})");
            1
        }
        Some(other) => {
            let names = ["init", "completions", "fmt", "check", "tasks", "version"];
            let help = suggest(other, names).map(|s| format!("did you mean `ok self {s}`?"));
            usage_error(&format!("unknown command `ok self {other}`"), help)
        }
        None => usage_error(
            "`ok self` needs a command: init, completions, fmt, check, tasks or version",
            None,
        ),
    }
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

fn finish(interp: &Interp, result: Result<Value, Flow>, color: Color, print_value: bool) -> i32 {
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
