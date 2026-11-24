//! End-to-end checks of the placeholder binary. These run the real `ok`
//! executable that Cargo builds for integration tests.

use std::process::Command;

fn ok() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ok"))
}

#[test]
fn self_version_prints_the_crate_version() {
    let out = ok().args(["self", "version"]).output().expect("run ok");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    assert_eq!(stdout.trim_end(), format!("ok {}", ok_lang::VERSION));
}

#[test]
fn unimplemented_commands_fail_with_a_clear_message() {
    let out = ok().arg("build").output().expect("run ok");
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).expect("utf-8");
    assert!(stderr.contains("not implemented yet"));
}
