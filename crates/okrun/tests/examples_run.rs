//! Run the M0 examples (SPEC.md): each case copies an
//! example to a temporary directory and runs it with **fake tools** on PATH
//! (copies of `ok-fixture` named `git`, `docker`, `psql`, ...), so the results
//! are the same on every OS and need no real tools. The expected output is in
//! `tests/examples/NAME.stdout`, `.stderr` and `.exit`; regenerate with
//! `OK_BLESS=1 cargo test --test examples_run` and review the diff.
//!
//! Examples that call fixed internet hosts (`deploy`, `stale-bugs`,
//! `install-tool`) are only parsed (see `examples_parse.rs`), not run.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct Case {
    name: &'static str,
    example: &'static str,
    args: Vec<String>,
    env: Vec<(&'static str, String)>,
    fakes: &'static str,
    setup: fn(&Path),
    /// Files whose content is appended to stdout after the run.
    show: &'static [&'static str],
    /// Extra (regex, replacement) pairs for output that varies per run.
    normalize: &'static [(&'static str, &'static str)],
}

fn none(_: &Path) {}

const FAKE_TOOLS: &[&str] = &[
    "git", "cargo", "docker", "npm", "node", "psql", "pg_dump", "rustfmt", "sqlx", "scp", "ssh",
];

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap().flatten() {
        let p = e.path();
        let d = dst.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &d);
        } else {
            std::fs::copy(&p, &d).unwrap();
        }
    }
}

/// A tiny HTTP server answering 200 to everything, for `wait-for`.
fn http_server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ =
                s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
    port
}

fn cases(port: u16) -> Vec<Case> {
    let a = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    vec![
        Case {
            name: "csv-to-json",
            example: "csv-to-json",
            args: a(&[
                "csv-to-json.ok",
                "users.csv",
                "--out",
                "active.json",
                "--verbose",
            ]),
            env: vec![],
            fakes: "{}",
            setup: none,
            show: &["active.json"],
            normalize: &[],
        },
        Case {
            name: "log-summary",
            example: "log-summary",
            args: a(&["log-summary.ok", "--top", "2"]),
            env: vec![],
            fakes: "{}",
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "config-render",
            example: "config-render",
            args: a(&["render-config.ok", "production"]),
            env: vec![("DB_PASSWORD", "s3cret".into())],
            fakes: "{}",
            setup: none,
            show: &["build/config.production.json"],
            normalize: &[],
        },
        Case {
            name: "largest-files",
            example: "largest-files",
            args: a(&["largest-files.ok", ".", "--top", "3"]),
            env: vec![],
            fakes: "{}",
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "tidy-names",
            example: "tidy-names",
            args: a(&["tidy-names.ok", "photos"]),
            env: vec![],
            fakes: "{}",
            setup: |d| {
                std::fs::create_dir_all(d.join("photos")).unwrap();
                for f in [
                    "Summer Holiday_01.JPG",
                    "2024-05-01-already-dated.png",
                    "notes.txt",
                ] {
                    std::fs::write(d.join("photos").join(f), "x").unwrap();
                }
            },
            show: &[],
            normalize: &[(r"\d{4}-\d{2}-\d{2}-(summer|notes)", "<date>-$1")],
        },
        Case {
            name: "codegen-build",
            example: "codegen-build",
            args: a(&["gen"]),
            env: vec![("OK_TRUST", "1".into())],
            fakes: "{}",
            setup: none,
            show: &["src/gen.rs"],
            normalize: &[],
        },
        Case {
            name: "env-check",
            example: "env-check",
            args: a(&["check-env.ok"]),
            env: vec![("DATABASE_URL", "postgres://localhost/app".into())],
            fakes: r#"{"node": [{"args": ["--version"], "stdout": "v18.19.0\n"}]}"#,
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "test-matrix",
            example: "test-matrix",
            args: a(&["test-matrix.ok"]),
            env: vec![],
            fakes: r#"{"cargo": [{"args": ["test", "--no-default-features", "--features", "yaml"], "stderr": "error: test parser::yaml failed\n", "exit": 101}]}"#,
            setup: none,
            show: &[],
            normalize: &[(r"\d+\.\ds\n", "<secs>\n")],
        },
        Case {
            name: "backup-rotation",
            example: "backup-rotation",
            args: a(&[
                "backup.ok",
                "--src",
                "data",
                "--dest",
                "backups",
                "--keep",
                "2",
            ]),
            env: vec![],
            fakes: "{}",
            setup: |d| {
                std::fs::create_dir_all(d.join("data")).unwrap();
                std::fs::write(d.join("data/db.sqlite"), "pretend database").unwrap();
                std::fs::create_dir_all(d.join("backups")).unwrap();
                for (i, old) in [
                    "data-2026-01-01_000000.tar.gz",
                    "data-2026-01-02_000000.tar.gz",
                ]
                .iter()
                .enumerate()
                {
                    std::fs::write(d.join("backups").join(old), "old").unwrap();
                    std::fs::write(d.join("backups").join(format!("{old}.sha256")), "old").unwrap();
                    // Give them distinct, older modification times.
                    let t = std::time::SystemTime::now()
                        - std::time::Duration::from_secs(3600 * (2 - i as u64));
                    std::fs::File::options()
                        .append(true)
                        .open(d.join("backups").join(old))
                        .unwrap()
                        .set_modified(t)
                        .unwrap();
                }
            },
            show: &[],
            normalize: &[(r"data-\d{4}-\d{2}-\d{2}_\d{6}", "data-<stamp>")],
        },
        Case {
            name: "docker-image",
            example: "docker-image",
            args: a(&["push"]),
            env: vec![("OK_TRUST", "1".into())],
            fakes: r#"{"git": [{"args": ["describe"], "stdout": "v1.4.0\n"}, {"args": ["rev-parse", "--short"], "stdout": "3f2a9c1\n"}]}"#,
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "db-dump",
            example: "db-dump",
            args: a(&["dump-all.ok"]),
            env: vec![],
            fakes: r#"{"psql": [{"stdout": "app\nanalytics\n"}], "pg_dump": [{"args": ["--no-owner", "app"], "stdout": "-- dump of app\n"}, {"stdout": "-- dump of another database\n"}]}"#,
            setup: none,
            show: &[],
            normalize: &[
                (r"\d{4}-\d{2}-\d{2}", "<date>"),
                (r"(?m)^ +[\d,]+  ", "<size>  "),
            ],
        },
        Case {
            name: "pre-commit",
            example: "pre-commit",
            args: a(&["pre-commit.ok"]),
            env: vec![],
            fakes: r#"{"git": [
                {"args": ["diff"], "stdout": "src/main.rs\nweb/app.js\nassets/video.mp4\n"},
                {"args": ["grep", "--cached", "-q", "-E", "dbg!\\(|console\\.log\\(", "--", "web/app.js"], "exit": 0},
                {"args": ["grep"], "exit": 1},
                {"args": ["cat-file", "-s", ":assets/video.mp4"], "stdout": "5242880\n"},
                {"args": ["cat-file"], "stdout": "2048\n"}
              ], "rustfmt": [{"exit": 1}]}"#,
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "release",
            example: "release",
            args: a(&["release.ok", "1.3.0"]),
            env: vec![],
            fakes: r#"{"git": [
                {"args": ["rev-parse", "--abbrev-ref", "HEAD"], "stdout": "main\n"},
                {"args": ["status", "--porcelain"], "stdout": ""},
                {"args": ["rev-parse", "--verify"], "exit": 1}
              ], "cargo": [
                {"args": ["build", "--release", "--target", "x86_64-unknown-linux-musl"], "write": {"target/x86_64-unknown-linux-musl/release/myapp": "x86 binary"}},
                {"args": ["build", "--release", "--target", "aarch64-unknown-linux-musl"], "write": {"target/aarch64-unknown-linux-musl/release/myapp": "arm binary"}}
              ]}"#,
            setup: |d| {
                std::fs::write(d.join("Cargo.toml"), "[package]\nname = \"myapp\"\nversion = \"1.2.9\"\n\n[dependencies]\nserde = { version = \"1\" }\n").unwrap()
            },
            show: &["Cargo.toml", "dist/SHA256SUMS"],
            normalize: &[],
        },
        Case {
            name: "wait-for",
            example: "wait-for",
            args: vec![
                "wait-for.ok".into(),
                format!("http://127.0.0.1:{port}/health"),
                "--timeout".into(),
                "5s".into(),
                "--".into(),
                "ok-fixture".into(),
                "echo".into(),
                "service is ready".into(),
            ],
            env: vec![],
            fakes: "{}",
            setup: none,
            show: &[],
            normalize: &[
                (r"127\.0\.0\.1:\d+", "127.0.0.1:<port>"),
                (r"up after [0-9.]+m?s", "up after <time>"),
            ],
        },
        Case {
            name: "monorepo-list",
            example: "monorepo",
            args: vec![],
            env: vec![],
            fakes: "{}",
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "monorepo-db-reset",
            example: "monorepo",
            args: a(&["--yes", "db", "reset"]),
            env: vec![
                ("OK_TRUST", "1".into()),
                ("DATABASE_URL", "postgres://localhost/app".into()),
            ],
            fakes: "{}",
            setup: none,
            show: &[],
            normalize: &[],
        },
        Case {
            name: "monorepo-build",
            example: "monorepo",
            args: a(&["build"]),
            env: vec![("OK_TRUST", "1".into())],
            fakes: r#"{"npm": [{"args": ["ci"], "write": {"node_modules/.package-lock.json": "{}"}}]}"#,
            setup: |d| std::fs::write(d.join("frontend/package-lock.json"), "{}").unwrap(),
            show: &[],
            normalize: &[],
        },
    ]
}

fn normalize(text: &str, work: &Path, extra: &[(&str, &str)]) -> String {
    let mut t = text.replace("\r\n", "\n");
    // Replace the longer spelling first: on macOS the canonical form of a
    // temp dir is `/private/var/...`, which contains the plain `/var/...`.
    let mut spellings = vec![
        work.to_path_buf(),
        std::fs::canonicalize(work).unwrap_or_default(),
    ];
    spellings.sort_by_key(|w| std::cmp::Reverse(w.as_os_str().len()));
    for w in spellings {
        let s = w.to_string_lossy().to_string();
        if !s.is_empty() {
            t = t
                .replace(&s, "<work>")
                .replace(&s.replace('\\', "/"), "<work>");
        }
    }
    for (re, rep) in extra {
        t = regex::Regex::new(re)
            .unwrap()
            .replace_all(&t, *rep)
            .into_owned();
    }
    t
}

#[test]
fn examples_run_with_fake_tools() {
    let ok = PathBuf::from(env!("CARGO_BIN_EXE_ok"));
    let bin_dir = ok.parent().unwrap().to_path_buf();
    let fixture = bin_dir.join(format!("ok-fixture{}", std::env::consts::EXE_SUFFIX));
    if !fixture.exists() {
        let status = Command::new(env!("CARGO"))
            .args(["build", "-p", "ok-fixture"])
            .status()
            .unwrap();
        assert!(status.success());
    }
    let base = std::env::temp_dir().join(format!("ok-examples-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let fakes = base.join("fakes");
    std::fs::create_dir_all(&fakes).unwrap();
    for tool in FAKE_TOOLS {
        std::fs::copy(
            &fixture,
            fakes.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)),
        )
        .unwrap();
    }
    std::fs::copy(
        &fixture,
        fakes.join(format!("ok-fixture{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    std::fs::copy(
        &ok,
        fakes.join(format!("ok{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
    let path = std::env::join_paths([fakes.clone()]).unwrap();
    let expect_dir = repo().join("tests/examples");
    std::fs::create_dir_all(&expect_dir).unwrap();
    let blessing = std::env::var_os("OK_BLESS").is_some();
    let port = http_server();
    let mut failures = Vec::new();

    for case in cases(port) {
        let work = base.join("work").join(case.name);
        copy_dir(&repo().join("examples").join(case.example), &work);
        (case.setup)(&work);
        let rules = base.join(format!("{}.fakes.json", case.name));
        std::fs::write(&rules, case.fakes).unwrap();
        let mut cmd = Command::new(&ok);
        cmd.args(&case.args)
            .current_dir(&work)
            .env_clear()
            .env("PATH", &path)
            .env("OK_FAKES", &rules)
            .env("OK_CONFIG_DIR", base.join("config"))
            .env("NO_COLOR", "1")
            .env("TZ", "UTC")
            .env("HOME", base.join("home"))
            .stdin(Stdio::null());
        for var in [
            "SystemRoot",
            "TEMP",
            "TMP",
            "USERPROFILE",
            "APPDATA",
            "LOCALAPPDATA",
            "PATHEXT",
        ] {
            if let Some(v) = std::env::var_os(var) {
                cmd.env(var, v);
            }
        }
        for (k, v) in &case.env {
            cmd.env(k, v);
        }
        let out = cmd.output().expect("run ok");
        let mut stdout = String::from_utf8_lossy(&out.stdout).to_string();
        for f in case.show {
            let content =
                std::fs::read_to_string(work.join(f)).unwrap_or_else(|_| "<missing>\n".into());
            stdout.push_str(&format!("==> {f} <==\n{content}"));
        }
        let stdout = normalize(&stdout, &work, case.normalize);
        let stderr = normalize(&String::from_utf8_lossy(&out.stderr), &work, case.normalize);
        let code = out.status.code().unwrap_or(-1).to_string();
        let files = [
            ("stdout", stdout, String::new()),
            ("stderr", stderr, String::new()),
            ("exit", format!("{code}\n"), "0\n".to_string()),
        ];
        for (ext, actual, default) in files {
            let p = expect_dir.join(format!("{}.{ext}", case.name));
            if blessing {
                if actual == default {
                    let _ = std::fs::remove_file(&p);
                } else {
                    std::fs::write(&p, &actual).unwrap();
                }
                continue;
            }
            let want = std::fs::read_to_string(&p)
                .map(|s| s.replace("\r\n", "\n"))
                .unwrap_or(default);
            if want != actual {
                failures.push(format!(
                    "== {} ({ext})\n--- expected\n{want}--- actual\n{actual}",
                    case.name
                ));
            }
        }
    }
    let _ = std::fs::remove_dir_all(&base);
    assert!(
        failures.is_empty(),
        "{} example output(s) differ (bless with OK_BLESS=1 after reviewing):\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
