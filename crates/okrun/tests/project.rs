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
