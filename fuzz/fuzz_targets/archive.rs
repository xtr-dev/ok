//! Extracting an untrusted archive must end in files under the destination
//! or an `ok` error: never a panic, a hang, or anything created outside the
//! destination. The first byte picks zip, tar or gzip-compressed tar; the
//! rest is the archive. Each run extracts into `box/work/out` and then checks
//! that `box` holds nothing but `work`, that `work` holds nothing but the
//! archive and `out`, and that no symlink under `out` exists.
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

fn no_links(dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        assert!(!kind.is_symlink(), "extraction created a link: {}", entry.path().display());
        if kind.is_dir() {
            no_links(&entry.path());
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
    if work.join("out").is_dir() {
        no_links(&work.join("out"));
    }
});
