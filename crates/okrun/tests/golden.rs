//! Golden tests (CONTRIBUTING.md): every `tests/cases/NAME.ok` is run with
//! the real `ok` binary from the `tests/cases` directory, and its stdout,
//! stderr and exit code are compared with `NAME.stdout`, `NAME.stderr`
//! (absent means empty) and `NAME.exit` (absent means 0).
//!
//! Optional `NAME.args` holds extra command-line arguments, one per line.
//!
//! Regenerate the expectations with `OK_BLESS=1 cargo test --test golden`
//! (or `ok bless`) and review every diff before committing it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/cases")
}

fn read_or_empty(p: &Path) -> String {
    fs::read_to_string(p)
        .unwrap_or_default()
        .replace("\r\n", "\n")
}

/// Write `content` to `p`, or remove `p` when the content is the default.
fn bless(p: &Path, content: &str, default: &str) {
    if content == default {
        let _ = fs::remove_file(p);
    } else {
        fs::write(p, content).expect("write expectation");
    }
}

#[test]
fn golden_cases() {
    let dir = cases_dir();
    let mut cases: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("tests/cases exists")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "ok"))
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "no golden cases found in {}",
        dir.display()
    );
    let blessing = std::env::var_os("OK_BLESS").is_some();
    let mut failures = Vec::new();

    for case in &cases {
        let name = case.file_name().unwrap().to_string_lossy().to_string();
        let stem = case.file_stem().unwrap().to_string_lossy().to_string();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ok"));
        cmd.current_dir(&dir).env("NO_COLOR", "1").arg(&name);
        let args_file = dir.join(format!("{stem}.args"));
        if args_file.exists() {
            cmd.args(read_or_empty(&args_file).lines().filter(|l| !l.is_empty()));
        }
        let out = cmd.output().expect("run ok");
        let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
        let stderr = String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n");
        let code = out.status.code().unwrap_or(-1).to_string();

        let p_out = dir.join(format!("{stem}.stdout"));
        let p_err = dir.join(format!("{stem}.stderr"));
        let p_exit = dir.join(format!("{stem}.exit"));
        if blessing {
            bless(&p_out, &stdout, "");
            bless(&p_err, &stderr, "");
            bless(&p_exit, &format!("{code}\n"), "0\n");
            continue;
        }
        let want_out = read_or_empty(&p_out);
        let want_err = read_or_empty(&p_err);
        let want_code = match read_or_empty(&p_exit).trim() {
            "" => "0".to_string(),
            c => c.to_string(),
        };
        let mut problems = Vec::new();
        if stdout != want_out {
            problems.push(format!(
                "stdout differs\n--- expected\n{want_out}--- actual\n{stdout}"
            ));
        }
        if stderr != want_err {
            problems.push(format!(
                "stderr differs\n--- expected\n{want_err}--- actual\n{stderr}"
            ));
        }
        if code != want_code {
            problems.push(format!("exit code: expected {want_code}, got {code}"));
        }
        if !problems.is_empty() {
            failures.push(format!("== {name}\n{}", problems.join("\n")));
        }
    }
    assert!(
        failures.is_empty(),
        "{} golden case(s) failed (bless with OK_BLESS=1 after reviewing):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
