//! Trust (SPEC.md section 16).
//!
//! A project's tasks run only after the user approved the exact content of
//! its files. Trust is content-addressed: the fingerprint is a SHA-256 over
//! every project file. Each trusted version is kept as a snapshot so that a
//! changed project can show a diff. Trust is not a sandbox; it only answers
//! "did I agree to run this code?".

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use sha2::Digest;

use crate::error::{ErrKind, OkError, terminal_safe, terminal_safe_line};
use crate::interp::{Flow, R, io_reason};
use crate::path::OkPath;
use crate::project::Project;

/// The trust store directory: `OK_CONFIG_DIR` if set (used by tests), else
/// the platform configuration directory (SPEC.md 16.5).
pub fn store_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("OK_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    let home = crate::files::home_dir();
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        home.map(|h| h.join("Library").join("Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config")))
    };
    base.map(|b| b.join("ok"))
}

/// SHA-256 over every file: path, NUL, length, NUL, content (SPEC.md 16.2).
pub fn fingerprint(files: &[(String, Rc<[u8]>)]) -> String {
    let mut sorted: Vec<&(String, Rc<[u8]>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = sha2::Sha256::new();
    for (rel, content) in sorted {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(content.len().to_string().as_bytes());
        h.update([0]);
        h.update(content);
    }
    hex::encode(h.finalize())
}

/// Everything trust covers for one run (SPEC.md 16.2): the project files,
/// and the env files the run loads, which can change what every program a
/// task starts does (`NODE_OPTIONS`, `PATH`), so they are code too.
pub struct Inputs {
    /// Project files, as (path relative to the root, content), in path
    /// order. The content is the project's [`crate::project::FileStore`]
    /// copy, the same bytes that are parsed and imported.
    pub code: Vec<(String, Rc<[u8]>)>,
    /// The env files that exist, in the same form. Reviews show only
    /// their variable names, since values are often secrets.
    pub env: Vec<(String, Rc<[u8]>)>,
}

impl Inputs {
    /// The inputs of a run of `project` with `--env env`.
    pub fn of(project: &Project, env: Option<&str>) -> Result<Inputs, OkError> {
        let read = |files: Vec<(String, PathBuf)>| {
            files
                .into_iter()
                .map(|(rel, path)| match project.store.read(&path) {
                    Ok(bytes) => Ok((rel, bytes)),
                    Err(e) => Err(OkError::new(
                        ErrKind::Io,
                        format!("cannot read {rel}: {}", io_reason(&e)),
                    )),
                })
                .collect::<Result<Vec<_>, OkError>>()
        };
        Ok(Inputs {
            code: read(project.trust_files.clone())?,
            env: read(project.existing_env_files(env))?,
        })
    }

    pub fn fingerprint(&self) -> String {
        let all: Vec<(String, Rc<[u8]>)> = self.code.iter().chain(&self.env).cloned().collect();
        fingerprint(&all)
    }

    /// The file names for the prompt and `ok trust`, env files marked.
    fn names(&self) -> String {
        let code = self
            .code
            .iter()
            .map(|(r, _)| terminal_safe_line(r).into_owned());
        let env = self
            .env
            .iter()
            .map(|(r, _)| format!("{} (names only)", terminal_safe_line(r)));
        code.chain(env).collect::<Vec<_>>().join(", ")
    }

    /// Each file as a review shows it: project files as they are, env
    /// files as their variable names.
    fn shown(&self) -> Vec<(String, String)> {
        let code = self
            .code
            .iter()
            .map(|(r, b)| (r.clone(), String::from_utf8_lossy(b).into_owned()));
        let env = self.env.iter().map(|(r, b)| (r.clone(), env_summary(b)));
        code.chain(env).collect()
    }
}

/// A file's text as the review compares it; unreadable files compare as
/// empty, and bytes that are not UTF-8 are replaced.
fn read_text(path: &Path) -> String {
    String::from_utf8_lossy(&std::fs::read(path).unwrap_or_default()).into_owned()
}

/// What the review shows of an env file: its variable names and a hash of
/// its content, never the values. The hash makes a changed value visible
/// in the diff without showing it.
fn env_summary(bytes: &[u8]) -> String {
    let sha = hex::encode(sha2::Sha256::digest(bytes));
    let text = String::from_utf8_lossy(bytes);
    let mut out = format!("# variable names only; sha256 of the content: {sha}\n");
    match crate::formats::parse_document("env", &text, "") {
        Ok(crate::value::Value::Record(r)) => {
            for (k, _) in r.iter() {
                out.push_str(k);
                out.push('\n');
            }
        }
        // The parse error may quote a value, so it is not shown.
        _ => out.push_str("# (not a valid env file)\n"),
    }
    out
}

#[derive(Clone)]
struct Entry {
    root: String,
    /// `"content"` or `"dir"`.
    mode: String,
    fingerprint: Option<String>,
    at: String,
}

struct Store {
    dir: PathBuf,
    entries: Vec<Entry>,
}

fn store_err(msg: String) -> OkError {
    OkError::new(ErrKind::Trust, msg)
}

/// The entries of the store in `dir`. A missing store is empty; any other
/// failure to read it is an error, never an empty store, which the next
/// save would write over the user's approvals.
fn read_entries(dir: &Path) -> Result<Vec<Entry>, OkError> {
    let file = dir.join("trust.json");
    let shown = OkPath::from_native(&file).display();
    let text = match std::fs::read(&file) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| {
            store_err(format!(
                "the trust store {shown} is damaged: it is not UTF-8"
            ))
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => {
            return Err(store_err(format!(
                "cannot read the trust store {shown}: {}",
                io_reason(&e)
            ))
            .with_help(
                "fix the file's permissions, or move it away to start with an empty store",
            ));
        }
    };
    let v: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| store_err(format!("the trust store {shown} is damaged: {e}")))?;
    let mut entries = Vec::new();
    for e in v
        .get("entries")
        .and_then(|e| e.as_array())
        .into_iter()
        .flatten()
    {
        let s = |k: &str| e.get(k).and_then(|x| x.as_str()).map(str::to_string);
        if let (Some(root), Some(mode)) = (s("root"), s("mode")) {
            entries.push(Entry {
                root,
                mode,
                fingerprint: s("fingerprint"),
                at: s("at").unwrap_or_default(),
            });
        }
    }
    Ok(entries)
}

/// Create `dir` and its missing parents, private to the user on Unix
/// (0700): the store says which projects may run code, and snapshots copy
/// project files.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        b.mode(0o700);
    }
    b.create(dir)
}

/// Write `path` readable only by the user on Unix (0600). The content goes
/// to a temporary file first and is renamed into place, so readers never
/// see a half-written file.
fn private_write(path: &Path, content: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", crate::files::process_id()));
    let tmp = PathBuf::from(tmp);
    let _ = std::fs::remove_file(&tmp);
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    let written = o.open(&tmp).and_then(|mut f| f.write_all(content));
    if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, path)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// An exclusive hold on the store while it is read, changed and written,
/// so that two `ok` processes trusting at the same time do not lose each
/// other's entries: a lock file created with `create_new`, removed when
/// the hold ends.
struct Lock {
    path: PathBuf,
}

impl Lock {
    /// How long to wait for another process before giving up, and how old
    /// a lock file must be to count as left behind by a crashed process.
    const WAIT: std::time::Duration = std::time::Duration::from_secs(10);
    const STALE: std::time::Duration = std::time::Duration::from_secs(30);

    fn take(dir: &Path) -> Result<Lock, OkError> {
        let path = dir.join("trust.json.lock");
        let started = std::time::Instant::now();
        loop {
            let mut o = std::fs::OpenOptions::new();
            o.write(true).create_new(true);
            match o.open(&path) {
                Ok(_) => return Ok(Lock { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > Lock::STALE);
                    if stale {
                        let _ = std::fs::remove_file(&path);
                        continue;
                    }
                    if started.elapsed() > Lock::WAIT {
                        return Err(store_err(format!(
                            "the trust store is locked by another ok process ({} exists)",
                            OkPath::from_native(&path).display()
                        ))
                        .with_help("try again; if no other ok is running, remove the lock file"));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(store_err(format!(
                        "cannot lock the trust store: {}",
                        io_reason(&e)
                    )));
                }
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Store {
    fn load() -> Result<Store, OkError> {
        let dir = store_dir().ok_or_else(|| {
            store_err(
                "cannot find a configuration directory for the trust store (HOME is not set)"
                    .into(),
            )
        })?;
        let entries = read_entries(&dir)?;
        Ok(Store { dir, entries })
    }

    /// Change the entries and save them: under the lock, the store is read
    /// again and `change` applied to what is on disk, so entries another
    /// process saved since [`Store::load`] are kept.
    fn update(&mut self, change: impl FnOnce(&mut Vec<Entry>)) -> Result<(), OkError> {
        private_dir(&self.dir).map_err(|e| {
            store_err(format!(
                "cannot create {}: {}",
                self.dir.display(),
                io_reason(&e)
            ))
        })?;
        let _lock = Lock::take(&self.dir)?;
        self.entries = read_entries(&self.dir)?;
        change(&mut self.entries);
        self.save()
    }

    fn save(&self) -> Result<(), OkError> {
        let entries: Vec<serde_json::Value> = self
            .entries
            .iter()
            .map(|e| {
                let mut m = serde_json::Map::new();
                m.insert("root".into(), e.root.clone().into());
                m.insert("mode".into(), e.mode.clone().into());
                if let Some(f) = &e.fingerprint {
                    m.insert("fingerprint".into(), f.clone().into());
                }
                m.insert("at".into(), e.at.clone().into());
                serde_json::Value::Object(m)
            })
            .collect();
        let doc = serde_json::json!({ "version": 1, "entries": entries });
        let text = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
        private_write(&self.dir.join("trust.json"), text.as_bytes())
            .map_err(|e| store_err(format!("cannot write the trust store: {}", io_reason(&e))))
    }

    fn snapshot_dir(&self, fp: &str) -> PathBuf {
        self.dir.join("snapshots").join(fp)
    }

    /// Where the env file summaries of a snapshot are kept: env files can
    /// live outside the root, so they are not copied into the snapshot
    /// directory by path.
    fn env_snapshot(&self, fp: &str) -> PathBuf {
        self.dir.join("snapshots").join(format!("{fp}-env.json"))
    }

    fn old_env(&self, fp: &str) -> Vec<(String, String)> {
        let text = std::fs::read_to_string(self.env_snapshot(fp)).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        v.as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
            .collect()
    }

    fn snapshot(&self, fp: &str, inputs: &Inputs) -> Result<(), OkError> {
        let dir = self.snapshot_dir(fp);
        if !inputs.env.is_empty() {
            let map: serde_json::Map<String, serde_json::Value> = inputs
                .env
                .iter()
                .map(|(r, b)| (r.clone(), env_summary(b).into()))
                .collect();
            let text = serde_json::to_string_pretty(&map).unwrap_or_default() + "\n";
            let dst = self.env_snapshot(fp);
            private_dir(&self.dir.join("snapshots"))
                .and_then(|_| private_write(&dst, text.as_bytes()))
                .map_err(|e| {
                    store_err(format!("cannot save the env file names: {}", io_reason(&e)))
                })?;
        }
        let files = &inputs.code;
        for (rel, content) in files {
            let dst = dir.join(OkPath::parse(rel).native());
            let parent = dst.parent().unwrap_or(&dir);
            private_dir(parent)
                .and_then(|_| private_write(&dst, content))
                .map_err(|e| {
                    store_err(format!("cannot save a copy of {rel}: {}", io_reason(&e)))
                })?;
        }
        Ok(())
    }

    /// Whether `root` is trusted with this fingerprint. Content trust
    /// belongs to the root it was given for: the same files elsewhere (a
    /// copy next to different env files, another checkout) are asked
    /// about again.
    fn is_trusted(&self, root: &str, fp: &str) -> bool {
        self.entries.iter().any(|e| {
            e.root == root
                && (e.mode == "dir"
                    || (e.mode == "content" && e.fingerprint.as_deref() == Some(fp)))
        })
    }

    /// The most recently trusted fingerprint for this root, if any.
    fn previous(&self, root: &str) -> Option<String> {
        self.entries
            .iter()
            .rev()
            .find(|e| e.root == root && e.mode == "content")
            .and_then(|e| e.fingerprint.clone())
    }

    /// Record trust for `root` and save the store.
    fn add(&mut self, root: &str, mode: &str, fp: Option<String>) -> Result<(), OkError> {
        let at = crate::system::format_time_local(jiff::Timestamp::now().as_nanosecond() as i64);
        self.update(|entries| {
            entries.retain(|e| {
                !(e.root == root && e.mode == mode && (mode == "dir" || e.fingerprint == fp))
            });
            entries.push(Entry {
                root: root.to_string(),
                mode: mode.to_string(),
                fingerprint: fp,
                at,
            });
        })
    }
}

fn now_trusting(store: &mut Store, root: &str, fp: &str, inputs: &Inputs) -> Result<(), OkError> {
    store.snapshot(fp, inputs)?;
    store.add(root, "content", Some(fp.to_string()))
}

fn root_key(p: &Project) -> String {
    OkPath::from_native(&p.root).display()
}

/// Check that a project may run tasks, asking on a terminal if needed
/// (SPEC.md 16.3, 16.4).
pub fn ensure_trusted(project: &Project, skip: bool, env: Option<&str>) -> R<()> {
    if skip || std::env::var("OK_TRUST").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
        return Ok(());
    }
    let mut store = Store::load()?;
    let root = root_key(project);
    let inputs = Inputs::of(project, env)?;
    let fp = inputs.fingerprint();
    if store.is_trusted(&root, &fp) {
        return Ok(());
    }
    let previous = store
        .previous(&root)
        .filter(|p| store.snapshot_dir(p).is_dir());
    if !std::io::stdin().is_terminal() {
        return Err(not_trusted(&store, &root, previous.as_deref(), &inputs).into());
    }
    let names = inputs.names();
    let mut err = std::io::stderr();
    let headline = if previous.is_some() {
        "The project files changed since you trusted them."
    } else {
        "This project is not trusted yet."
    };
    let _ = writeln!(
        err,
        "{headline}\n\n  root:   {}\n  files:  {names}\n  sha256: {fp}\n\nTasks are code that runs with your permissions. Trust is not a sandbox.",
        terminal_safe_line(&root)
    );
    loop {
        let question = if previous.is_some() {
            "[d]iff, [v]iew files, [t]rust and run, [n]o: "
        } else {
            "[v]iew files, [t]rust and run, [n]o: "
        };
        let _ = write!(err, "{question}");
        let _ = err.flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
            return Err(Flow::Exit(1));
        }
        match line.trim().to_ascii_lowercase().as_str() {
            "v" | "view" => {
                for (rel, text) in inputs.shown() {
                    let _ = writeln!(
                        err,
                        "\n==> {} <==\n{}",
                        terminal_safe_line(&rel),
                        view_text(&text)
                    );
                }
            }
            "d" | "diff" if previous.is_some() => {
                let old = previous.as_deref().unwrap_or_default();
                let _ = write!(err, "{}", diff_project(&store, old, &inputs));
            }
            "t" | "trust" | "y" | "yes" => {
                now_trusting(&mut store, &root, &fp, &inputs)?;
                return Ok(());
            }
            "n" | "no" | "" => {
                let _ = writeln!(err, "not running anything");
                return Err(Flow::Exit(1));
            }
            _ => {}
        }
    }
}

/// A project file as `[v]iew` shows it. The review is what the user agrees
/// to, so the text must look exactly like what ok will run: control,
/// bidirectional and zero-width characters are shown as escapes (a `\r`
/// or an ANSI sequence could otherwise hide a line of code), and a leading
/// byte order mark, which ok ignores, is left out.
fn view_text(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    terminal_safe(text).into_owned()
}

/// The refusal to run without a terminal (SPEC.md 16.4). When an earlier
/// version of the root was trusted, the error says the files changed and
/// names them, standing in for the prompt's diff.
fn not_trusted(store: &Store, root: &str, previous: Option<&str>, inputs: &Inputs) -> OkError {
    let help = "review the files, then run `ok trust`; in CI set OK_TRUST=1 or pass --trust";
    let at = format!("the project is at {root}");
    match previous {
        Some(fp) => {
            let changed: Vec<String> = changed_files(store, fp, inputs)
                .into_iter()
                .map(|(rel, _, _)| rel)
                .collect();
            let mut e = OkError::new(
                ErrKind::Trust,
                "the project files changed since you trusted them",
            )
            .with_note(at);
            if !changed.is_empty() {
                e = e.with_note(format!("changed: {}", changed.join(", ")));
            }
            e.with_help(help)
        }
        None => OkError::new(ErrKind::Trust, "this project is not trusted")
            .with_note(at)
            .with_help(help),
    }
}

/// The files whose content differs between the snapshot of the trusted
/// fingerprint `fp` and the current inputs, added and removed files
/// included, as (path, trusted content, current content) in path order.
/// Env files compare as their summaries (names and a hash).
fn changed_files(store: &Store, fp: &str, current: &Inputs) -> Vec<(String, String, String)> {
    let old_dir = store.snapshot_dir(fp);
    let mut old: Vec<(String, String)> = Vec::new();
    let mut old_files: Vec<String> = Vec::new();
    collect_rel(&old_dir, &old_dir, &mut old_files);
    for rel in old_files {
        let text = read_text(&old_dir.join(OkPath::parse(&rel).native()));
        old.push((rel, text));
    }
    old.extend(store.old_env(fp));
    let new = current.shown();
    let mut names: Vec<&String> = old.iter().chain(&new).map(|(r, _)| r).collect();
    names.sort();
    names.dedup();
    let find = |list: &[(String, String)], rel: &str| {
        list.iter()
            .find(|(r, _)| r == rel)
            .map(|(_, t)| t.clone())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for rel in names {
        let (o, n) = (find(&old, rel), find(&new, rel));
        if o != n {
            out.push((rel.clone(), o, n));
        }
    }
    out
}

/// A unified diff between the trusted snapshot and the current files.
fn diff_project(store: &Store, fp: &str, current: &Inputs) -> String {
    let mut out = String::new();
    for (rel, old, new) in changed_files(store, fp, current) {
        out.push_str(&unified_diff(&rel, &old, &new));
    }
    if out.is_empty() {
        out.push_str("(no differences in file content)\n");
    }
    out
}

fn collect_rel(base: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            collect_rel(base, &p, out);
        } else if let Ok(r) = p.strip_prefix(base) {
            out.push(OkPath::from_native(r).display());
        }
    }
}

/// A line diff with three lines of context, computed with a longest common
/// subsequence table (project files are small).
pub fn unified_diff(name: &str, old: &str, new: &str) -> String {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let (n, m) = (a.len(), b.len());
    if n * m > 25_000_000 {
        let name = terminal_safe_line(name);
        return format!(
            "--- trusted/{name}\n+++ current/{name}\n(the file is too large to diff: {n} and {m} lines)\n"
        );
    }
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    // Edit script: (' ' | '-' | '+', line).
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            // Removals come before additions, as in `diff -u`.
            ops.push(('-', a[i]));
            i += 1;
        } else {
            ops.push(('+', b[j]));
            j += 1;
        }
    }
    // Names and lines are shown with unsafe characters escaped, for the
    // same reason as the file view.
    let name = terminal_safe_line(name);
    let mut out = format!("--- trusted/{name}\n+++ current/{name}\n");
    let changed: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter(|(_, (c, _))| *c != ' ')
        .map(|(k, _)| k)
        .collect();
    let mut shown_until = 0usize;
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(3).max(shown_until);
        let mut end = changed[k] + 4;
        while k + 1 < changed.len() && changed[k + 1] <= end + 3 {
            k += 1;
            end = changed[k] + 4;
        }
        let end = end.min(ops.len());
        if start > shown_until || shown_until == 0 && start > 0 {
            out.push_str("@@\n");
        }
        for (c, line) in &ops[start..end] {
            out.push(*c);
            out.push_str(&terminal_safe_line(line));
            out.push('\n');
        }
        shown_until = end;
        k += 1;
    }
    out
}

/// `ok trust [--dir] [--list] [DIR]` and `ok untrust [DIR]` (SPEC.md 16.5).
pub fn command(
    untrust: bool,
    env: Option<&str>,
    args: &[String],
    discover_from: &Path,
    interp: &mut crate::interp::Interp,
) -> R<()> {
    let mut dir_mode = false;
    let mut list = false;
    let mut target: Option<String> = None;
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print!("{}", crate::cli::TRUST_USAGE);
        return Ok(());
    }
    for a in args {
        match a.as_str() {
            "--dir" if !untrust => dir_mode = true,
            "--list" if !untrust => list = true,
            other if other.starts_with('-') => {
                let mut e = OkError::new(
                    ErrKind::Arg,
                    format!(
                        "unknown option {other} for ok {}",
                        if untrust { "untrust" } else { "trust" }
                    ),
                );
                e.help = Some(if untrust {
                    "usage: ok untrust [DIR]".into()
                } else {
                    "usage: ok trust [--dir] [--list] [DIR]".into()
                });
                e.exit_code = Some(2);
                return Err(e.into());
            }
            other => target = Some(other.to_string()),
        }
    }
    let mut store = Store::load()?;
    if list {
        if store.entries.is_empty() {
            println!("nothing is trusted yet");
        }
        for e in &store.entries {
            match e.mode.as_str() {
                "dir" => println!(
                    "{}  directory, any content  ({})",
                    terminal_safe_line(&e.root),
                    e.at
                ),
                _ => println!(
                    "{}  sha256 {}  ({})",
                    terminal_safe_line(&e.root),
                    e.fingerprint.as_deref().unwrap_or("?"),
                    e.at
                ),
            }
        }
        return Ok(());
    }
    let start = match &target {
        Some(t) => discover_from.join(t),
        None => discover_from.to_path_buf(),
    };
    let Some(root) = crate::project::discover(&start) else {
        let mut e = OkError::new(
            ErrKind::Arg,
            format!(
                "no project found in {} or its parents ({})",
                OkPath::from_native(&start).display(),
                crate::project::ROOT_FORMS
            ),
        );
        e.exit_code = Some(2);
        return Err(e.into());
    };
    let key = OkPath::from_native(&root).display();
    if untrust {
        let mut removed = false;
        store.update(|entries| {
            let before = entries.len();
            entries.retain(|e| e.root != key);
            removed = entries.len() != before;
        })?;
        if !removed {
            println!("{} was not trusted", terminal_safe_line(&key));
        } else {
            println!("no longer trusting {}", terminal_safe_line(&key));
        }
        return Ok(());
    }
    if dir_mode {
        store.add(&key, "dir", None)?;
        println!(
            "trusting {} whatever its content (for your own projects)",
            terminal_safe_line(&key)
        );
        return Ok(());
    }
    let project = interp.load_project(&root)?;
    let inputs = Inputs::of(&project, env)?;
    let fp = inputs.fingerprint();
    now_trusting(&mut store, &key, &fp, &inputs)?;
    println!(
        "trusted {}\n  files:  {}\n  sha256: {fp}",
        terminal_safe_line(&key),
        inputs.names()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_shows_changed_lines_with_context() {
        let d = unified_diff(
            "ok.ok",
            "a\nb\nc\nd\ne\nf\ng\nh\n",
            "a\nb\nc\nD\ne\nf\ng\nh\n",
        );
        assert!(d.contains("-d\n+D\n"), "{d}");
        assert!(d.contains(" c\n"));
        assert!(!d.contains(" h\n"), "only three lines of context: {d}");
    }

    #[test]
    fn diff_and_view_show_hidden_characters_as_escapes() {
        // A carriage return can put a harmless-looking text over the code
        // that runs; the review must show it.
        let d = unified_diff(
            "ok\x1b[8m.ok",
            "task a {}\n",
            "task a { run evil } # \r task a {}\n",
        );
        assert!(d.contains("+task a { run evil } # \\r task a {}\n"), "{d}");
        assert!(d.contains("--- trusted/ok\\u{1b}[8m.ok"), "{d}");
        let dir = std::env::temp_dir().join(format!("ok-view-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ok.ok"), "\u{feff}a\u{202e}b\tc\n").unwrap();
        let shown = view_text(&read_text(&dir.join("ok.ok")));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(shown, "a\\u{202e}b\tc\n");
    }

    #[test]
    fn fingerprint_depends_on_names_and_content() {
        let x: Rc<[u8]> = Rc::from(&b"x"[..]);
        let y: Rc<[u8]> = Rc::from(&b"y"[..]);
        let f1 = fingerprint(&[("ok.ok".into(), x.clone())]);
        let f2 = fingerprint(&[("ok/x.ok".into(), x)]);
        let f3 = fingerprint(&[("ok.ok".into(), y)]);
        assert_ne!(f1, f2);
        assert_ne!(f1, f3);
        assert_eq!(f1.len(), 64);
    }
}
