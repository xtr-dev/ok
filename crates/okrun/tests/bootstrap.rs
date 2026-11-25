//! `ok self init` and the `./ok` wrapper (SPEC.md 18), end to end: a local
//! directory stands in for the GitHub release, so the wrapper's download,
//! checksum check and cache are exercised without network access.

#![cfg(unix)]

use std::path::Path;
use std::process::Command;

use sha2::Digest;

fn sha256(p: &Path) -> String {
    hex::encode(sha2::Sha256::digest(std::fs::read(p).unwrap()))
}

#[test]
fn a_fresh_project_runs_through_the_wrapper() {
    if Command::new("curl").arg("--version").output().is_err() {
        eprintln!("skipping: curl is not installed");
        return;
    }
    let base = std::env::temp_dir().join(format!("ok-bootstrap-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let release = base.join("release");
    let project = base.join("project");
    let cache = base.join("cache");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::create_dir_all(&project).unwrap();

    // A fake release: this build of ok under every Unix target name.
    let ok = Path::new(env!("CARGO_BIN_EXE_ok"));
    let mut sums = String::new();
    for (_, file) in okrun::tools::TARGETS {
        let dst = release.join(file);
        let here = format!(
            "{}-{}",
            std::env::consts::ARCH,
            if cfg!(target_os = "macos") {
                "apple-darwin"
            } else {
                "unknown-linux-musl"
            }
        );
        if !file.contains(&here) {
            std::fs::write(&dst, b"not used on this machine").unwrap();
        } else {
            std::fs::copy(ok, &dst).unwrap();
        }
        sums.push_str(&format!("{}  {file}\n", sha256(&dst)));
    }
    std::fs::write(release.join("SHA256SUMS"), &sums).unwrap();

    let init = Command::new(ok)
        .args(["self", "init", "--checksums"])
        .arg(release.join("SHA256SUMS"))
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    for f in ["ok.ok", "ok", "ok.cmd", "ok.ps1"] {
        assert!(project.join(f).exists(), "{f} written");
    }
    let again = Command::new(ok)
        .args(["self", "init", "--checksums"])
        .arg(release.join("SHA256SUMS"))
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(!again.status.success(), "init never overwrites");

    // Only the wrapper: PATH has no ok, just the basic tools the wrapper uses.
    let run = |args: &[&str]| {
        Command::new(project.join("ok"))
            .args(args)
            .current_dir(&project)
            .env("PATH", "/usr/bin:/bin")
            .env("OK_DOWNLOAD_URL", format!("file://{}", release.display()))
            .env("XDG_CACHE_HOME", &cache)
            .env("OK_CONFIG_DIR", base.join("config"))
            .env("OK_TRUST", "1")
            .output()
            .unwrap()
    };
    let out = run(&["hello", "wrapper"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hello, wrapper!\n");
    // The second run uses the cached binary.
    assert!(run(&["self", "version"]).status.success());

    // A tampered binary is refused.
    std::fs::remove_dir_all(&cache).unwrap();
    for (_, file) in okrun::tools::TARGETS {
        if !file.ends_with(".exe") {
            std::fs::write(release.join(file), b"#!/bin/sh\necho evil\n").unwrap();
        }
    }
    let bad = run(&["hello"]);
    assert!(!bad.status.success());
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("checksum mismatch"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
    let _ = std::fs::remove_dir_all(&base);
}
