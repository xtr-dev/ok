//! Trust (SPEC.md section 16).
//!
//! A project's tasks run only after the user approved the exact content of
//! its files. Trust is content-addressed: the fingerprint is a SHA-256 over
//! every project file. Each trusted version is kept as a snapshot so that a
//! changed project can show a diff. Trust is not a sandbox; it only answers
//! "did I agree to run this code?".

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use sha2::Digest;

use crate::error::{ErrKind, OkError};
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
pub fn fingerprint(files: &[(String, PathBuf)]) -> Result<String, OkError> {
    let mut sorted: Vec<&(String, PathBuf)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = sha2::Sha256::new();
    for (rel, path) in sorted {
        let content = std::fs::read(path).map_err(|e| {
            OkError::new(ErrKind::Io, format!("cannot read {rel}: {}", io_reason(&e)))
        })?;
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(content.len().to_string().as_bytes());
        h.update([0]);
        h.update(&content);
    }
    Ok(hex::encode(h.finalize()))
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

impl Store {
    fn load() -> Result<Store, OkError> {
        let dir = store_dir().ok_or_else(|| {
            store_err(
                "cannot find a configuration directory for the trust store (HOME is not set)"
                    .into(),
            )
        })?;
        let file = dir.join("trust.json");
        let mut entries = Vec::new();
        if let Ok(text) = std::fs::read_to_string(&file) {
            let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
                store_err(format!(
                    "the trust store {} is damaged: {e}",
                    file.display()
                ))
            })?;
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
        }
        Ok(Store { dir, entries })
    }

    fn save(&self) -> Result<(), OkError> {
        std::fs::create_dir_all(&self.dir).map_err(|e| {
            store_err(format!(
                "cannot create {}: {}",
                self.dir.display(),
                io_reason(&e)
            ))
        })?;
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
        // Write to a temporary file and rename, so the store is never half-written.
        let tmp = self
            .dir
            .join(format!("trust.json.{}.tmp", crate::files::process_id()));
        std::fs::write(&tmp, text)
            .map_err(|e| store_err(format!("cannot write the trust store: {}", io_reason(&e))))?;
        std::fs::rename(&tmp, self.dir.join("trust.json"))
            .map_err(|e| store_err(format!("cannot write the trust store: {}", io_reason(&e))))
    }

    fn snapshot_dir(&self, fp: &str) -> PathBuf {
        self.dir.join("snapshots").join(fp)
    }

    fn snapshot(&self, fp: &str, files: &[(String, PathBuf)]) -> Result<(), OkError> {
        let dir = self.snapshot_dir(fp);
        for (rel, path) in files {
            let dst = dir.join(OkPath::parse(rel).native());
            crate::files::ensure_parent(&dst)
                .and_then(|_| std::fs::copy(path, &dst).map(|_| ()))
                .map_err(|e| {
                    store_err(format!("cannot save a copy of {rel}: {}", io_reason(&e)))
                })?;
        }
        Ok(())
    }

    fn is_trusted(&self, root: &str, fp: &str) -> bool {
        self.entries.iter().any(|e| {
            (e.mode == "dir" && e.root == root)
                || (e.mode == "content" && e.fingerprint.as_deref() == Some(fp))
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

    fn add(&mut self, root: &str, mode: &str, fp: Option<String>) {
        self.entries.retain(|e| {
            !(e.root == root && e.mode == mode && (mode == "dir" || e.fingerprint == fp))
        });
        let at = crate::system::format_time_local(jiff::Timestamp::now().as_nanosecond() as i64);
        self.entries.push(Entry {
            root: root.to_string(),
            mode: mode.to_string(),
            fingerprint: fp,
            at,
        });
    }
}

fn now_trusting(
    store: &mut Store,
    root: &str,
    fp: &str,
    files: &[(String, PathBuf)],
) -> Result<(), OkError> {
    store.snapshot(fp, files)?;
    store.add(root, "content", Some(fp.to_string()));
    store.save()
}

fn root_key(p: &Project) -> String {
    OkPath::from_native(&p.root).display()
}

/// Check that a project may run tasks, asking on a terminal if needed
/// (SPEC.md 16.3, 16.4).
pub fn ensure_trusted(project: &Project, skip: bool) -> R<()> {
    if skip || std::env::var("OK_TRUST").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true")) {
        return Ok(());
    }
    let mut store = Store::load()?;
    let root = root_key(project);
    let fp = fingerprint(&project.trust_files)?;
    if store.is_trusted(&root, &fp) {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        let mut e = OkError::new(
            ErrKind::Trust,
            format!("this project is not trusted: {root}"),
        );
        e.help = Some(
            "review the files, then run `ok trust`; in CI set OK_TRUST=1 or pass --trust".into(),
        );
        return Err(e.into());
    }
    let previous = store
        .previous(&root)
        .filter(|p| store.snapshot_dir(p).is_dir());
    let names: Vec<&str> = project
        .trust_files
        .iter()
        .map(|(r, _)| r.as_str())
        .collect();
    let mut err = std::io::stderr();
    let headline = if previous.is_some() {
        "The project files changed since you trusted them."
    } else {
        "This project is not trusted yet."
    };
    let _ = writeln!(
        err,
        "{headline}\n\n  root:   {root}\n  files:  {}\n  sha256: {fp}\n\nTasks are code that runs with your permissions. Trust is not a sandbox.",
        names.join(", ")
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
                for (rel, path) in &project.trust_files {
                    let text = std::fs::read_to_string(path).unwrap_or_default();
                    let _ = writeln!(err, "\n==> {rel} <==\n{text}");
                }
            }
            "d" | "diff" if previous.is_some() => {
                let old_dir = store.snapshot_dir(previous.as_deref().unwrap_or_default());
                let _ = write!(err, "{}", diff_project(&old_dir, &project.trust_files));
            }
            "t" | "trust" | "y" | "yes" => {
                now_trusting(&mut store, &root, &fp, &project.trust_files)?;
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

/// A unified diff between the trusted snapshot and the current files.
fn diff_project(old_dir: &Path, current: &[(String, PathBuf)]) -> String {
    let mut out = String::new();
    let mut old_files: Vec<String> = Vec::new();
    collect_rel(old_dir, old_dir, &mut old_files);
    let mut names: Vec<String> = current.iter().map(|(r, _)| r.clone()).collect();
    names.extend(old_files);
    names.sort();
    names.dedup();
    for rel in names {
        let old =
            std::fs::read_to_string(old_dir.join(OkPath::parse(&rel).native())).unwrap_or_default();
        let new = current
            .iter()
            .find(|(r, _)| *r == rel)
            .and_then(|(_, p)| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        if old != new {
            out.push_str(&unified_diff(&rel, &old, &new));
        }
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
            out.push_str(line);
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
    args: &[String],
    discover_from: &Path,
    interp: &mut crate::interp::Interp,
) -> R<()> {
    let mut dir_mode = false;
    let mut list = false;
    let mut target: Option<String> = None;
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
                "dir" => println!("{}  directory, any content  ({})", e.root, e.at),
                _ => println!(
                    "{}  sha256 {}  ({})",
                    e.root,
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
                "no ok.ok found in {} or its parents",
                OkPath::from_native(&start).display()
            ),
        );
        e.exit_code = Some(2);
        return Err(e.into());
    };
    let key = OkPath::from_native(&root).display();
    if untrust {
        let before = store.entries.len();
        store.entries.retain(|e| e.root != key);
        store.save()?;
        if store.entries.len() == before {
            println!("{key} was not trusted");
        } else {
            println!("no longer trusting {key}");
        }
        return Ok(());
    }
    if dir_mode {
        store.add(&key, "dir", None);
        store.save()?;
        println!("trusting {key} whatever its content (for your own projects)");
        return Ok(());
    }
    let project = interp.load_project(&root)?;
    let fp = fingerprint(&project.trust_files)?;
    now_trusting(&mut store, &key, &fp, &project.trust_files)?;
    println!(
        "trusted {key}\n  files:  {}\n  sha256: {fp}",
        project
            .trust_files
            .iter()
            .map(|(r, _)| r.as_str())
            .collect::<Vec<_>>()
            .join(", ")
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
    fn fingerprint_depends_on_names_and_content() {
        let dir = std::env::temp_dir().join(format!("ok-fp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a"), "x").unwrap();
        let f1 = fingerprint(&[("ok.ok".into(), dir.join("a"))]).unwrap();
        let f2 = fingerprint(&[("ok/x.ok".into(), dir.join("a"))]).unwrap();
        std::fs::write(dir.join("a"), "y").unwrap();
        let f3 = fingerprint(&[("ok.ok".into(), dir.join("a"))]).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_ne!(f1, f2);
        assert_ne!(f1, f3);
        assert_eq!(f1.len(), 64);
    }
}
