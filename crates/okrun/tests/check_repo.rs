//! `ok self check` must find nothing in valid code: every example, every
//! golden case that is not a deliberate error (`err_*`), the smoke test and
//! the repository's own `ok.ok`, and each example project as a whole. A
//! finding here is a false positive of the checker, to be fixed in the
//! checker, never by silencing it.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn ok_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "out") {
                continue;
            }
            ok_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ok")
            || path.file_name().is_some_and(|n| n == ".ok")
        {
            out.push(path);
        }
    }
}

fn check(dir: &Path, files: &[String]) -> (bool, String) {
    let o = Command::new(env!("CARGO_BIN_EXE_ok"))
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .args(["self", "check"])
        .args(files)
        .output()
        .expect("run ok");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.success() && !text.contains("warning"), text)
}

#[test]
fn valid_files_have_no_findings() {
    let root = root();
    let mut files = vec![root.join("ok.ok"), root.join("tests/smoke/smoke.ok")];
    ok_files(&root.join("examples"), &mut files);
    ok_files(&root.join("tests/cases"), &mut files);
    files.retain(|f| {
        !f.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("err_"))
    });
    let rel: Vec<String> = files
        .iter()
        .map(|f| {
            f.strip_prefix(&root)
                .unwrap_or(f)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(rel.len() > 60, "found only {} files", rel.len());
    let (ok, text) = check(&root, &rel);
    assert!(ok, "findings in valid code:\n{text}");
}

#[test]
fn example_projects_have_no_findings() {
    let root = root();
    for dir in [
        ".",
        "examples/codegen-build",
        "examples/deploy",
        "examples/docker-image",
        "examples/monorepo",
        "examples/monorepo/frontend",
    ] {
        let (ok, text) = check(&root.join(dir), &[]);
        assert!(ok, "findings in the project {dir}:\n{text}");
        assert!(text.contains("no problems found"), "{dir}: {text}");
    }
}
