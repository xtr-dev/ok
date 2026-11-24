//! End-to-end checks of the `ok` command line. These run the real binary
//! that Cargo builds for integration tests.

use std::process::{Command, Output};

fn ok(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ok"))
        .args(args)
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
    assert!(stderr(&out).contains("M5"));
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
