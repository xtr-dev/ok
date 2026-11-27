//! End-to-end checks of the `ok` command line. These run the real binary
//! that Cargo builds for integration tests.

use std::process::{Command, Output};

/// Run ok from an empty directory, outside any project.
fn ok(args: &[&str]) -> Output {
    let dir = std::env::temp_dir().join(format!("ok-cli-tests-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    Command::new(env!("CARGO_BIN_EXE_ok"))
        .args(args)
        .current_dir(&dir)
        .env("NO_COLOR", "1")
        .output()
        .expect("run ok")
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n")
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).replace("\r\n", "\n")
}

#[test]
fn self_version_prints_the_crate_version() {
    for args in [&["self", "version"][..], &["--version"]] {
        let out = ok(args);
        assert!(out.status.success());
        assert_eq!(stdout(&out).trim_end(), format!("ok {}", okrun::VERSION));
    }
}

#[test]
fn eval_prints_the_final_value() {
    let out = ok(&["-e", "1 + 2"]);
    assert_eq!(stdout(&out), "3\n");
    assert!(out.status.success());
    let out = ok(&["-e", "let x = 5\nprint(\"x is {x}\")"]);
    assert_eq!(stdout(&out), "x is 5\n");
}

#[test]
fn eval_errors_name_the_source() {
    let out = ok(&["-e", "nope"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(stderr(&out).contains("--> <-e>:1:1"), "{}", stderr(&out));
}

#[test]
fn eval_gets_arguments() {
    let out = ok(&["-e", "args", "a", "b"]);
    assert_eq!(stdout(&out), "[\"a\", \"b\"]\n");
}

#[test]
fn usage_errors_exit_with_2() {
    let out = ok(&["--trce"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("did you mean --trace?"));
    let out = ok(&["missing.ok"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("does not exist"));
    let out = ok(&["build"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("no project found in "));
    assert!(stderr(&out).contains("(no ok.ok, .ok or .ok/ok.ok), so `build` is not a task"));
}

#[test]
fn no_arguments_prints_usage() {
    let out = ok(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stdout(&out).contains("Usage:"));
    let out = ok(&["--help"]);
    assert!(out.status.success());
}

#[test]
fn startup_is_fast() {
    // SPEC goal: under 10 ms for a small script. Measured loosely here (CI
    // machines vary); a regression to hundreds of milliseconds fails.
    let start = std::time::Instant::now();
    for _ in 0..5 {
        let out = ok(&["-e", "1"]);
        assert!(out.status.success());
    }
    let each = start.elapsed() / 5;
    assert!(each.as_millis() < 250, "ok -e took {each:?} per run");
}

#[cfg(unix)]
#[test]
fn shebang_scripts_run_directly() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("ok-shebang-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let script = dir.join("hello");
    std::fs::write(
        &script,
        "#!/usr/bin/env ok\n# Say hello.\nfn main(name: string) { print(\"hello {name}\") }\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let bin_dir = std::path::Path::new(env!("CARGO_BIN_EXE_ok"))
        .parent()
        .unwrap()
        .to_path_buf();
    let mut paths = vec![bin_dir];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let out = Command::new(&script)
        .arg("bas")
        .env("PATH", std::env::join_paths(paths).unwrap())
        .output()
        .expect("run script");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(stdout(&out), "hello bas\n");
}

#[test]
fn dry_run_shows_every_redirect() {
    let out = ok(&[
        "--dry-run",
        "-e",
        "run tool a < in.txt > out.txt 2>&1\nrun tool b >> log 2>> errors.log\nrun tool c 2> {path(\"logs/err.txt\")}",
    ]);
    assert!(out.status.success(), "{}", stderr(&out));
    assert_eq!(
        stderr(&out),
        "~ tool a < in.txt > out.txt 2>&1\n~ tool b >> log 2>> errors.log\n~ tool c 2> logs/err.txt\n"
    );
}

#[test]
fn self_and_trust_print_their_help() {
    for args in [&["self", "--help"][..], &["self", "-h"]] {
        let out = ok(args);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(stdout(&out).starts_with("ok self: "), "{}", stdout(&out));
        assert!(stdout(&out).contains("  ok self check [--strict] [FILES...]"));
    }
    for args in [
        &["trust", "--help"][..],
        &["trust", "-h"],
        &["untrust", "--help"],
    ] {
        let out = ok(args);
        assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
        assert!(stdout(&out).starts_with("ok trust: "), "{}", stdout(&out));
        assert!(stdout(&out).contains("  ok untrust [DIR]"));
    }
}

/// `remove` refuses the home directory and every directory that holds it,
/// such as `/home` (SPEC.md 12.1).
#[test]
fn remove_refuses_directories_that_hold_the_home_directory() {
    let base = std::env::temp_dir().join(format!("ok-remove-home-{}", std::process::id()));
    let home = base.join("homes").join("me");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(base.join("homes").join("other")).unwrap();
    let script = "for p in [\"homes\", \"homes/me\", \"homes/other\"] { print(try { remove(p) } catch e { e.message }) }\n";
    let out = Command::new(env!("CARGO_BIN_EXE_ok"))
        .args(["-e", script])
        .current_dir(&base)
        .env("NO_COLOR", "1")
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .output()
        .expect("run ok");
    assert_eq!(
        stdout(&out),
        "refusing to remove homes because it is a directory that contains your home directory\nrefusing to remove homes/me because it is your home directory\ntrue\n",
        "{}",
        stderr(&out)
    );
    assert!(home.is_dir());
    let _ = std::fs::remove_dir_all(&base);
}
