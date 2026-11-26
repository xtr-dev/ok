//! Extracting an untrusted archive must end in files under the destination
//! or an `ok` error: never a panic, a hang, or anything created outside the
//! destination. The first byte picks zip, tar or gzip-compressed tar; the
//! rest is the archive. Each run extracts into `box/work/out` and then checks
//! that `box` holds nothing but `work`, that `work` holds nothing but the
//! archive and `out`, and that every link under `out` resolves to a place
//! inside `out` (SPEC.md 12.9 allows only those).
#![no_main]

use libfuzzer_sys::fuzz_target;
use std::path::{Path, PathBuf};

fn sandbox() -> PathBuf {
    let root = std::env::temp_dir().join(format!("okrun-fuzz-archive-{}", std::process::id()));
    let work = root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::env::set_current_dir(&work).unwrap();
    root
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> =
        std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// Resolve `p` the way the operating system would, following links one
/// component at a time, so dangling links are judged by where they point.
fn resolve(p: &Path, depth: usize) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in p.components() {
        match comp {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => {
                out.push(other);
                if depth < 40
                    && let Ok(target) = std::fs::read_link(&out)
                {
                    out.pop();
                    out = resolve(&out.join(target), depth + 1);
                }
            }
        }
    }
    out
}

fn links_stay_inside(dir: &Path, out: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_symlink() {
            let to = resolve(&entry.path(), 0);
            assert!(to.starts_with(out), "link {} points outside to {}", entry.path().display(), to.display());
        } else if kind.is_dir() {
            links_stay_inside(&entry.path(), out);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    static ROOT: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    let root = ROOT.get_or_init(sandbox);
    let work = root.join("work");
    let Some((&sel, body)) = data.split_first() else { return };
    let (file, func) = match sel % 3 {
        0 => ("in.zip", "unzip"),
        1 => ("in.tar", "untar"),
        _ => ("in.tar.gz", "untar"),
    };
    let _ = std::fs::remove_dir_all(work.join("out"));
    for name in names(&work) {
        std::fs::remove_file(work.join(name)).unwrap();
    }
    std::fs::write(work.join(file), body).unwrap();
    let src = format!("{func}(\"{file}\", \"out\")\n");
    let mut interp = okrun::interp::Interp::new(Vec::new());
    let _ = interp.run_source("fuzz.ok", &src, work.clone());
    assert_eq!(names(root), ["work"], "extraction wrote outside its destination");
    for name in names(&work) {
        assert!(name == file || name == "out", "extraction wrote {name} next to its destination");
    }
    let out = work.join("out");
    if out.is_dir() {
        links_stay_inside(&out, &out);
    }
});
