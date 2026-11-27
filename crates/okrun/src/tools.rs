//! `ok self check`, `ok self fmt`, `ok self init` and `ok self promote`
//! (SPEC.md 17), and the templates of the bootstrap wrappers that
//! `ok self init --wrappers` writes (SPEC.md 18).

use std::path::{Path, PathBuf};

use crate::error::{Color, ErrKind, OkError, render};
use crate::interp::{FileKind, Flow, Interp, R, check_items};
use crate::path::OkPath;
use crate::value::Value;

/// The release targets: each target triple with the file name of its binary
/// in a release, which is also its name in `SHA256SUMS`.
pub const TARGETS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-musl", "ok-x86_64-unknown-linux-musl"),
    (
        "aarch64-unknown-linux-musl",
        "ok-aarch64-unknown-linux-musl",
    ),
    ("x86_64-apple-darwin", "ok-x86_64-apple-darwin"),
    ("aarch64-apple-darwin", "ok-aarch64-apple-darwin"),
    ("x86_64-pc-windows-msvc", "ok-x86_64-pc-windows-msvc.exe"),
    ("aarch64-pc-windows-msvc", "ok-aarch64-pc-windows-msvc.exe"),
];

fn usage(msg: &str) -> Flow {
    let mut e = OkError::new(ErrKind::Arg, msg);
    e.exit_code = Some(2);
    e.into()
}

/// The kind of an ok file judged by its place (SPEC.md 17.1): a root file
/// (`ok.ok`, `.ok`, `.ok/ok.ok`) is a project root; a file directly in the
/// `.ok/` (or deprecated `ok/`) directory of a project root is a namespace,
/// or a library when its name starts with `_`; anything else is a script.
fn kind_of(path: &Path) -> FileKind {
    if path
        .file_name()
        .is_some_and(|n| crate::project::is_root_name(&n.to_string_lossy()))
    {
        return FileKind::Project { root: true };
    }
    // `db.ok` given from inside `.ok/` has no parent to look at until the
    // path is made absolute.
    let full = crate::path::canonical(path).unwrap_or_else(|_| path.to_path_buf());
    let parent = full.parent();
    let in_ok_dir = parent
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == ".ok" || n == "ok");
    let has_root = parent
        .and_then(|p| p.parent())
        .is_some_and(|pp| !crate::project::root_files_in(pp).is_empty());
    if in_ok_dir && has_root {
        let helper = path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('_'));
        return if helper {
            FileKind::Library
        } else {
            FileKind::Project { root: false }
        };
    }
    FileKind::Script
}

/// Split the arguments of `ok self check` / `ok self fmt` into the allowed
/// options and the files.
fn options_and_files(
    args: &[String],
    allowed: &str,
    cmd: &str,
) -> Result<(bool, Vec<PathBuf>), Flow> {
    let mut flag = false;
    let mut files = Vec::new();
    for a in args {
        if a == allowed {
            flag = true;
        } else if a.starts_with('-') && a.len() > 1 {
            return Err(usage(&format!(
                "unknown option {a} for ok self {cmd} (it takes {allowed} and files)"
            )));
        } else {
            files.push(PathBuf::from(a));
        }
    }
    Ok((flag, files))
}

/// Read a file to check or format and register it in the source map, so
/// that findings can show its lines.
fn read_source(interp: &mut Interp, shown: &str, path: &Path) -> Result<(u32, String), OkError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        OkError::new(
            ErrKind::Io,
            format!("cannot read {shown}: {}", crate::interp::io_reason(&e)),
        )
    })?;
    let id = interp.sources.add(
        shown.to_string(),
        Some(path.to_path_buf()),
        text.clone().into(),
    );
    Ok((id, text))
}

/// Like [`read_source`], but the source map gets the text the formatter
/// works on: `\n` line endings, no byte order mark. Returns the id, the
/// text as read and the normalised text.
fn read_for_format(
    interp: &mut Interp,
    shown: &str,
    path: &Path,
) -> Result<(u32, String, String), OkError> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        OkError::new(
            ErrKind::Io,
            format!("cannot read {shown}: {}", crate::interp::io_reason(&e)),
        )
    })?;
    let norm = crate::fmt::normalize(&text);
    let id = interp.sources.add(
        shown.to_string(),
        Some(path.to_path_buf()),
        norm.clone().into(),
    );
    Ok((id, text, norm))
}

/// `ok self check [--strict] [FILES...]`: parse and check without running
/// anything (SPEC.md 17). Errors exit with 1; warnings only with
/// `--strict`.
pub fn check(interp: &mut Interp, args: &[String], start: &Path) -> R<Value> {
    let (strict, files) = options_and_files(args, "--strict", "check")?;
    // (shown name, path, kind)
    let mut targets: Vec<(String, PathBuf, FileKind)> = Vec::new();
    let mut tasks = None;
    if files.is_empty() {
        let Some(root) = crate::project::discover(start) else {
            return Err(usage(&format!(
                "no project found here ({}); pass the files to check: ok self check FILE...",
                crate::project::ROOT_FORMS
            )));
        };
        // A load-time error (SPEC.md 15, 16.6) stops the check, since the
        // project's files are only known once it loads; it is reported
        // like a finding, with a summary line.
        let project = match interp.load_project(&root) {
            Ok(p) => p,
            Err(Flow::Err(e)) => {
                eprint!("{}", render(&e, &interp.sources, Color::Never));
                eprintln!("ok: the project failed to load: 1 error");
                return Err(Flow::Exit(1));
            }
            Err(other) => return Err(other),
        };
        tasks = Some(project.tasks.len());
        for (rel, path) in &project.trust_files {
            // Files a project imports are libraries (SPEC.md 11).
            let kind = match kind_of(path) {
                FileKind::Script => FileKind::Library,
                k => k,
            };
            targets.push((rel.clone(), path.clone(), kind));
        }
    } else {
        for f in &files {
            let shown = OkPath::from_native(f).display();
            targets.push((shown, f.clone(), kind_of(f)));
        }
    }
    let (mut errors, mut warnings) = (0, 0);
    for (shown, path, kind) in &targets {
        let findings = match read_source(interp, shown, path) {
            Err(e) => vec![e],
            Ok((id, text)) => match crate::parser::parse(id, &text)
                .and_then(|prog| check_items(&prog, *kind).map(|()| prog))
            {
                Err(e) => vec![e],
                Ok(prog) => crate::check::check_file(&prog, &text, shown, path, *kind),
            },
        };
        for e in &findings {
            if e.warning {
                warnings += 1;
            } else {
                errors += 1;
            }
            eprint!("{}", render(e, &interp.sources, Color::Never));
        }
    }
    let n = targets.len();
    let what = match tasks {
        Some(t) => format!("{n} file{} and {t} task{} checked", plural(n), plural(t)),
        None => format!("{n} file{} checked", plural(n)),
    };
    if errors == 0 && warnings == 0 {
        println!("ok: {what}, no problems found");
        return Ok(Value::Null);
    }
    let mut found = Vec::new();
    if errors > 0 {
        found.push(format!("{errors} error{}", plural(errors)));
    }
    if warnings > 0 {
        found.push(format!("{warnings} warning{}", plural(warnings)));
    }
    eprintln!("ok: {what}: {}", found.join(" and "));
    if errors > 0 || strict {
        return Err(Flow::Exit(1));
    }
    Ok(Value::Null)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// `ok self fmt [--check] [FILES...]` (SPEC.md 17.2). Without files it
/// formats every file of the project. With `--check` it writes nothing and
/// exits with 1 when a file would change.
pub fn fmt(interp: &mut Interp, args: &[String], start: &Path) -> R<Value> {
    let (check_only, mut files) = options_and_files(args, "--check", "fmt")?;
    if files.is_empty() {
        let Some(root) = crate::project::discover(start) else {
            return Err(usage(&format!(
                "no project found here ({}); pass the files to format: ok self fmt FILE...",
                crate::project::ROOT_FORMS
            )));
        };
        let project = interp.load_project(&root)?;
        files = project.trust_files.iter().map(|(_, p)| p.clone()).collect();
    }
    let mut changed = Vec::new();
    let mut failed = 0;
    for f in &files {
        let shown = OkPath::from_native(f).display();
        let result = read_for_format(interp, &shown, f).and_then(|(id, text, norm)| {
            crate::fmt::format_file(id, &norm).map(|formatted| (text, formatted))
        });
        let (text, formatted) = match result {
            Ok(r) => r,
            Err(mut e) => {
                if e.span.is_none() {
                    e.message = format!("{shown}: {}", e.message);
                }
                eprint!("{}", render(&e, &interp.sources, Color::Never));
                failed += 1;
                continue;
            }
        };
        if formatted != text {
            changed.push(shown.clone());
            if !check_only {
                std::fs::write(f, formatted).map_err(|e| {
                    OkError::new(
                        ErrKind::Io,
                        format!("cannot write {shown}: {}", crate::interp::io_reason(&e)),
                    )
                })?;
                println!("formatted {}", crate::error::terminal_safe_line(&shown));
            }
        }
    }
    if check_only {
        for c in &changed {
            println!("would reformat {}", crate::error::terminal_safe_line(c));
        }
    }
    if failed > 0 || (check_only && !changed.is_empty()) {
        return Err(Flow::Exit(1));
    }
    if changed.is_empty() {
        println!(
            "ok: {} file{} already formatted",
            files.len(),
            plural(files.len())
        );
    }
    Ok(Value::Null)
}

/// The starter `.ok/ok.ok`. `{requires}` is replaced by the running
/// version's major.minor, the oldest ok the file is known to work with
/// (SPEC.md 15.7).
const STARTER: &str = r#"requires "{requires}"

# Say hello
task hello(name: string = "world") {
  print("hello, {name}!")
}
"#;

/// `0.2` for `0.2.0`: the form `requires` takes.
fn major_minor(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// `ok self init [--wrappers [--checksums FILE]]`: a starter `.ok/ok.ok`,
/// and with `--wrappers` the bootstrap wrappers, which pin this version and
/// the checksums of its release (SPEC.md 17, 18).
pub fn init(args: &[String], start: &Path) -> R<Value> {
    let mut sums_file: Option<String> = None;
    let mut wrappers = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--wrappers" => wrappers = true,
            "--checksums" => {
                i += 1;
                sums_file = Some(
                    args.get(i)
                        .cloned()
                        .ok_or_else(|| usage("--checksums needs a SHA256SUMS file"))?,
                );
            }
            other => {
                return Err(usage(&format!(
                    "unknown option {other} for ok self init (it takes --wrappers and --checksums FILE)"
                )));
            }
        }
        i += 1;
    }
    if sums_file.is_some() && !wrappers {
        let mut e = OkError::new(
            ErrKind::Arg,
            "--checksums is only used for the wrappers, which ok self init writes with --wrappers",
        )
        .with_help("run ok self init --wrappers --checksums FILE");
        e.exit_code = Some(2);
        return Err(e.into());
    }
    let dir = start.to_path_buf();
    // Everything is checked before anything is written or downloaded.
    // An existing project keeps its root file; with `--wrappers` it gets
    // the wrappers, which is the way to add them later (SPEC.md 17).
    let existing = crate::project::root_files_in(&dir).first().copied();
    if let Some(existing) = existing
        && !wrappers
    {
        return Err(OkError::new(
            ErrKind::Io,
            format!("{existing} already exists; ok self init does not overwrite files"),
        )
        .with_help("this directory is already a project: add tasks to its root file, or run ok self init --wrappers to add the ./ok wrappers")
        .into());
    }
    let version = crate::VERSION;
    let starter = STARTER.replace("{requires}", &major_minor(version));
    let mut outputs: Vec<(&str, String)> = Vec::new();
    if existing.is_none() {
        outputs.push((".ok/ok.ok", starter));
    }
    if wrappers {
        for name in ["ok", "ok.ps1", "ok.cmd"] {
            let p = dir.join(name);
            if p.is_dir() {
                return Err(OkError::new(
                    ErrKind::Io,
                    format!("{name}/ is a directory, so the ./{name} wrapper cannot be written"),
                )
                .with_help("namespaces belong in .ok/; move the directory, or run ok self init without --wrappers")
                .into());
            }
            if std::fs::symlink_metadata(&p).is_ok() {
                return Err(OkError::new(
                    ErrKind::Io,
                    format!("{name} already exists; ok self init does not overwrite files"),
                )
                .into());
            }
        }
        let sum = checksums(version, sums_file.as_deref())?;
        let sum = |t: &str| {
            sum.iter()
                .find(|(x, _)| *x == t)
                .map(|(_, s)| s.clone())
                .unwrap_or_default()
        };
        outputs.push(("ok", sh_wrapper(version, &sum)));
        outputs.push(("ok.ps1", ps_wrapper(version, &sum).replace('\n', "\r\n")));
        outputs.push(("ok.cmd", CMD_WRAPPER.replace('\n', "\r\n")));
    }
    let write_err = |name: &str, e: std::io::Error| -> Flow {
        OkError::new(
            ErrKind::Io,
            format!("cannot write {name}: {}", crate::interp::io_reason(&e)),
        )
        .into()
    };
    if existing.is_none() {
        std::fs::create_dir_all(dir.join(".ok")).map_err(|e| write_err(".ok/", e))?;
    }
    for (name, content) in &outputs {
        // `create_new` keeps a file that appeared since the check above.
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dir.join(OkPath::parse(name).native()))
            .map_err(|e| write_err(name, e))?;
        std::io::Write::write_all(&mut f, content.as_bytes()).map_err(|e| write_err(name, e))?;
    }
    if wrappers {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(dir.join("ok"), std::fs::Permissions::from_mode(0o755));
        }
        match existing {
            None => println!(
                "wrote .ok/ok.ok, ./ok, ok.cmd and ok.ps1 (pinned to ok {version})\nrun ./ok hello (or ok.cmd hello on Windows)"
            ),
            Some(_) => println!(
                "wrote ./ok, ok.cmd and ok.ps1 (pinned to ok {version})\nrun ./ok (or ok.cmd on Windows) to list the tasks"
            ),
        }
    } else {
        println!("wrote .ok/ok.ok\nrun ok hello");
    }
    Ok(Value::Null)
}

/// The SHA-256 of each release target's binary, from `file` or downloaded
/// from the release of `version`.
fn checksums(version: &str, file: Option<&str>) -> Result<Vec<(&'static str, String)>, Flow> {
    let base = std::env::var("OK_DOWNLOAD_URL")
        .unwrap_or_else(|_| format!("https://github.com/xtr-dev/ok/releases/download/v{version}"));
    // The checksums are what every later run of the wrappers trusts, so
    // they are only downloaded over HTTPS; a mirror without it can still be
    // used with a SHA256SUMS file checked by hand.
    if file.is_none() && !base.starts_with("https://") {
        return Err(OkError::new(
            ErrKind::Net,
            format!(
                "the checksums must be downloaded over https, but OK_DOWNLOAD_URL is {}",
                crate::command::display_arg(&base)
            ),
        )
        .with_help("use an https:// mirror, or download SHA256SUMS yourself, check it, and pass it with --checksums FILE")
        .into());
    }
    let sums_text = match file {
        Some(f) => std::fs::read_to_string(f).map_err(|e| OkError::new(ErrKind::Io, format!("cannot read {f}: {}", crate::interp::io_reason(&e))))?,
        None => download_text(&format!("{base}/SHA256SUMS")).map_err(|m| {
            OkError::new(ErrKind::Net, format!("cannot download the checksums of ok {version}: {m}"))
                .with_help("pass a downloaded SHA256SUMS file with --checksums FILE, or set OK_DOWNLOAD_URL to a mirror")
        })?,
    };
    let mut sums: Vec<(&'static str, String)> = Vec::new();
    for (target, file) in TARGETS {
        let hash = sums_text.lines().find_map(|l| {
            let mut parts = l.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (name == *file).then_some(hash)
        });
        let Some(hash) = hash else {
            return Err(OkError::new(
                ErrKind::Value,
                format!("the checksums have no entry for {file}"),
            )
            .into());
        };
        // The digest is pasted into shell and PowerShell code, so nothing
        // but 64 hex digits may get through.
        if !is_sha256(hash) {
            return Err(OkError::new(
                ErrKind::Value,
                format!(
                    "the checksum for {file} is not a SHA-256 digest: {}",
                    crate::command::display_arg(hash)
                ),
            )
            .with_help("a SHA256SUMS line is 64 hexadecimal digits, spaces, and the file name")
            .into());
        }
        sums.push((target, hash.to_ascii_lowercase()));
    }
    Ok(sums)
}

/// One step of `ok self promote`, kept so that a failure can undo the
/// steps made before it.
enum Step {
    Moved(PathBuf, PathBuf),
    MadeDir(PathBuf),
}

/// `ok self promote` (SPEC.md 17): move a `.ok` root file to `.ok/ok.ok`
/// and the deprecated `ok/` directory's contents into `.ok/`, checking
/// every move before making any and undoing them all if one fails.
pub fn promote(args: &[String], start: &Path) -> R<Value> {
    if let Some(a) = args.first() {
        return Err(usage(&format!(
            "ok self promote takes no arguments, got {a}"
        )));
    }
    let Some(root) = crate::project::discover(start) else {
        return Err(usage(&format!(
            "no project found here ({}), so there is nothing to promote",
            crate::project::ROOT_FORMS
        )));
    };
    let root_rel = crate::project::root_file(&root)?;
    let refuse = |msg: String| -> Flow { OkError::new(ErrKind::Io, msg).into() };
    let old_dir = root.join("ok");
    let dot_dir = root.join(".ok");
    // `ok/` is the deprecated namespace directory only when it holds ok
    // files, as for loading (SPEC.md 15.1); otherwise it belongs to
    // something else and stays.
    let old_used = !crate::project::ok_files_in(&old_dir, "ok/", false)?.is_empty();
    // (from, to), relative to the root with `/`.
    let mut moves: Vec<(String, String)> = Vec::new();
    if root_rel == ".ok" {
        // A link moved one directory down no longer points at its file
        // when its target is relative, and the project would be gone.
        if std::fs::symlink_metadata(&dot_dir).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(OkError::new(
                ErrKind::Io,
                ".ok is a symbolic link; ok self promote only moves regular files",
            )
            .with_help("make .ok/ and put the file, or a link to it, at .ok/ok.ok by hand")
            .into());
        }
        moves.push((".ok".into(), ".ok/ok.ok".into()));
    }
    if old_used {
        // A linked directory would move files from outside the project,
        // or into it from somewhere else.
        for (d, shown) in [(&old_dir, "ok/"), (&dot_dir, ".ok/")] {
            if std::fs::symlink_metadata(d).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(refuse(format!(
                    "{shown} is a symbolic link; ok self promote only moves files inside the project"
                )));
            }
        }
        let mut names: Vec<String> = std::fs::read_dir(&old_dir)
            .map_err(|e| refuse(format!("cannot read ok/: {}", crate::interp::io_reason(&e))))?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        for n in names {
            moves.push((format!("ok/{n}"), format!(".ok/{n}")));
        }
    }
    if moves.is_empty() {
        if root_rel == ".ok/ok.ok" {
            println!("nothing to promote: the project already uses .ok/ok.ok");
            return Ok(Value::Null);
        }
        return Err(OkError::new(
            ErrKind::Io,
            format!("nothing to promote: the root file is {root_rel}, not a .ok file, and there is no ok/ directory"),
        )
        .with_help(format!("ok self promote moves a .ok file to .ok/ok.ok and ok/ to .ok/; {root_rel} works where it is"))
        .into());
    }
    // Refuse before moving anything.
    for (from, to) in &moves {
        if to == ".ok/ok.ok" && from != ".ok" {
            return Err(OkError::new(
                ErrKind::Io,
                format!("{from} would become .ok/ok.ok, a second root file"),
            )
            .with_help(format!("rename {from} (the namespace `ok`) first"))
            .into());
        }
        let exists = std::fs::symlink_metadata(root.join(OkPath::parse(to).native())).is_ok();
        if exists && to != ".ok/ok.ok" {
            return Err(refuse(format!(
                "cannot move {from} to {to}: {to} already exists; nothing was moved"
            )));
        }
    }
    let notes = import_notes(&root, root_rel, &moves);

    let native = |rel: &str| root.join(OkPath::parse(rel).native());
    let mut steps: Vec<Step> = Vec::new();
    let run = |steps: &mut Vec<Step>| -> std::io::Result<()> {
        if root_rel == ".ok" {
            // `.ok/` cannot be made while the file `.ok` holds the name, so
            // the file steps aside under a name nothing else uses.
            // (No process id in the name: WebAssembly has none.)
            let tmp = (0u32..)
                .map(|n| root.join(format!(".ok.promote-{n}")))
                .find(|p| std::fs::symlink_metadata(p).is_err())
                .expect("a free temporary name");
            std::fs::rename(&dot_dir, &tmp)?;
            steps.push(Step::Moved(dot_dir.clone(), tmp.clone()));
            std::fs::create_dir(&dot_dir)?;
            steps.push(Step::MadeDir(dot_dir.clone()));
            let to = dot_dir.join("ok.ok");
            std::fs::rename(&tmp, &to)?;
            steps.push(Step::Moved(tmp, to));
        } else if std::fs::symlink_metadata(&dot_dir).is_err() {
            std::fs::create_dir(&dot_dir)?;
            steps.push(Step::MadeDir(dot_dir.clone()));
        }
        for (from, to) in moves.iter().filter(|(f, _)| f != ".ok") {
            let (from, to) = (native(from), native(to));
            // `rename` replaces an existing file on Unix; never overwrite
            // one that appeared since the check.
            if std::fs::symlink_metadata(&to).is_ok() {
                return Err(std::io::ErrorKind::AlreadyExists.into());
            }
            std::fs::rename(&from, &to)?;
            steps.push(Step::Moved(from, to));
        }
        Ok(())
    };
    if let Err(e) = run(&mut steps) {
        let mut stuck = Vec::new();
        for step in steps.iter().rev() {
            let undone = match step {
                Step::Moved(from, to) => std::fs::rename(to, from).map_err(|_| to),
                Step::MadeDir(d) => std::fs::remove_dir(d).map_err(|_| d),
            };
            if let Err(p) = undone {
                stuck.push(rel_display(&root, p));
            }
        }
        let mut err = OkError::new(
            ErrKind::Io,
            format!(
                "cannot promote the project: {}",
                crate::interp::io_reason(&e)
            ),
        );
        err = if stuck.is_empty() {
            err.with_note("nothing was moved")
        } else {
            err.with_note(format!(
                "these could not be moved back: {}",
                stuck.join(", ")
            ))
            .with_help("move them back by hand")
        };
        return Err(err.into());
    }
    for (from, to) in &moves {
        println!("moved {from} to {to}");
    }
    if old_used && let Err(e) = std::fs::remove_dir(&old_dir) {
        println!(
            "ok/ is empty but could not be removed: {}",
            crate::interp::io_reason(&e)
        );
    }
    if !notes.is_empty() {
        println!("imports are relative to their file; these no longer point at the same file:");
        for n in &notes {
            println!("  {}", crate::error::terminal_safe_line(n));
        }
    }
    println!(
        "the project files moved, so the next task run asks for trust again (or run ok trust)"
    );
    Ok(Value::Null)
}

fn rel_display(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .map(|r| OkPath::from_native(r).display())
        .unwrap_or_else(|_| OkPath::from_native(p).display())
}

/// `a/./b/../c` as `a/c`, for `/`-separated paths relative to the project
/// root; `None` for a path that leaves the root.
fn normalize_rel(p: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    Some(parts.join("/"))
}

/// The directory part of a relative path (`""` for the root).
fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map_or("", |(d, _)| d)
}

fn join_rel(dir: &str, p: &str) -> String {
    if dir.is_empty() {
        p.to_string()
    } else {
        format!("{dir}/{p}")
    }
}

/// The path from directory `from` to `to`, both relative to the root.
fn relative_rel(from: &str, to: &str) -> String {
    let f: Vec<&str> = from.split('/').filter(|s| !s.is_empty()).collect();
    let t: Vec<&str> = to.split('/').filter(|s| !s.is_empty()).collect();
    let common = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut out: Vec<&str> = vec![".."; f.len() - common];
    out.extend(&t[common..]);
    out.join("/")
}

/// Where `rel` ends up after `moves`.
fn moved(rel: &str, moves: &[(String, String)]) -> String {
    for (from, to) in moves {
        if rel == from {
            return to.clone();
        }
        if let Some(rest) = rel.strip_prefix(&format!("{from}/")) {
            return format!("{to}/{rest}");
        }
    }
    rel.to_string()
}

/// The imports that `moves` breaks: in the root file and in the moved ok
/// files, each import whose path, read from the file's new place, names a
/// different file than it did. Each note gives the path to write instead.
fn import_notes(root: &Path, root_rel: &str, moves: &[(String, String)]) -> Vec<String> {
    let mut files = vec![root_rel.to_string()];
    let mut stack = vec![root.join("ok")];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "ok") {
                files.push(rel_display(root, &p));
            }
        }
    }
    files.sort();
    let mut notes = Vec::new();
    for old in files {
        let Ok(text) = std::fs::read_to_string(root.join(OkPath::parse(&old).native())) else {
            continue;
        };
        let Ok(prog) = crate::parser::parse(0, &text) else {
            continue;
        };
        let new = moved(&old, moves);
        for stmt in &prog.stmts {
            let crate::ast::StmtKind::Import {
                path, path_span, ..
            } = &stmt.kind
            else {
                continue;
            };
            let as_path = OkPath::parse(path);
            if as_path.is_abs() || path.starts_with(['/', '\\']) || path.contains("://") {
                continue;
            }
            let p = path.replace('\\', "/");
            let Some(target) = normalize_rel(&join_rel(dir_of(&old), &p)) else {
                continue;
            };
            let want = moved(&target, moves);
            if normalize_rel(&join_rel(dir_of(&new), &p)).as_deref() == Some(want.as_str()) {
                continue;
            }
            let line = text[..path_span.start as usize].matches('\n').count() + 1;
            notes.push(format!(
                "{new}:{line}: \"{path}\" should now be \"{}\"",
                relative_rel(dir_of(&new), &want)
            ));
        }
    }
    notes
}

/// Whether `s` is a SHA-256 digest in hex: exactly 64 hex digits.
fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn download_text(url: &str) -> Result<String, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(30)))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::Rustls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into();
    let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
    resp.body_mut().read_to_string().map_err(|e| e.to_string())
}

fn sh_wrapper(version: &str, sum: &dyn Fn(&str) -> String) -> String {
    format!(
        r#"#!/bin/sh
# ok bootstrap wrapper, written by `ok self init`.
# It downloads the pinned ok release for this machine once, checks its
# SHA-256, keeps it in the user cache, and runs it with all arguments.
set -eu
OK_VERSION="{version}"
OK_BASE_URL="${{OK_DOWNLOAD_URL:-https://github.com/xtr-dev/ok/releases/download/v$OK_VERSION}}"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)                target=x86_64-unknown-linux-musl;  sum={lx} ;;
  Linux-aarch64 | Linux-arm64) target=aarch64-unknown-linux-musl; sum={la} ;;
  Darwin-x86_64)               target=x86_64-apple-darwin;        sum={mx} ;;
  Darwin-arm64)                target=aarch64-apple-darwin;       sum={ma} ;;
  *) echo "ok: no prebuilt ok for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

# The cache is keyed by the checksum too, so a binary cached for another
# pin of the same version (a mirror, an edited wrapper) is never reused.
cache="${{XDG_CACHE_HOME:-$HOME/.cache}}/ok/$OK_VERSION-$(printf '%.16s' "$sum")"
bin="$cache/ok-$target"
if [ ! -x "$bin" ]; then
  mkdir -p "$cache"
  tmp="$bin.download.$$"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$OK_BASE_URL/ok-$target" -o "$tmp"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$OK_BASE_URL/ok-$target" -O "$tmp"
  else
    echo "ok: curl or wget is needed to download ok" >&2; exit 1
  fi
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp" | cut -d' ' -f1)
  else
    actual=$(shasum -a 256 "$tmp" | cut -d' ' -f1)
  fi
  if [ "$actual" != "$sum" ]; then
    rm -f "$tmp"
    echo "ok: checksum mismatch for ok-$target (expected $sum, got $actual); not running it" >&2
    exit 1
  fi
  chmod +x "$tmp"
  mv "$tmp" "$bin"
fi
exec "$bin" "$@"
"#,
        lx = sum("x86_64-unknown-linux-musl"),
        la = sum("aarch64-unknown-linux-musl"),
        mx = sum("x86_64-apple-darwin"),
        ma = sum("aarch64-apple-darwin"),
    )
}

fn ps_wrapper(version: &str, sum: &dyn Fn(&str) -> String) -> String {
    format!(
        r#"# ok bootstrap wrapper for Windows, written by `ok self init`.
# It downloads the pinned ok release for this machine once, checks its
# SHA-256, keeps it in the user cache, and runs it with all arguments.
$ErrorActionPreference = 'Stop'
$Version = '{version}'
$Base = if ($env:OK_DOWNLOAD_URL) {{ $env:OK_DOWNLOAD_URL }} else {{ "https://github.com/xtr-dev/ok/releases/download/v$Version" }}
$Arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
switch ($Arch) {{
    'X64'   {{ $Target = 'x86_64-pc-windows-msvc';  $Sum = '{wx}' }}
    'Arm64' {{ $Target = 'aarch64-pc-windows-msvc'; $Sum = '{wa}' }}
    default {{ Write-Error "ok: no prebuilt ok for Windows $Arch"; exit 1 }}
}}
# The cache is keyed by the checksum too, so a binary cached for another
# pin of the same version (a mirror, an edited wrapper) is never reused.
$Cache = Join-Path $env:LOCALAPPDATA "ok\$Version-$($Sum.Substring(0, 16))"
$Bin = Join-Path $Cache "ok-$Target.exe"
if (-not (Test-Path -PathType Leaf $Bin)) {{
    New-Item -ItemType Directory -Force -Path $Cache | Out-Null
    $Tmp = "$Bin.download-$([guid]::NewGuid())"
    Invoke-WebRequest -UseBasicParsing -Uri "$Base/ok-$Target.exe" -OutFile $Tmp
    $Actual = (Get-FileHash -Algorithm SHA256 $Tmp).Hash.ToLower()
    if ($Actual -ne $Sum) {{
        Remove-Item $Tmp
        Write-Error "ok: checksum mismatch for ok-$Target.exe (expected $Sum, got $Actual); not running it"
        exit 1
    }}
    Move-Item -Force $Tmp $Bin
}}
& $Bin @args
exit $LASTEXITCODE
"#,
        wx = sum("x86_64-pc-windows-msvc"),
        wa = sum("aarch64-pc-windows-msvc"),
    )
}

/// cmd.exe looks for a bare `powershell` in the current directory before
/// PATH, so a `powershell.bat` in a cloned repository would run instead;
/// the wrapper names the system's PowerShell by its full path.
const CMD_WRAPPER: &str = "@echo off\nrem ok bootstrap wrapper for cmd.exe, written by `ok self init`; the work is done by ok.ps1.\n\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -ExecutionPolicy Bypass -File \"%~dp0ok.ps1\" %*\nexit /b %ERRORLEVEL%\n";
