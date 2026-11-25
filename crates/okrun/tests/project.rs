//! Project mode and trust, end to end (SPEC.md sections 15 and 16). Each
//! test builds a small project in a temporary directory and runs the real
//! `ok` binary with its own trust store.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Proj {
    dir: PathBuf,
    store: PathBuf,
}

impl Proj {
    fn new(name: &str, files: &[(&str, &str)]) -> Proj {
        let base = std::env::temp_dir().join(format!("ok-proj-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let dir = base.join("app");
        for (rel, content) in files {
            let p = dir.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, content).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        Proj {
            dir,
            store: base.join("config"),
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_ok"));
        c.args(args)
            .current_dir(&self.dir)
            .env("OK_CONFIG_DIR", &self.store)
            .env("NO_COLOR", "1")
            .env_remove("OK_TRUST")
            .stdin(Stdio::null());
        c
    }

    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().expect("run ok")
    }

    /// Run with trust skipped, as CI does.
    fn run_trusted(&self, args: &[&str]) -> Output {
        self.cmd(args)
            .env("OK_TRUST", "1")
            .output()
            .expect("run ok")
    }

    fn write(&self, rel: &str, content: &str) {
        std::fs::write(self.dir.join(rel), content).unwrap();
    }
}

impl Drop for Proj {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.dir.parent().unwrap());
    }
}

fn out(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).replace("\r\n", "\n")
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).replace("\r\n", "\n")
}

const BASIC: &str = r#"
# Generate things
task gen {
  print("gen")
}

# Build it
task build(--release: bool) needs [gen] {
  print("build release={release}")
}

task all needs [build, gen] {
  print("all")
}

task secret hidden {
  print("secret")
}
"#;

#[test]
fn untrusted_projects_cannot_run_tasks_without_approval() {
    let p = Proj::new("untrusted", &[("ok.ok", BASIC)]);
    let o = p.run(&["gen"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("this project is not trusted"),
        "{}",
        err(&o)
    );
    assert!(out(&o).is_empty(), "nothing may run: {}", out(&o));
    // Listing and help only parse, so they work without trust.
    assert!(p.run(&[]).status.success());
    assert!(p.run(&["build", "--help"]).status.success());
}

#[test]
fn trust_is_bound_to_content() {
    let p = Proj::new("content", &[("ok.ok", BASIC)]);
    let t = p.run(&["trust"]);
    assert!(t.status.success(), "{}", err(&t));
    assert!(out(&t).contains("sha256"));
    assert_eq!(out(&p.run(&["gen"])), "gen\n");
    p.write("ok.ok", &format!("{BASIC}\ntask extra {{ print(1) }}\n"));
    let o = p.run(&["gen"]);
    assert_eq!(o.status.code(), Some(1), "a change needs approval again");
    assert!(out(&p.run(&["trust", "--list"])).contains("sha256"));
    assert!(p.run(&["untrust"]).status.success());
    assert!(p.run(&["trust", "--dir"]).status.success());
    p.write("ok.ok", BASIC);
    assert_eq!(
        out(&p.run(&["gen"])),
        "gen\n",
        "a trusted directory runs whatever its content"
    );
}

#[test]
fn dependencies_run_first_and_once() {
    let p = Proj::new("needs", &[("ok.ok", BASIC)]);
    assert_eq!(
        out(&p.run_trusted(&["all"])),
        "gen\nbuild release=false\nall\n"
    );
    assert_eq!(
        out(&p.run_trusted(&["build", "--release"])),
        "gen\nbuild release=true\n"
    );
}

#[test]
fn listing_hides_hidden_tasks_and_shows_descriptions() {
    let p = Proj::new(
        "listing",
        &[
            ("ok.ok", BASIC),
            (
                "ok/db.ok",
                "# Apply migrations\ntask migrate { print(\"migrate\") }\n",
            ),
            ("ok/_lib.ok", "fn helper() { 1 }\n"),
        ],
    );
    let l = out(&p.run(&[]));
    assert!(l.contains("gen") && l.contains("Generate things"), "{l}");
    assert!(!l.contains("secret"), "{l}");
    assert!(l.contains("db\n  migrate"), "{l}");
    assert!(!l.contains("_lib"), "{l}");
    assert!(out(&p.run(&["self", "tasks", "--all"])).contains("secret"));
    assert_eq!(out(&p.run_trusted(&["db", "migrate"])), "migrate\n");
    assert_eq!(out(&p.run_trusted(&["secret"])), "secret\n");
}

#[test]
fn side_effects_at_the_top_level_fail_at_load_time() {
    let p = Proj::new(
        "toplevel",
        &[("ok.ok", "print(\"loading\")\ntask a { print(1) }\n")],
    );
    for args in [&[][..], &["a"]] {
        let o = p.run_trusted(args);
        assert_eq!(o.status.code(), Some(1));
        assert!(
            err(&o).contains("project files cannot run code at the top level"),
            "{}",
            err(&o)
        );
        assert!(!out(&o).contains("loading"));
    }
    let p = Proj::new(
        "toplevel-let",
        &[("ok.ok", "let x = read(\"f.txt\")\ntask a { print(x) }\n")],
    );
    assert!(err(&p.run_trusted(&["a"])).contains("must be a constant"));
}

#[test]
fn cycles_and_unknown_dependencies_are_load_errors() {
    let p = Proj::new(
        "cycle",
        &[("ok.ok", "task a needs [b] { }\ntask b needs [a] { }\n")],
    );
    let o = p.run_trusted(&["a"]);
    assert!(
        err(&o).contains("task dependency cycle: a -> b -> a"),
        "{}",
        err(&o)
    );
    let p = Proj::new(
        "unknown-dep",
        &[("ok.ok", "task a needs [bild] { }\ntask build { }\n")],
    );
    assert!(err(&p.run_trusted(&["a"])).contains("did you mean `build`?"));
}

#[test]
fn imports_cannot_leave_the_project() {
    let p = Proj::new(
        "confined",
        &[("ok.ok", "import \"../outside.ok\"\ntask a { print(1) }\n")],
    );
    std::fs::write(p.dir.parent().unwrap().join("outside.ok"), "fn f() { 1 }\n").unwrap();
    let o = p.run_trusted(&["a"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("outside the project"), "{}", err(&o));
}

#[cfg(unix)]
#[test]
fn symlinks_that_escape_the_project_are_refused() {
    let p = Proj::new("symlink", &[("ok.ok", "task a { print(1) }\n")]);
    let outside = p.dir.parent().unwrap().join("evil.ok");
    std::fs::write(&outside, "task boom { print(\"boom\") }\n").unwrap();
    std::fs::create_dir_all(p.dir.join("ok")).unwrap();
    std::os::unix::fs::symlink(&outside, p.dir.join("ok/evil.ok")).unwrap();
    let o = p.run_trusted(&["a"]);
    assert!(err(&o).contains("outside the project"), "{}", err(&o));
}

#[test]
fn confirm_needs_a_terminal_or_yes() {
    let p = Proj::new(
        "confirm",
        &[(
            "ok.ok",
            "task wipe confirm \"This wipes everything\" { print(\"wiped\") }\n",
        )],
    );
    let o = p.run_trusted(&["wipe"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(err(&o).contains("--yes"), "{}", err(&o));
    assert_eq!(out(&p.run_trusted(&["--yes", "wipe"])), "wiped\n");
}

#[test]
fn sources_and_outputs_skip_up_to_date_tasks() {
    let p = Proj::new(
        "uptodate",
        &[
            (
                "ok.ok",
                "task gen sources [\"in/*.txt\"] outputs [\"out/gen.txt\"] {\n  print(\"generating\")\n  write(\"x\", \"out/gen.txt\")\n}\n",
            ),
            ("in/a.txt", "a"),
        ],
    );
    assert_eq!(out(&p.run_trusted(&["gen"])), "generating\n");
    let o = p.run_trusted(&["gen"]);
    assert_eq!(out(&o), "");
    assert!(err(&o).contains("gen: up to date"));
    assert_eq!(out(&p.run_trusted(&["--force", "gen"])), "generating\n");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    p.write("in/a.txt", "changed");
    assert_eq!(out(&p.run_trusted(&["gen"])), "generating\n");
}

#[test]
fn env_files_and_env_flag() {
    let p = Proj::new(
        "envfile",
        &[
            (
                "ok.ok",
                "env file \".env\"\ntask show { print(env.get(\"A\"), env.get(\"B\"), cwd.name) }\n",
            ),
            (".env", "A=base\nB=base\n"),
            (".env.prod", "B=prod\n"),
        ],
    );
    assert_eq!(out(&p.run_trusted(&["show"])), "base base app\n");
    assert_eq!(
        out(&p.run_trusted(&["--env", "prod", "show"])),
        "base prod app\n"
    );
    assert!(err(&p.run_trusted(&["--env", "nope", "show"])).contains(".env.nope"));
}

#[test]
fn tasks_run_in_the_project_root_from_a_subdirectory() {
    let p = Proj::new(
        "subdir",
        &[
            (
                "ok.ok",
                "task where(file: path) { print(cwd.name, invocation_dir.name, file.name, exists(file)) }\n",
            ),
            ("sub/x.txt", "x"),
        ],
    );
    let o = p
        .cmd(&["where", "x.txt"])
        .current_dir(p.dir.join("sub"))
        .env("OK_TRUST", "1")
        .output()
        .unwrap();
    assert_eq!(out(&o), "app sub x.txt true\n", "{}", err(&o));
}

#[test]
fn mounted_sub_projects_run_in_their_directory() {
    let p = Proj::new(
        "mount",
        &[
            (
                "ok.ok",
                "import \"web/ok.ok\" as web\ntask all needs [web.build] { print(\"root\") }\n",
            ),
            (
                "web/ok.ok",
                "task deps hidden { print(\"deps in\", cwd.name) }\ntask build needs [deps] { print(\"build in\", cwd.name) }\n",
            ),
        ],
    );
    assert_eq!(
        out(&p.run_trusted(&["all"])),
        "deps in web\nbuild in web\nroot\n"
    );
    assert_eq!(
        out(&p.run_trusted(&["web", "build"])),
        "deps in web\nbuild in web\n"
    );
}

#[test]
fn unknown_tasks_and_namespaces_exit_with_2() {
    let p = Proj::new(
        "unknown",
        &[("ok.ok", BASIC), ("ok/db.ok", "task migrate { }\n")],
    );
    let o = p.run(&["buidl"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("did you mean `ok build`?"));
    let o = p.run(&["db", "migrat"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("ok db migrate"));
    let o = p.run(&["build", "--relase"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("did you mean --release?"));
}

#[test]
fn completions_list_tasks_namespaces_and_flags() {
    let p = Proj::new(
        "complete",
        &[("ok.ok", BASIC), ("ok/db.ok", "task migrate { }\n")],
    );
    assert_eq!(out(&p.run(&["self", "complete", "--", "b"])), "build\n");
    assert_eq!(
        out(&p.run(&["self", "complete", "--", "db", ""])),
        "migrate\n"
    );
    assert_eq!(
        out(&p.run(&["self", "complete", "--", "build", "--"])),
        "--help\n--release\n"
    );
    for shell in ["bash", "zsh", "fish", "powershell"] {
        assert!(p.run(&["self", "completions", shell]).status.success());
    }
}

#[test]
fn completions_offer_choices() {
    let p = Proj::new(
        "complete-choices",
        &[
            (
                "ok.ok",
                "task deploy(env: \"staging\" | \"production\", --role: \"admin\" | \"user\" = \"user\", --note: string?, --force: bool) { }\n",
            ),
            (
                "ok/db.ok",
                "task seed(--size: \"small\" | \"large\" = \"small\") { }\n",
            ),
        ],
    );
    let complete = |words: &[&str]| {
        let mut args = vec!["self", "complete", "--"];
        args.extend_from_slice(words);
        out(&p.run(&args))
    };
    // The first positional parameter has a choice type.
    assert_eq!(
        complete(&["deploy", ""]),
        "--force\n--help\n--note\n--role\nproduction\nstaging\n"
    );
    assert_eq!(complete(&["deploy", "st"]), "staging\n");
    // After a flag that takes a choice, only the choices.
    assert_eq!(complete(&["deploy", "--role", ""]), "admin\nuser\n");
    assert_eq!(
        complete(&["deploy", "--role="]),
        "--role=admin\n--role=user\n"
    );
    // After a flag that takes a free value, nothing.
    assert_eq!(complete(&["deploy", "--note", ""]), "");
    // A switch does not take the next word, so positional choices follow.
    assert_eq!(complete(&["deploy", "--force", "pro"]), "production\n");
    // Once the positional argument is given, no more choices.
    assert_eq!(complete(&["deploy", "staging", "pro"]), "");
    // Tasks in namespaces.
    assert_eq!(complete(&["db", "seed", "--size", ""]), "large\nsmall\n");
}

#[test]
fn tasks_take_short_flags() {
    let p = Proj::new(
        "short-flags",
        &[(
            "ok.ok",
            "# Build it\ntask build(--release, -r: bool, --target, -t: \"x86\" | \"arm\" = \"x86\", --jobs: int = 1) {\n  print(\"release={release} target={target} jobs={jobs}\")\n}\n",
        )],
    );
    assert_eq!(
        out(&p.run_trusted(&["build", "-r", "-t", "arm"])),
        "release=true target=arm jobs=1\n"
    );
    let help = out(&p.run_trusted(&["build", "-h"]));
    assert!(help.contains("  -r, --release"), "{help}");
    assert!(help.contains("      --jobs <int>"), "{help}");
    // ok's own -C after the task name gets the "put it first" hint.
    let o = p.run_trusted(&["build", "-C", "web"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("ok's own options go before the name"),
        "{}",
        err(&o)
    );
    let complete = |words: &[&str]| {
        let mut args = vec!["self", "complete", "--"];
        args.extend_from_slice(words);
        out(&p.run(&args))
    };
    assert_eq!(
        complete(&["build", "-"])
            .lines()
            .filter(|l| !l.starts_with("--"))
            .collect::<Vec<_>>(),
        ["-h", "-r", "-t"]
    );
    assert_eq!(
        complete(&["build", "--"]),
        "--help\n--jobs\n--release\n--target\n"
    );
    assert_eq!(complete(&["build", "-t", ""]), "arm\nx86\n");
}

#[test]
fn reserved_short_flags_fail_at_load_time() {
    let p = Proj::new(
        "short-reserved",
        &[("ok.ok", "task build(--eval, -e: bool) { }\n")],
    );
    let o = p.run_trusted(&["build"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("the short flag -e is reserved"),
        "{}",
        err(&o)
    );
}

#[test]
fn command_failures_in_tasks_exit_with_the_child_code() {
    let fixture = Path::new(env!("CARGO_BIN_EXE_ok"))
        .with_file_name(format!("ok-fixture{}", std::env::consts::EXE_SUFFIX));
    if !fixture.exists() {
        return; // built by `cargo test --workspace`
    }
    let p = Proj::new(
        "childcode",
        &[(
            "ok.ok",
            &format!(
                "task fail {{\n  run {{path({:?})}} exit 42 boom\n}}\n",
                fixture.display().to_string().replace('\\', "/")
            ),
        )],
    );
    let o = p.run_trusted(&["fail"]);
    assert_eq!(o.status.code(), Some(42), "{}", err(&o));
}

#[test]
fn fmt_and_check() {
    let p = Proj::new("fmt", &[("ok.ok", "task a {\nprint(1)\n}\n")]);
    let o = p.run(&["self", "fmt", "--check"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(p.run(&["self", "fmt"]).status.success());
    assert_eq!(
        std::fs::read_to_string(p.dir.join("ok.ok")).unwrap(),
        "task a {\n  print(1)\n}\n"
    );
    assert!(p.run(&["self", "fmt", "--check"]).status.success());
    assert!(out(&p.run(&["self", "check"])).contains("no problems"));
}
