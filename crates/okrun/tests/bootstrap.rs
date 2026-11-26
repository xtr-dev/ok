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
    // The starter requires the running version's major.minor.
    let mut version = env!("CARGO_PKG_VERSION").split('.');
    let requires = format!(
        "requires \"{}.{}\"\n",
        version.next().unwrap(),
        version.next().unwrap()
    );
    let starter = std::fs::read_to_string(project.join("ok.ok")).unwrap();
    assert!(starter.starts_with(&requires), "{starter}");
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
    // The second run uses the cached binary, kept under the version and
    // the start of its checksum.
    assert!(run(&["self", "version"]).status.success());
    let cached: Vec<String> = std::fs::read_dir(cache.join("ok"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(cached.len(), 1, "{cached:?}");
    assert!(
        cached[0].starts_with(&format!("{version}-")) && cached[0].len() == version.len() + 17,
        "{cached:?}"
    );
    let ps1 = std::fs::read_to_string(project.join("ok.ps1")).unwrap();
    assert!(ps1.contains("[guid]::NewGuid()"), "a unique download name");
    let cmd = std::fs::read_to_string(project.join("ok.cmd")).unwrap();
    assert!(
        cmd.contains("\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\""),
        "{cmd}"
    );

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

#[test]
fn init_accepts_only_hex_checksums_and_https_downloads() {
    let base = std::env::temp_dir().join(format!("ok-init-sums-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let ok = Path::new(env!("CARGO_BIN_EXE_ok"));
    let good = "ab".repeat(32);
    let mut sums = String::new();
    for (i, (_, file)) in okrun::tools::TARGETS.iter().enumerate() {
        // One entry is shell code of the right length instead of a digest.
        let hash = if i == 0 {
            format!("$(reboot){}", "0".repeat(55))
        } else {
            good.clone()
        };
        sums.push_str(&format!("{hash}  {file}\n"));
    }
    std::fs::write(base.join("SHA256SUMS"), &sums).unwrap();
    let o = Command::new(ok)
        .args(["self", "init", "--checksums", "SHA256SUMS"])
        .current_dir(&base)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(err.contains("is not a SHA-256 digest"), "{err}");
    assert!(!base.join("ok").exists(), "no wrapper is written");

    // Upper-case digests are accepted and written in lower case.
    let upper = sums
        .lines()
        .skip(1)
        .map(|l| format!("{l}\n"))
        .collect::<String>();
    let first = okrun::tools::TARGETS[0].1;
    std::fs::write(
        base.join("SHA256SUMS"),
        format!("{}  {first}\n{upper}", "CD".repeat(32)),
    )
    .unwrap();
    let o = Command::new(ok)
        .args(["self", "init", "--checksums", "SHA256SUMS"])
        .current_dir(&base)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let wrappers = std::fs::read_to_string(base.join("ok")).unwrap()
        + &std::fs::read_to_string(base.join("ok.ps1")).unwrap();
    assert!(!wrappers.contains("CD"), "normalised to lower case");

    // Without a file, the checksums must come over https.
    let empty = base.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let o = Command::new(ok)
        .args(["self", "init"])
        .env("OK_DOWNLOAD_URL", "http://mirror.example/ok")
        .current_dir(&empty)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!o.status.success());
    assert!(err.contains("must be downloaded over https"), "{err}");
    let _ = std::fs::remove_dir_all(&base);
}
