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
        err(&o)
            .starts_with("error[trust]: this project is not trusted\n  = note: the project is at "),
        "{}",
        err(&o)
    );
    assert!(err(&o).contains("or pass --trust\n"), "{}", err(&o));
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
    // Without a terminal there is no diff, so the error names the files.
    assert!(
        err(&o).starts_with("error[trust]: the project files changed since you trusted them\n"),
        "{}",
        err(&o)
    );
    assert!(
        err(&o).contains("  = note: changed: ok.ok\n"),
        "{}",
        err(&o)
    );
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
                ".ok/db.ok",
                "# Apply migrations\ntask migrate { print(\"migrate\") }\n",
            ),
            (".ok/_lib.ok", "fn helper() { 1 }\n"),
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
    assert_refused_everywhere(&p, "error[trust]: \"../outside.ok\" is outside the project");
}

/// An import that leaves the project is a load-time error (SPEC.md 16.6):
/// running, listing, `ok self check` and `ok trust` all refuse, and nothing
/// is trusted, so a fingerprint never leaves out the outside file.
fn assert_refused_everywhere(p: &Proj, expected: &str) {
    for args in [
        &["a"][..],
        &[],
        &["self", "tasks"],
        &["self", "check"],
        &["trust"],
    ] {
        let o = p.run_trusted(args);
        assert_eq!(o.status.code(), Some(1), "{args:?}: {}", err(&o));
        assert!(err(&o).contains(expected), "{args:?}: {}", err(&o));
        assert_eq!(out(&o), "", "{args:?}");
    }
    assert!(
        !p.store.join("trust.json").exists(),
        "nothing may be trusted"
    );
}

#[test]
fn self_check_summarises_a_project_and_a_failed_load() {
    let p = Proj::new(
        "check-summary",
        &[(
            "ok.ok",
            "task build(--release: bool) { }\ntask ship needs [build(release: true)] { }\n",
        )],
    );
    let o = p.run(&["self", "check"]);
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(
        out(&o),
        "ok: 1 file and 2 tasks checked, no problems found\n"
    );
    p.write(
        "ok.ok",
        "task build(--release: bool) { }\ntask ship needs [build(relase: true)] { }\n",
    );
    let o = p.run(&["self", "check"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("error[arg]: task `build` has no parameter `relase`\n"),
        "{}",
        err(&o)
    );
    assert!(
        err(&o).ends_with("\nok: the project failed to load: 1 error\n"),
        "{}",
        err(&o)
    );
}

#[test]
fn imports_of_imports_cannot_leave_the_project() {
    let p = Proj::new(
        "confined-deep",
        &[
            ("ok.ok", "import \"lib/a.ok\"\ntask a { print(1) }\n"),
            ("lib/a.ok", "import \"../../outside.ok\"\nfn g() { 2 }\n"),
            (".ok/_helper.ok", "import \"../../outside.ok\"\n"),
        ],
    );
    std::fs::write(p.dir.parent().unwrap().join("outside.ok"), "fn f() { 1 }\n").unwrap();
    assert_refused_everywhere(&p, "\"../../outside.ok\" is outside the project");
    let o = p.run_trusted(&["self", "check"]);
    // The snippet shows the importing file, which no task had loaded.
    assert!(
        err(&o).contains(" --> lib/a.ok:1:8\n") || err(&o).contains(" --> .ok/_helper.ok:1:8\n"),
        "{}",
        err(&o)
    );
}

#[cfg(unix)]
#[test]
fn symlinks_that_escape_the_project_are_refused() {
    let p = Proj::new("symlink", &[("ok.ok", "task a { print(1) }\n")]);
    let outside = p.dir.parent().unwrap().join("evil.ok");
    std::fs::write(&outside, "task boom { print(\"boom\") }\n").unwrap();
    std::fs::create_dir_all(p.dir.join(".ok")).unwrap();
    std::os::unix::fs::symlink(&outside, p.dir.join(".ok/evil.ok")).unwrap();
    assert_refused_everywhere(
        &p,
        "error[trust]: .ok/evil.ok is a link to a file outside the project",
    );
    // A symlinked import is refused the same way.
    std::fs::remove_file(p.dir.join(".ok/evil.ok")).unwrap();
    std::os::unix::fs::symlink(&outside, p.dir.join("lib.ok")).unwrap();
    p.write("ok.ok", "import \"lib.ok\"\ntask a { print(1) }\n");
    assert_refused_everywhere(&p, "error[trust]: \"lib.ok\" is outside the project");
    // And so is a root ok.ok that is a link to a file elsewhere.
    std::fs::remove_file(p.dir.join("lib.ok")).unwrap();
    std::fs::remove_file(p.dir.join("ok.ok")).unwrap();
    std::os::unix::fs::symlink(&outside, p.dir.join("ok.ok")).unwrap();
    assert_refused_everywhere(
        &p,
        "error[trust]: ok.ok is a link to a file outside the project",
    );
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
    // File systems keep coarse timestamps (a few ms on Linux), so a source
    // written just before the first run could share the output's mtime, and
    // "newer than the newest source" would then be false. Age the source.
    let a = std::fs::File::options()
        .write(true)
        .open(p.dir.join("in/a.txt"))
        .unwrap();
    a.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(10))
        .unwrap();
    drop(a);
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
    let o = p.run_trusted(&["--env", "nope", "show"]);
    assert_eq!(
        o.status.code(),
        Some(2),
        "a missing --env file is bad usage"
    );
    assert!(err(&o).contains(".env.nope"), "{}", err(&o));
    assert!(err(&o).contains("leave out --env nope"), "{}", err(&o));
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
        &[("ok.ok", BASIC), (".ok/db.ok", "task migrate { }\n")],
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
        &[("ok.ok", BASIC), (".ok/db.ok", "task migrate { }\n")],
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
                ".ok/db.ok",
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

#[test]
fn check_finds_problems_in_every_project_file() {
    let p = Proj::new(
        "checkall",
        &[
            (
                "ok.ok",
                "import \".ok/_lib.ok\"\n\ntask build(--release: bool) {\n  print(relase, helper(1))\n}\n",
            ),
            (".ok/_lib.ok", "fn helper() { 1 }\n"),
            (
                ".ok/db.ok",
                "task seed sources [glob(\"*.sql\", root: \"db\", recursive: true)] {\n  print(\"seeding\")\n}\n",
            ),
        ],
    );
    let o = p.run(&["self", "check"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    let e = err(&o);
    assert!(
        e.contains("unknown name `relase`") && e.contains("did you mean `release`?"),
        "{e}"
    );
    assert!(
        e.contains("`helper` takes 0 positional arguments, got 1"),
        "{e}"
    );
    assert!(e.contains("is declared at .ok/_lib.ok:1:4"), "{e}");
    assert!(e.contains(".ok/db.ok:1:"), "{e}");
    assert!(e.contains("`glob` has no parameter `recursive`"), "{e}");
    assert!(e.contains("3 files and 2 tasks checked: 3 errors"), "{e}");
    // An unknown dependency is a load error, reported before anything else.
    p.write("ok.ok", "task a needs [nope] {\n}\n");
    let o = p.run(&["self", "check"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("unknown task `nope` in needs"),
        "{}",
        err(&o)
    );
}

#[test]
fn check_rejects_unknown_options() {
    let p = Proj::new("checkopts", &[("ok.ok", "task a {\n}\n")]);
    let o = p.run(&["self", "check", "--bogus"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("unknown option --bogus"), "{}", err(&o));
    let o = p.run(&["self", "fmt", "--strict"]);
    assert_eq!(o.status.code(), Some(2));
}

/// Tasks `a` and `b` each wait until the other one has started, so the
/// invocation only finishes when they really run at the same time.
const RENDEZVOUS: &str = r#"
fn meet(me: string, other: string) {
  write("", path("{me}.started"))
  var waited = 0
  while not exists(path("{other}.started")) {
    if waited > 2000 {
      fail("{other} never started")
    }
    sleep(10ms)
    waited += 1
  }
}

task a {
  meet("a", "b")
  print("a ran")
}

task b {
  meet("b", "a")
  eprint("b ran")
}

task both needs [a, b] {
  print("both: body")
}

task broken {
  fail("it broke")
}

task slow {
  sleep(300ms)
  print("slow finished")
}

task fails needs [broken, slow] {
  print("never")
}

task ask {
  print(confirm("really?"))
}

task asks needs [ask, slow] {
}
"#;

#[test]
fn independent_dependencies_run_in_parallel_with_prefixed_output() {
    let p = Proj::new("parallel", &[("ok.ok", RENDEZVOUS)]);
    let o = p.run_trusted(&["--jobs", "2", "both"]);
    assert!(o.status.success(), "{}", err(&o));
    let stdout = out(&o);
    assert!(stdout.contains("a | a ran\n"), "{stdout}");
    assert!(err(&o).contains("b | b ran\n"), "{}", err(&o));
    assert!(
        stdout.ends_with("both: body\n"),
        "the body runs after all dependencies: {stdout}"
    );
}

#[test]
fn workers_need_the_same_trust_as_a_normal_run() {
    let p = Proj::new("parallel-trust", &[("ok.ok", RENDEZVOUS)]);
    // A worker started by hand on an untrusted project refuses to run.
    let o = p.run(&["--task-worker", "0:a:both:0"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("this project is not trusted"),
        "{}",
        err(&o)
    );
    assert!(p.run(&["trust"]).status.success());
    // Trusted in the store: the workers find the approval there.
    let o = p.run(&["--jobs", "2", "both"]);
    assert!(o.status.success(), "{}", err(&o));
    // The right trust but a stale fingerprint is refused too.
    let o = p.run(&["--task-worker", "0:a:both:0"]);
    assert!(err(&o).contains("the project files changed"), "{}", err(&o));
}

#[test]
fn a_failing_dependency_is_traced_to_its_needs_entry_in_every_mode() {
    let p = Proj::new(
        "trace-needs",
        &[(
            "ok.ok",
            "task gen {\n  fail(\"boom\")\n}\ntask other { }\ntask build needs [gen, other] { }\ntask top {\n  fail(\"top\")\n}\n",
        )],
    );
    // The needs entry `gen` is at 5:19, whether the dependency runs in ok's
    // process (-j1) or in a parallel worker.
    for jobs in ["-j1", "-j4"] {
        let o = p.run_trusted(&[jobs, "build"]);
        assert_eq!(o.status.code(), Some(1));
        assert!(
            err(&o).contains("= in `gen`, called at ok.ok:5:19\n"),
            "{jobs}: {}",
            err(&o)
        );
    }
    // The task named on the command line has no call site in the source.
    let o = p.run_trusted(&["top"]);
    assert!(
        err(&o).contains("= in `top`, started by ok\n"),
        "{}",
        err(&o)
    );
}

#[test]
fn jobs_1_runs_dependencies_one_at_a_time_without_prefixes() {
    let p = Proj::new(
        "sequential",
        &[(
            "ok.ok",
            "task a { print(\"a\") }\ntask b { print(\"b\") }\ntask c needs [a, b] { print(\"c\") }\n",
        )],
    );
    for args in [&["-j1", "c"][..], &["--jobs", "1", "c"], &["--jobs=1", "c"]] {
        let o = p.run_trusted(args);
        assert_eq!(out(&o), "a\nb\nc\n", "{}", err(&o));
    }
    // --dry-run is always sequential, so the plan prints in a stable order.
    let o = p.run_trusted(&["--dry-run", "c"]);
    assert_eq!(out(&o), "a\nb\nc\n", "{}", err(&o));
    let o = p.run_trusted(&["--jobs", "0", "c"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("--jobs needs a number of at least 1"));
}

#[test]
fn a_failed_parallel_dependency_waits_for_the_others_and_stops_the_run() {
    let p = Proj::new("parallel-fail", &[("ok.ok", RENDEZVOUS)]);
    let o = p.run_trusted(&["--jobs", "4", "fails"]);
    assert_eq!(o.status.code(), Some(1));
    let (stdout, stderr) = (out(&o), err(&o));
    assert!(stdout.contains("slow   | slow finished"), "{stdout}");
    assert!(!stdout.contains("never"), "{stdout}");
    assert!(
        stderr.contains("broken | error[fail]: it broke"),
        "{stderr}"
    );
    assert!(
        stderr.ends_with(
            "error: task `broken` failed with exit code 1\n  = its output is above, each line starting with `broken |`\n"
        ),
        "{stderr}"
    );
}

#[test]
fn parallel_tasks_cannot_ask_questions() {
    let p = Proj::new("parallel-ask", &[("ok.ok", RENDEZVOUS)]);
    let o = p.run_trusted(&["--jobs", "2", "asks"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("this task runs in parallel with other tasks and has no terminal"),
        "{}",
        err(&o)
    );
    let o = p.run_trusted(&["--yes", "--jobs", "2", "asks"]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(out(&o).contains("ask  | true"), "{}", out(&o));
}

const WITH_ARGS: &str = r#"
let wasm = "wasm32"

task compile(--release: bool, --target: string = "native") {
  print("compile release={release} target={target}")
}

task web needs [compile(target: wasm, release: true)] {
  print("web")
}

task dist needs [compile(release: true, target: wasm), web, compile] {
  print("dist")
}
"#;

#[test]
fn dependencies_take_arguments_and_run_once_per_argument_set() {
    let p = Proj::new("needs-args", &[("ok.ok", WITH_ARGS)]);
    let o = p.run_trusted(&["-j1", "dist"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        out(&o),
        "compile release=true target=wasm32\nweb\ncompile release=false target=native\ndist\n"
    );
    let o = p.run_trusted(&["--jobs", "2", "dist"]);
    assert!(o.status.success(), "{}", err(&o));
    let stdout = out(&o);
    assert_eq!(
        stdout.matches("compile release=true").count(),
        1,
        "{stdout}"
    );
    // Labels are padded to the longest one among the parallel tasks.
    let longest = "compile(release: true, target: \"wasm32\")";
    assert!(
        stdout.contains(&format!(
            "{:<w$} | compile release=false target=native\n",
            "compile",
            w = longest.len()
        )),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("{longest} | compile release=true")),
        "{stdout}"
    );
    assert!(stdout.ends_with("dist\n"), "{stdout}");
    let help = out(&p.run(&["dist", "--help"]));
    assert!(
        help.contains("Runs first: compile(release: true, target: wasm), web, compile"),
        "{help}"
    );
}

#[test]
fn wrong_dependency_arguments_are_errors_with_spans() {
    let cases: &[(&str, &str)] = &[
        (
            "task c(--release: bool) { }\ntask d needs [c(relase: true)] { }\n",
            "error[arg]: task `c` has no parameter `relase`\n --> ok.ok:2:17\n  |\n2 | task d needs [c(relase: true)] { }\n  |                 ^^^^^^\n  |\n  = help: did you mean `release`?\n",
        ),
        (
            "task c(name: string) { }\ntask d needs [c] { }\n",
            "error[arg]: task `c` needs the argument `name`\n --> ok.ok:2:15\n  |\n2 | task d needs [c] { }\n  |               ^\n  |\n  = help: give it here: c(name: ...)\n",
        ),
        (
            "task c(--n: int = 1) { }\ntask d needs [c(n: read(\"x\"))] { }\n",
            "error[syntax]: an argument of a dependency must be a constant\n --> ok.ok:2:20\n  |\n2 | task d needs [c(n: read(\"x\"))] { }\n  |                    ^^^^^^^^^\n  |\n  = help: use literals, constant lets, path(...) and os.name/arch/exe; compute other values inside the task\n",
        ),
        (
            "task c(--n: int = 1) { }\ntask d needs [c(\"x\")] { }\n",
            "error[syntax]: arguments of a dependency must be named\n --> ok.ok:2:17\n  |\n2 | task d needs [c(\"x\")] { }\n  |                 ^^^\n  |\n  = help: name the parameter: c(name: value)\n",
        ),
        (
            "task c(--n: int = 1) { }\ntask d needs [c(n: \"many\")] { }\n",
            "error[type]: argument `n` of task `c` must be an int, got a string\n --> ok.ok:2:20\n  |\n2 | task d needs [c(n: \"many\")] { }\n  |                    ^^^^^^\n",
        ),
    ];
    for (i, (src, want)) in cases.iter().enumerate() {
        let p = Proj::new(&format!("needs-arg-err-{i}"), &[("ok.ok", src)]);
        let o = p.run_trusted(&["d"]);
        assert_eq!(o.status.code(), Some(1), "{src}");
        assert_eq!(err(&o), *want, "{src}");
    }
}

#[test]
fn positional_parameters_are_named_in_needs() {
    let p = Proj::new(
        "needs-positional",
        &[(
            "ok.ok",
            "task greet(greeting: string = \"hello\", name: string) {\n  print(\"{greeting} {name}\")\n}\ntask d needs [greet(name: \"bas\")] { }\n",
        )],
    );
    let o = p.run_trusted(&["d"]);
    assert_eq!(out(&o), "hello bas\n", "{}", err(&o));
}

#[test]
fn listings_and_help_show_hidden_characters_as_escapes() {
    // Listing and `--help` need no trust, so text from a fresh clone must
    // not reach the terminal raw.
    let p = Proj::new(
        "invisible",
        &[(
            "ok.ok",
            "# Build \u{200b}it\u{2060}\ntask build(--fast: bool) {\n  print(\"x\")\n}\n",
        )],
    );
    let listing = out(&p.run(&[]));
    assert!(listing.contains("Build \\u{200b}it\\u{2060}"), "{listing}");
    let help = out(&p.run(&["build", "--help"]));
    assert!(help.starts_with("Build \\u{200b}it\\u{2060}"), "{help}");
}

/// Every file under `dir`, read as text, joined: to check what the trust
/// store keeps.
fn read_tree(dir: &Path) -> String {
    let mut all = String::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            all.push_str(&read_tree(&p));
        } else {
            all.push_str(&String::from_utf8_lossy(&std::fs::read(&p).unwrap()));
        }
    }
    all
}

#[test]
fn env_files_are_trusted_with_the_project_but_their_values_are_never_shown() {
    let p = Proj::new(
        "envtrust",
        &[
            (
                "ok.ok",
                "env file \".env\"\ntask show { print(env.get(\"TOKEN\")) }\n",
            ),
            (".env", "TOKEN=first-secret\n"),
            (".env.prod", "TOKEN=prod-secret\n"),
        ],
    );
    let t = p.run(&["trust"]);
    assert!(t.status.success(), "{}", err(&t));
    assert!(
        out(&t).contains("files:  ok.ok, .env (names only)"),
        "{}",
        out(&t)
    );
    assert_eq!(out(&p.run(&["show"])), "first-secret\n");

    // A changed value needs approval again, and the error names the file.
    p.write(".env", "TOKEN=second-secret\n");
    let o = p.run(&["show"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("= note: changed: .env\n"), "{}", err(&o));
    assert!(p.run(&["trust"]).status.success());
    assert_eq!(out(&p.run(&["show"])), "second-secret\n");

    // `--env NAME` loads one more file, so that run is trusted separately.
    let o = p.run(&["--env", "prod", "show"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(err(&o).contains("changed: .env.prod"), "{}", err(&o));
    assert!(p.run(&["--env", "prod", "trust"]).status.success());
    assert_eq!(out(&p.run(&["--env", "prod", "show"])), "prod-secret\n");

    // The store keeps the names of env variables, never their values.
    let store = read_tree(&p.store);
    assert!(store.contains("TOKEN"), "{store}");
    assert!(!store.contains("secret"), "{store}");
}

#[test]
fn content_trust_belongs_to_the_directory_it_was_given_for() {
    let p = Proj::new("bound", &[("ok.ok", BASIC)]);
    assert!(p.run(&["trust"]).status.success());
    assert_eq!(out(&p.run(&["gen"])), "gen\n");
    // The same file in another directory (next to other env files, or a
    // different checkout) is not trusted by that approval.
    let other = p.dir.parent().unwrap().join("copy");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("ok.ok"), BASIC).unwrap();
    let o = p.cmd(&["gen"]).current_dir(&other).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(
        err(&o).starts_with("error[trust]: this project is not trusted"),
        "{}",
        err(&o)
    );
}

#[test]
fn env_files_cannot_set_ok_or_loader_variables() {
    for key in [
        "OK_TRUST",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
        "ok_config_dir",
    ] {
        let p = Proj::new(
            "envkeys",
            &[
                ("ok.ok", "env file \".env\"\ntask show { print(\"ran\") }\n"),
                (".env", &format!("A=1\n{key}=x\n")),
            ],
        );
        let o = p.run_trusted(&["show"]);
        assert_eq!(o.status.code(), Some(1), "{key}: {}", err(&o));
        assert_eq!(out(&o), "", "{key}: nothing runs");
        assert!(
            err(&o).starts_with(&format!("error: .env sets {key}, but env files cannot set")),
            "{}",
            err(&o)
        );
    }
}

#[test]
fn an_unreadable_trust_store_is_an_error_and_is_not_overwritten() {
    let p = Proj::new("badstore", &[("ok.ok", BASIC)]);
    // A directory where trust.json should be cannot be read as a file.
    std::fs::create_dir_all(p.store.join("trust.json")).unwrap();
    let o = p.run(&["trust"]);
    assert_eq!(o.status.code(), Some(1), "{}", err(&o));
    assert!(
        err(&o).contains("cannot read the trust store"),
        "{}",
        err(&o)
    );
    assert!(p.store.join("trust.json").is_dir(), "left as it was");
    std::fs::remove_dir(p.store.join("trust.json")).unwrap();
    std::fs::write(p.store.join("trust.json"), b"{\"entries\": [\xff]}").unwrap();
    let o = p.run(&["gen"]);
    assert!(
        err(&o).contains("is damaged: it is not UTF-8"),
        "{}",
        err(&o)
    );
    assert_eq!(
        std::fs::read(p.store.join("trust.json")).unwrap(),
        b"{\"entries\": [\xff]}"
    );
}

#[cfg(unix)]
#[test]
fn the_trust_store_is_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;
    let p = Proj::new("private", &[("ok.ok", BASIC)]);
    assert!(p.run(&["trust"]).status.success());
    let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&p.store), 0o700);
    assert_eq!(mode(&p.store.join("trust.json")), 0o600);
    let snaps = p.store.join("snapshots");
    assert_eq!(mode(&snaps), 0o700);
    let snap = std::fs::read_dir(&snaps)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(mode(&snap), 0o700);
    assert_eq!(mode(&snap.join("ok.ok")), 0o600);
}

#[test]
fn trusting_projects_at_the_same_time_keeps_every_entry() {
    let projects: Vec<Proj> = (0..6)
        .map(|i| Proj::new(&format!("concurrent{i}"), &[("ok.ok", BASIC)]))
        .collect();
    // One store for all of them.
    let store = projects[0].store.clone();
    let children: Vec<std::process::Child> = projects
        .iter()
        .map(|p| {
            p.cmd(&["trust"])
                .env("OK_CONFIG_DIR", &store)
                .stdout(Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut c in children {
        assert!(c.wait().unwrap().success());
    }
    let list = out(&projects[0].run(&["trust", "--list"]));
    for i in 0..6 {
        let name = format!("ok-proj-concurrent{i}-");
        assert!(list.contains(&name), "{name} missing from\n{list}");
    }
}

/// The fingerprint of SPEC.md 16.2 for `files` (path, content), computed
/// independently of ok, so a test pins which paths trust covers.
fn fingerprint(files: &[(&str, &str)]) -> String {
    use sha2::Digest;
    let mut sorted = files.to_vec();
    sorted.sort();
    let mut h = sha2::Sha256::new();
    for (rel, content) in sorted {
        h.update(rel.as_bytes());
        h.update([0]);
        h.update(content.len().to_string().as_bytes());
        h.update([0]);
        h.update(content.as_bytes());
    }
    hex::encode(h.finalize())
}

const HELLO: &str = "task hello { print(\"hello\") }\n";
const MIGRATE: &str = "task migrate { print(\"migrate\") }\n";

#[test]
fn every_root_file_form_is_a_project_and_is_fingerprinted() {
    let forms: [&[(&str, &str)]; 4] = [
        &[("ok.ok", HELLO)],
        &[(".ok", HELLO)],
        &[(".ok/ok.ok", HELLO), (".ok/db.ok", MIGRATE)],
        &[("ok.ok", HELLO), (".ok/db.ok", MIGRATE)],
    ];
    for (i, files) in forms.into_iter().enumerate() {
        let p = Proj::new(&format!("form{i}"), files);
        std::fs::create_dir_all(p.dir.join("sub")).unwrap();
        // Discovery finds the root from a subdirectory too.
        let o = p
            .cmd(&["-C", "sub", "hello"])
            .env("OK_TRUST", "1")
            .output()
            .unwrap();
        assert_eq!(out(&o), "hello\n", "{files:?}: {}", err(&o));
        assert_eq!(err(&o), "", "{files:?}");
        if files.len() > 1 {
            assert_eq!(out(&p.run_trusted(&["db", "migrate"])), "migrate\n");
        }
        // Trust covers exactly these files, under their paths from the
        // root: a project that keeps `ok.ok` keeps its fingerprint.
        let t = p.run(&["trust"]);
        let mut names: Vec<&str> = files.iter().map(|(n, _)| *n).collect();
        names.sort();
        assert!(
            out(&t).contains(&format!("  files:  {}\n", names.join(", "))),
            "{}",
            out(&t)
        );
        assert!(
            out(&t).contains(&format!("sha256: {}\n", fingerprint(files))),
            "{}",
            out(&t)
        );
        assert_eq!(out(&p.run(&["hello"])), "hello\n");
        // A change to any of them needs trust again.
        let (last, body) = files[files.len() - 1];
        p.write(last, &format!("{body}\n# changed\n"));
        let o = p.run(&["hello"]);
        assert_eq!(o.status.code(), Some(1), "{files:?}");
        assert!(err(&o).contains(&format!("changed: {last}")), "{}", err(&o));
    }
}

#[test]
fn helpers_in_the_dot_ok_directory_are_fingerprinted() {
    let p = Proj::new(
        "dot-helpers",
        &[
            (
                ".ok/ok.ok",
                "import \"_lib.ok\"\ntask hello { print(greeting()) }\n",
            ),
            (".ok/_lib.ok", "fn greeting() { \"hi\" }\n"),
            (".ok/_unused.ok", "fn unused() { 1 }\n"),
            (".ok/tools/gen.ok", "fn gen() { 2 }\n"),
        ],
    );
    let t = out(&p.run(&["trust"]));
    assert!(
        t.contains("files:  .ok/_lib.ok, .ok/_unused.ok, .ok/ok.ok\n"),
        "{t}"
    );
    assert_eq!(out(&p.run(&["hello"])), "hi\n");
    p.write(".ok/_lib.ok", "fn greeting() { \"changed\" }\n");
    assert_eq!(p.run(&["hello"]).status.code(), Some(1));
    // A file in a subdirectory of .ok/ is no namespace, and joins the
    // project files once something imports it.
    let l = out(&p.run(&[]));
    assert!(!l.contains("tools") && !l.contains("gen"), "{l}");
    p.write(
        ".ok/ok.ok",
        "import \"_lib.ok\"\nimport \"tools/gen.ok\"\ntask hello { print(greeting(), gen()) }\n",
    );
    let t = out(&p.run(&["trust"]));
    assert!(t.contains(".ok/ok.ok, .ok/tools/gen.ok\n"), "{t}");
}

#[test]
fn two_root_files_are_an_error() {
    for (i, other) in [".ok", ".ok/ok.ok"].into_iter().enumerate() {
        let p = Proj::new(
            &format!("two-roots{i}"),
            &[("ok.ok", HELLO), (other, HELLO)],
        );
        let root = p.dir.canonicalize().unwrap();
        let root = okrun::path::OkPath::from_native(&root).display();
        let expected = format!(
            "error: this project has two root files, ok.ok and {other}\n  = note: the project is at {root}\n  = help: a project has one root file: move the tasks of ok.ok into {other}, or the other way round, and delete the one left empty\n"
        );
        for args in [&["hello"][..], &[], &["self", "check"], &["trust"]] {
            let o = p.run_trusted(args);
            assert_eq!(o.status.code(), Some(1), "{args:?}");
            let e = err(&o);
            let e = e.trim_end_matches("ok: the project failed to load: 1 error\n");
            assert_eq!(e, expected, "{args:?}");
            assert_eq!(out(&o), "", "{args:?}");
        }
        assert!(!p.store.join("trust.json").exists());
    }
}

#[test]
fn a_dot_ok_directory_without_a_root_file_is_no_project() {
    let p = Proj::new("dot-dir-only", &[(".ok/db.ok", MIGRATE)]);
    let o = p.run(&["migrate"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("(no ok.ok, .ok or .ok/ok.ok), so `migrate` is not a task"),
        "{}",
        err(&o)
    );
    // With a root above it, that project is found instead.
    let p = Proj::new(
        "dot-dir-nested",
        &[("ok.ok", HELLO), ("sub/.ok/db.ok", MIGRATE)],
    );
    let o = p.cmd(&["-C", "sub"]).output().unwrap();
    assert!(out(&o).contains("hello"), "{}", out(&o));
    assert!(!out(&o).contains("migrate"), "{}", out(&o));
}

#[test]
fn the_old_ok_directory_still_works_with_a_warning() {
    let p = Proj::new(
        "old-dir",
        &[
            (
                "ok.ok",
                "task all needs [db.migrate, db.seed] { print(\"all\") }\n",
            ),
            (
                "ok/db.ok",
                "task migrate { print(\"migrate\") }\ntask seed { print(\"seed\") }\n",
            ),
            ("ok/_lib.ok", "fn f() { 1 }\n"),
        ],
    );
    let warning = "warning: ok/ is deprecated: namespaces now live in .ok/\n  = help: run `ok self promote` to move them; that also frees the name ok for the ./ok wrapper\n";
    // Task workers (parallel dependencies) leave the warning to their parent.
    let o = p.run_trusted(&["-j", "4", "all"]);
    let stdout = out(&o);
    let mut lines: Vec<&str> = stdout.lines().collect();
    lines.sort();
    assert_eq!(
        lines,
        ["all", "db.migrate | migrate", "db.seed    | seed"],
        "{}",
        err(&o)
    );
    assert_eq!(err(&o), warning, "once per run");
    let o = p.run(&[]);
    assert!(out(&o).contains("db\n  migrate"), "{}", out(&o));
    assert_eq!(err(&o), warning);
    // Completions keep the prompt clean.
    let o = p.run(&["self", "complete", "--", "d"]);
    assert_eq!((out(&o).as_str(), err(&o).as_str()), ("db\n", ""));
    let t = out(&p.run(&["trust"]));
    assert!(t.contains("files:  ok.ok, ok/_lib.ok, ok/db.ok\n"), "{t}");
    // Namespace files in both directories are an error.
    p.write("ok.ok", "task all { }\n");
    std::fs::create_dir_all(p.dir.join(".ok")).unwrap();
    p.write(".ok/admin.ok", "task user { }\n");
    let o = p.run_trusted(&[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains(
            "error: this project has namespace files in both .ok/ and the deprecated ok/\n"
        ),
        "{}",
        err(&o)
    );
    assert!(
        err(&o).contains("= help: move the files of ok/ into .ok/ (ok self promote does it when no names clash)\n"),
        "{}",
        err(&o)
    );
    // An ok/ directory without ok files is not ok's.
    let p = Proj::new(
        "old-dir-unrelated",
        &[(".ok/ok.ok", HELLO), ("ok/readme.txt", "go code\n")],
    );
    let o = p.run_trusted(&["hello"]);
    assert_eq!((out(&o).as_str(), err(&o).as_str()), ("hello\n", ""));
}

#[test]
fn mounts_accept_every_root_file_form() {
    let fe = "task build { print(\"fe build\") }\n";
    let cases: [(&str, &str, &str); 3] = [
        ("ok.ok", "import \"web/.ok/ok.ok\" as fe\n", "web/.ok/ok.ok"),
        ("ok.ok", "import \"web/.ok\" as fe\n", "web/.ok"),
        (".ok/ok.ok", "import \"../web/ok.ok\" as fe\n", "web/ok.ok"),
    ];
    for (i, (root, import, sub)) in cases.into_iter().enumerate() {
        let p = Proj::new(&format!("mount-form{i}"), &[(root, import), (sub, fe)]);
        let o = p.run_trusted(&["fe", "build"]);
        assert_eq!(out(&o), "fe build\n", "{sub}: {}", err(&o));
        let t = out(&p.run(&["trust"]));
        assert!(t.contains(sub), "{t}");
    }
    // Only a project's root file can be mounted.
    let p = Proj::new(
        "mount-not-root",
        &[
            ("ok.ok", "import \"web/ok.ok\" as fe\n"),
            ("web/ok.ok", fe),
            ("web/.ok/ok.ok", fe),
        ],
    );
    let o = p.run_trusted(&[]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains(
            "cannot mount \"web/ok.ok\": this project has two root files, ok.ok and .ok/ok.ok"
        ),
        "{}",
        err(&o)
    );
}

#[cfg(unix)]
#[test]
fn a_dot_ok_directory_linked_out_of_the_project_is_refused() {
    let p = Proj::new("dot-link", &[]);
    let outside = p.dir.parent().unwrap().join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("ok.ok"), "task a { print(\"boom\") }\n").unwrap();
    std::os::unix::fs::symlink(&outside, p.dir.join(".ok")).unwrap();
    assert_refused_everywhere(
        &p,
        "error[trust]: .ok/ok.ok is a link to a file outside the project",
    );
    // A namespace file linked out of a real .ok/ is refused too.
    std::fs::remove_file(p.dir.join(".ok")).unwrap();
    std::fs::create_dir_all(p.dir.join(".ok")).unwrap();
    p.write(".ok/ok.ok", "task a { print(1) }\n");
    std::os::unix::fs::symlink(outside.join("ok.ok"), p.dir.join(".ok/evil.ok")).unwrap();
    assert_refused_everywhere(
        &p,
        "error[trust]: .ok/evil.ok is a link to a file outside the project",
    );
}

#[test]
fn check_and_fmt_take_a_dot_ok_file() {
    let p = Proj::new(
        "check-dot",
        &[(".ok", "requires \"0.1\"\n\ntask hello {\n  print( 1 )\n}\n")],
    );
    let o = p.run(&["self", "check", ".ok"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "ok: 1 file checked, no problems found\n");
    let o = p.run(&["self", "fmt", "--check", ".ok"]);
    assert_eq!(out(&o), "would reformat .ok\n");
    let o = p.run(&["self", "fmt"]);
    assert!(
        out(&o).starts_with("formatted ") && out(&o).ends_with("/app/.ok\n"),
        "{}",
        out(&o)
    );
    // Inside .ok/, a namespace given by name is checked as a project file.
    let p = Proj::new(
        "check-dot-dir",
        &[(".ok/ok.ok", HELLO), (".ok/db.ok", MIGRATE)],
    );
    let o = p
        .cmd(&["self", "check", "db.ok"])
        .current_dir(p.dir.join(".ok"))
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    let o = p.run(&["self", "check"]);
    assert_eq!(
        out(&o),
        "ok: 2 files and 2 tasks checked, no problems found\n"
    );
}

#[test]
fn init_writes_dot_ok_slash_ok_ok_without_network() {
    let p = Proj::new("init", &[]);
    // An http mirror would be refused, so this shows nothing is downloaded.
    let o = p
        .cmd(&["self", "init"])
        .env("OK_DOWNLOAD_URL", "http://mirror.example/ok")
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(out(&o), "wrote .ok/ok.ok\nrun ok hello\n");
    let names: Vec<String> = std::fs::read_dir(&p.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, [".ok"]);
    assert_eq!(out(&p.run_trusted(&["hello"])), "hello, world!\n");
    // Refuses whenever a root file exists.
    let o = p.run(&["self", "init"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with(
            "error: .ok/ok.ok already exists; ok self init does not overwrite files\n"
        ),
        "{}",
        err(&o)
    );
    for root in ["ok.ok", ".ok"] {
        let p = Proj::new(&format!("init-{}", root.len()), &[(root, HELLO)]);
        let o = p.run(&["self", "init"]);
        assert!(
            err(&o).starts_with(&format!("error: {root} already exists;")),
            "{}",
            err(&o)
        );
        assert_eq!(std::fs::read_to_string(p.dir.join(root)).unwrap(), HELLO);
    }
    // --checksums is only for the wrappers.
    let p = Proj::new("init-sums", &[]);
    let o = p.run(&["self", "init", "--checksums", "SHA256SUMS"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(
        err(&o).contains("--checksums is only used for the wrappers"),
        "{}",
        err(&o)
    );
    // The ./ok wrapper cannot share its name with a directory.
    let p = Proj::new("init-ok-dir", &[("ok/readme.txt", "")]);
    let o = p.run(&["self", "init", "--wrappers"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("error: ok/ is a directory, so the ./ok wrapper cannot be written\n"),
        "{}",
        err(&o)
    );
    assert!(!p.dir.join(".ok").exists(), "nothing is written");
    // Without --wrappers, that directory is no obstacle.
    assert!(p.run(&["self", "init"]).status.success());
}

#[test]
fn promote_moves_a_dot_ok_file_into_the_folder() {
    let body = "import \"lib/x.ok\"\n\ntask hello { print(x()) }\n";
    let p = Proj::new(
        "promote",
        &[(".ok", body), ("lib/x.ok", "fn x() { \"x\" }\n")],
    );
    assert!(p.run(&["trust"]).status.success());
    let o = p.run(&["self", "promote"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        out(&o),
        "moved .ok to .ok/ok.ok\nimports are relative to their file; these no longer point at the same file:\n  .ok/ok.ok:1: \"lib/x.ok\" should now be \"../lib/x.ok\"\nthe project files moved, so the next task run asks for trust again (or run ok trust)\n"
    );
    assert_eq!(
        std::fs::read_to_string(p.dir.join(".ok/ok.ok")).unwrap(),
        body
    );
    let leftovers: Vec<String> = std::fs::read_dir(&p.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".ok."))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    p.write(".ok/ok.ok", &body.replace("lib/x.ok", "../lib/x.ok"));
    // Trust does not move with the files.
    let o = p.run(&["hello"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("the project files changed since you trusted them"),
        "{}",
        err(&o)
    );
    assert_eq!(out(&p.run_trusted(&["hello"])), "x\n");
    // A second run has nothing to do.
    let o = p.run(&["self", "promote"]);
    assert!(o.status.success());
    assert_eq!(
        out(&o),
        "nothing to promote: the project already uses .ok/ok.ok\n"
    );
}

#[test]
fn promote_moves_the_old_ok_directory() {
    let p = Proj::new(
        "promote-old",
        &[
            ("ok.ok", "import \"ok/_lib.ok\"\ntask a { print(f()) }\n"),
            ("ok/_lib.ok", "fn f() { 1 }\n"),
            ("ok/db.ok", "import \"./_lib.ok\"\ntask m { print(f()) }\n"),
            ("ok/data/seed.sql", "select 1;\n"),
        ],
    );
    let o = p.run(&["self", "promote"]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        out(&o),
        "moved ok/_lib.ok to .ok/_lib.ok\nmoved ok/data to .ok/data\nmoved ok/db.ok to .ok/db.ok\nimports are relative to their file; these no longer point at the same file:\n  ok.ok:1: \"ok/_lib.ok\" should now be \".ok/_lib.ok\"\nthe project files moved, so the next task run asks for trust again (or run ok trust)\n"
    );
    assert!(!p.dir.join("ok").exists());
    assert!(p.dir.join(".ok/data/seed.sql").is_file());
    p.write("ok.ok", "import \".ok/_lib.ok\"\ntask a { print(f()) }\n");
    let o = p.run_trusted(&["db", "m"]);
    assert_eq!((out(&o).as_str(), err(&o).as_str()), ("1\n", ""));
    // The root ok.ok stays, so there is nothing left to do.
    let o = p.run(&["self", "promote"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("error: nothing to promote: the root file is ok.ok, not a .ok file, and there is no ok/ directory\n"),
        "{}",
        err(&o)
    );
}

/// A linked `.ok` with a relative target would dangle one directory
/// down, and the project would be gone, so promote leaves it alone.
#[cfg(unix)]
#[test]
fn promote_refuses_a_linked_dot_ok_file() {
    let p = Proj::new("promote-link", &[("real/tasks.ok", HELLO)]);
    std::os::unix::fs::symlink("real/tasks.ok", p.dir.join(".ok")).unwrap();
    let o = p.run(&["self", "promote"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with(
            "error: .ok is a symbolic link; ok self promote only moves regular files\n"
        ),
        "{}",
        err(&o)
    );
    assert!(p.dir.join(".ok").is_file(), "the link still works");
}

#[test]
fn promote_refuses_before_moving_anything() {
    // A name that exists in both directories.
    let p = Proj::new(
        "promote-clash",
        &[
            (".ok/ok.ok", HELLO),
            (".ok/db.ok", MIGRATE),
            ("ok/admin.ok", HELLO),
            ("ok/db.ok", MIGRATE),
        ],
    );
    let o = p.run(&["self", "promote"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("error: cannot move ok/db.ok to .ok/db.ok: .ok/db.ok already exists; nothing was moved\n"),
        "{}",
        err(&o)
    );
    assert!(p.dir.join("ok/admin.ok").is_file(), "nothing moved");
    // ok/ok.ok would become a second root file.
    let p = Proj::new(
        "promote-root-clash",
        &[(".ok", HELLO), ("ok/ok.ok", MIGRATE)],
    );
    let o = p.run(&["self", "promote"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).starts_with("error: ok/ok.ok would become .ok/ok.ok, a second root file\n"),
        "{}",
        err(&o)
    );
    assert_eq!(std::fs::read_to_string(p.dir.join(".ok")).unwrap(), HELLO);
    // A project with ok.ok and nothing else to move.
    let p = Proj::new("promote-okok", &[("ok.ok", HELLO)]);
    let o = p.run(&["self", "promote"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        err(&o).contains("the root file is ok.ok, not a .ok file"),
        "{}",
        err(&o)
    );
    // No project at all.
    let p = Proj::new("promote-none", &[]);
    let o = p.run(&["self", "promote"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("nothing to promote"), "{}", err(&o));
}
