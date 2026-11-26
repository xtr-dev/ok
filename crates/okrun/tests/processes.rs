//! Commands with a timeout and the processes they start (SPEC.md 9.6), and
//! failures inside pipelines of commands (9.7). These depend on timing, so
//! they live here rather than in the golden cases, with generous limits.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

/// The directory with `ok` and the `ok-fixture` test program, first on PATH.
fn fixture_path() -> std::ffi::OsString {
    let bin_dir = Path::new(env!("CARGO_BIN_EXE_ok"))
        .parent()
        .expect("bin dir")
        .to_path_buf();
    let fixture = bin_dir.join(format!("ok-fixture{}", std::env::consts::EXE_SUFFIX));
    if !fixture.exists() {
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "ok-fixture"])
            .status()
            .expect("build ok-fixture");
        assert!(status.success(), "could not build ok-fixture");
    }
    let mut dirs = vec![bin_dir];
    dirs.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(dirs).expect("join PATH")
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ok-processes-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Run `script` with ok in `dir`; returns the output and how long it took.
fn run_script(dir: &Path, script: &str) -> (Output, Duration) {
    std::fs::write(dir.join("s.ok"), script).unwrap();
    let started = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_ok"))
        .arg("s.ok")
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .env("PATH", fixture_path())
        .output()
        .expect("run ok");
    (out, started.elapsed())
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).replace("\r\n", "\n")
}

/// Whether the process with this id has ended, waiting up to a few seconds
/// for it to be reaped. Only Linux shows processes as files.
#[cfg(target_os = "linux")]
fn gone(pid_file: &Path) -> bool {
    let pid = std::fs::read_to_string(pid_file).expect("pid file");
    let proc_dir = PathBuf::from(format!("/proc/{}", pid.trim()));
    let until = Instant::now() + Duration::from_secs(5);
    while Instant::now() < until {
        match std::fs::read_to_string(proc_dir.join("stat")) {
            // Gone, or a zombie waiting for init to reap it.
            Err(_) => return true,
            Ok(stat) if stat.contains(") Z ") => return true,
            Ok(_) => std::thread::sleep(Duration::from_millis(50)),
        }
    }
    false
}

/// A command that started a long-running child, in each position of
/// SPEC.md 9.5: the timeout stops both, and promptly, although the child
/// holds the output pipes open.
#[test]
fn timeout_stops_the_command_and_the_processes_it_started() {
    let dir = scratch("timeout");
    let cases = [
        (
            "captured",
            "let r = try { run ok-fixture spawn pid sleep 30000 with {timeout: 300ms} } catch e { \"{e.kind}: {e.message}\" }\nprint(r)\n",
        ),
        (
            "live",
            "try { run ok-fixture spawn pid sleep 30000 with {timeout: 300ms} } catch e { print(\"{e.kind}: {e.message}\") }\n",
        ),
        (
            "piped into a command",
            "let r = try { let x = run ok-fixture spawn pid sleep 30000 with {timeout: 300ms} | run ok-fixture cat } catch e { \"{e.kind}: {e.message}\" }\nprint(r)\n",
        ),
        (
            "piped",
            "let r = try { run ok-fixture spawn pid sleep 30000 with {timeout: 300ms} | lines } catch e { \"{e.kind}: {e.message}\" }\nprint(r)\n",
        ),
    ];
    for (name, script) in cases {
        let _ = std::fs::remove_file(dir.join("pid"));
        let (out, took) = run_script(&dir, script);
        assert_eq!(
            text(&out.stdout),
            "timeout: command timed out after 300ms\n",
            "{name}: {}",
            text(&out.stderr)
        );
        assert!(took < Duration::from_secs(15), "{name} took {took:?}");
        #[cfg(target_os = "linux")]
        assert!(gone(&dir.join("pid")), "{name}: the child survived");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A command that finishes in time keeps its output.
#[test]
fn timeout_that_does_not_expire_changes_nothing() {
    let dir = scratch("in-time");
    let (out, _) = run_script(
        &dir,
        "print(run(\"ok-fixture\", \"lines\", \"2\", timeout: 20s).out)\nprint(run ok-fixture lines 2 with {timeout: 20s} | lines)\nrun ok-fixture echo live with {timeout: 20s}\n",
    );
    assert_eq!(
        text(&out.stdout),
        "line 1\nline 2\n[\"line 1\", \"line 2\"]\nlive\n",
        "{}",
        text(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}
