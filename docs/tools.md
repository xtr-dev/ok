# Formatter, checker and wrappers

The tools built into the ok binary: `ok self fmt` formats code, `ok self check` finds mistakes without running it, `ok self init` adds the `./ok` wrappers.

ok ships its own tooling, so a project needs nothing besides the one binary to keep its scripts tidy and correct. All of it lives under `ok self`, a name no task can take. This page covers the formatter, the checker, the bootstrap wrappers that let people run your tasks without installing ok, and how to use them in an editor and in CI. The reference is [section 17](../SPEC.md#17-built-in-commands-ok-self-) and [section 18](../SPEC.md#18-bootstrap-wrappers) of the spec.

## Formatting with `ok self fmt`

`ok self fmt` rewrites ok files in one canonical layout, the style used throughout this guide and the repository's examples. There are no options to argue about. Here is a file written in a hurry:

```ok file=tidy.ok nocheck
let files = glob("**/*.log", root: ".", hidden: true, exclude: ["**/.git", "**/node_modules", "**/target"])
let r = {name:"app.log" , size:3}
if r.size>2 {print( "big" )}
```

`--check` reports the files that would change, without touching them, and exits with 1 if there are any:

```output run="ok self fmt --check tidy.ok" exit=1
would reformat tidy.ok
```

Without `--check`, it rewrites them:

```output run="ok self fmt tidy.ok"
formatted tidy.ok
```

The file now reads:

```ok
let files = glob(
  "**/*.log",
  root: ".",
  hidden: true,
  exclude: ["**/.git", "**/node_modules", "**/target"],
)
let r = {name: "app.log", size: 3}
if r.size > 2 { print("big") }
```

Running it again changes nothing and says `ok: 1 file already formatted`. Without file arguments, `ok self fmt` formats the project files around the current directory: the root `ok.ok`, the files in `ok/`, and what they import.

### What the formatter changes

The formatter keeps your line breaks and comments and normalises the rest:

- Indentation is two spaces per level. A line that continues a statement, such as a pipeline stage starting with `|` or a method chain starting with `.name`, gets one extra level.
- One space around binary operators and after `,` and `:`. No space inside parentheses, brackets and record literals (`{a: 1}`), but one inside the braces of a one-line block (`{ print(x) }`).
- At most one blank line in a row, none at the start or end of the file, and one newline at the end.
- A line longer than 100 characters is broken: at each `|` of a pipeline, or else by putting the elements of its largest bracket group (call arguments, parameters, a list, a record, `needs [...]`) on lines of their own, with a trailing comma.
- Comments stay where you put them. End-of-line comments keep their column, so aligned comments stay aligned.

Some things are never touched. String literals and commands after `run` are copied byte for byte, because changing them would change what the program does. Where a space changes meaning, the formatter keeps it: `f (x)` and `f(x)` are different in ok (a juxtaposed argument versus a call), as are `reset-db` and `reset - db`.

### Formatting is safe

After formatting, ok parses the result and compares its syntax tree and comments with the original. If they differ, it reports an internal error and leaves the file alone. So `ok self fmt` cannot change what your code does. A file that does not parse is reported with its syntax error and left unchanged, and the exit code is 1.

## Checking with `ok self check`

`ok self check` reads ok files and reports mistakes without running any of their code. It is fast, needs no trust, and does not need the programs your scripts call to be installed. Here is a script with three problems:

```ok file=report.ok
fn greet(name: string) {
  print("hello, {name}")
}

let release = args.len() > 0
if relase { greet("ok", "again") }
let report = {ok: true}
write("report.json", report)
```

```output run="ok self check report.ok" exit=1
error[name]: unknown name `relase`
 --> report.ok:6:4
  |
6 | if relase { greet("ok", "again") }
  |    ^^^^^^ not defined here
  |
  = help: did you mean `release`?
error[arg]: `greet` takes 1 positional argument, got 2
 --> report.ok:6:13
  |
6 | if relase { greet("ok", "again") }
  |             ^^^^^^^^^^^^^^^^^^^^
  |
  = note: `greet` is declared at report.ok:1:4
warning[arg]: the arguments of `write` look swapped: "report.json" looks like a path
 --> report.ok:8:1
  |
8 | write("report.json", report)
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: write(value, path) writes the value to the file at path
  = help: the value comes first and the path second: write(report, "report.json")
ok: 1 file checked: 2 errors and 1 warning
```

Running the script would only have found the first of these, and only when `args` was not empty. The checker reports every finding in every file, in file and source order.

### What it finds

- **Syntax errors**, and code at the top level of a project file that is not a declaration.
- **Unknown names**: a variable or function that is not declared where it is used, an assignment to a name that was never declared with `let` or `var`, a method call with no function of that name, a name an imported module does not export, and an `env` method that does not exist. The help line suggests a close name, or tells you the name is declared further down.
- **Wrong arguments** to a function it knows (a `fn` of the file or an import, a closure bound with `let`, or a builtin): too many or too few positional arguments, a missing required flag, a named argument that is not a flag of the function.
- **Imports** of files that do not exist.

It also has one **lint**, a warning about code that runs but is almost certainly wrong: `write` with its arguments swapped, as above.

### What it does not find

A finding always means the code fails when it runs, so the checker stays quiet where the outcome depends on values. It does not check types of arguments, fields of records (`r.name`, `env.HOME`), calls of functions held in variables or parameters, and code inside `try` or before `?`, where errors are meant to become values. It does not follow calls either: an unknown name inside a function is reported even if nothing calls that function. A clean check is not a test suite.

### Checking a project

Without file arguments, `ok self check` checks the project around the current directory. First it applies the rules for loading a project (namespaces, mounts, the tasks and arguments named in `needs`, dependency cycles); then it checks every project file and everything they import.

```ok file=ok.ok
# Compile
task build(--release: bool) {
  print("build")
}

# Ship a release build
task ship needs [build(relase: true)] {
  print("shipping")
}
```

```output run="ok self check" exit=1
error[arg]: task `build` has no parameter `relase`
 --> ok.ok:7:24
  |
7 | task ship needs [build(relase: true)] {
  |                        ^^^^^^
  |
  = help: did you mean `release`?
ok: the project failed to load: 1 error
```

An error like this one stops the check, because the project's files are only known once it loads. A clean project prints a one-line summary that also counts its tasks:

```ok file=ok.ok
# Compile
task build(--release: bool) {
  print("build")
}

# Ship a release build
task ship needs [build(release: true)] {
  print("shipping")
}
```

```output run="ok self check"
ok: 1 file and 2 tasks checked, no problems found
```

### Exit codes and `--strict`

`ok self check` exits with 1 when it found an error and 0 when it found only warnings or nothing, so a lint does not break your build. `--strict` makes warnings count too:

```ok file=save.ok
let totals = {apples: 3, pears: 5}
write("totals.json", totals)
```

```output run="ok self check save.ok"
warning[arg]: the arguments of `write` look swapped: "totals.json" looks like a path
 --> save.ok:2:1
  |
2 | write("totals.json", totals)
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: write(value, path) writes the value to the file at path
  = help: the value comes first and the path second: write(totals, "totals.json")
ok: 1 file checked: 1 warning
```

```output run="ok self check --strict save.ok" exit=1
warning[arg]: the arguments of `write` look swapped: "totals.json" looks like a path
 --> save.ok:2:1
  |
2 | write("totals.json", totals)
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: write(value, path) writes the value to the file at path
  = help: the value comes first and the path second: write(totals, "totals.json")
ok: 1 file checked: 1 warning
```

Findings and the summary go to stderr; a clean summary goes to stdout. Bad usage, such as an unknown option, exits with 2.

## Starting a project with `ok self init`

`ok self init` sets up a new project in the current directory. It writes a starter `ok.ok` and three bootstrap wrappers:

```text
$ ok self init
wrote ok.ok, ./ok, ok.cmd and ok.ps1 (pinned to ok 0.2.0)
run ./ok hello (or ok.cmd hello on Windows)
```

The starter file has one task to build on:

```ok
requires "0.1"

# Say hello
task hello(name: string = "world") {
  print("hello, {name}!")
}
```

To pin the wrappers, `ok self init` downloads the checksums of its own release (`SHA256SUMS` from the GitHub release of the version you are running). Without network access, download that file some other way and pass it with `--checksums SHA256SUMS`. `OK_DOWNLOAD_URL` points both `init` and the wrappers at a mirror instead of GitHub.

`ok self init` never overwrites anything: if any of `ok.ok`, `ok`, `ok.cmd` or `ok.ps1` exists, it stops with an error and writes nothing. To add wrappers to a project that already has an `ok.ok`, run it in an empty directory and copy the three wrapper files over.

## The `./ok` wrappers

The wrappers let anyone run your tasks with nothing but a fresh checkout. Commit them next to `ok.ok`:

- `ok` is a POSIX `sh` script for Linux and macOS: `./ok build`.
- `ok.cmd` and `ok.ps1` are for Windows, from `cmd.exe` or PowerShell: `ok.cmd build` or `.\ok.ps1 build`. `ok.cmd` only hands over to `ok.ps1`.

Each wrapper pins one ok version and the SHA-256 of each release binary. On first use it downloads the binary for the current OS and architecture into the user cache (`~/.cache/ok/VERSION/` on Linux and macOS, `%LOCALAPPDATA%\ok\VERSION\` on Windows), checks its checksum, and runs it with all your arguments. If the checksum does not match, it deletes the download and refuses to run it. Later runs use the cached binary straight away, so the wrapper costs nothing after the first time.

Because the version is pinned, everyone on the project, and CI, runs the same ok. If the project's `requires` is newer than the ok someone has installed, the error points at the wrapper as the fix. To move a project to a newer ok, delete the three wrappers, run `ok self init` with the new version in an empty directory, and copy the new wrappers in.

The wrappers are the only shell code ok produces, and they stay short enough to read before you run them. They still go through [trust](trust.md) like any other way of running tasks.

## Other `ok self` commands

| Command | What it does |
|---|---|
| `ok self tasks` | Print the task listing, the same as `ok` with no arguments. `--all` includes [hidden tasks](tasks.md#hidden-tasks). |
| `ok self completions SHELL` | Print a completion script for `bash`, `zsh`, `fish` or `powershell` (see [Tasks](tasks.md#shell-completions)). |
| `ok self complete -- WORDS...` | What the completion scripts call: prints the candidates for the last word. |
| `ok self version` | Print the version, such as `ok 0.2.0`. `ok --version` does the same. |

`ok self complete` is handy for checking what Tab will offer, without a shell:

```ok file=ok.ok
# Deploy the app
task deploy(target: "staging" | "production", --verbose, -v: bool) {
  print("deploying to {target}")
}
```

```output run="ok self complete -- deploy -"
--help
--verbose
-h
-v
```

After the task name with nothing typed yet, it offers the flags and the choices for the first argument:

```output run="ok self complete -- deploy ''"
--help
--verbose
production
staging
```

## In your editor and in CI

Both tools take file arguments, so editors can run them on one file:

- Format on save: run `ok self fmt FILE` on the saved file. It prints `formatted FILE` when something changed and leaves the file alone if it does not parse.
- Show problems: run `ok self check FILE`. Findings have the usual `FILE:LINE:COLUMN` location after `-->`, which most editors' problem matchers can read.

In CI, check formatting and correctness before running anything else. Neither needs trust, so they also work in a job that never runs a task:

```yaml
steps:
  - uses: actions/checkout@v4
  - run: ./ok self fmt --check
  - run: ./ok self check --strict
  - run: ./ok test
    env:
      OK_TRUST: "1"
```

`ok self fmt --check` fails the job and names the files when someone forgot to format; `ok self check --strict` fails on warnings too. A git pre-commit hook can run the same two commands; the [pre-commit example](../examples/pre-commit/) is one written in ok.
