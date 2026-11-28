# Formatter, checker and wrappers

The ok binary has its tools built in under `ok self`: `ok self fmt` formats code, `ok self check` finds mistakes without running code, and `ok self init` starts a project.

This page also covers `ok self promote`, the `./ok` wrappers that let people run your tasks without installing ok, and the use of these commands in an editor and in CI. No task can be named `self`, so these commands never clash with the tasks of a project. The definitions are in [section 17](../SPEC.md#17-built-in-commands-ok-self-) and [section 18](../SPEC.md#18-bootstrap-wrappers) of the spec.

## Formatting with `ok self fmt`

`ok self fmt` rewrites ok files in one canonical layout, which is the layout of the samples in this guide and of the examples in the repository. It has no options for style. This file has uneven spacing and one long line:

```ok file=tidy.ok nocheck
let files = glob("**/*.log", root: ".", hidden: true, exclude: ["**/.git", "**/node_modules", "**/target"])
let r = {name:"app.log" , size:3}
if r.size>2 {print( "big" )}
```

`--check` names the files that would change and changes nothing. It exits with 1 if there is such a file:

```output run="ok self fmt --check tidy.ok" exit=1
would reformat tidy.ok
```

Without `--check`, `ok self fmt` rewrites the files:

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

A second run changes nothing and prints `ok: 1 file already formatted`.

Without file arguments, `ok self fmt` formats the project files of the project around the current directory: the root file, the files in `.ok/` and the files they import.

### What the formatter changes

The formatter keeps your line breaks and comments and normalises the rest:

- Indentation is two spaces per level. A line that continues a statement, such as a pipeline stage that starts with `|` or a method chain that starts with `.name`, gets one more level.
- Binary operators have one space on each side, and `,` and `:` have one space after them. There is no space inside parentheses, brackets and record literals (`{a: 1}`), and one space inside the braces of a block on one line (`{ print(x) }`).
- There is at most one blank line in a row, no blank line at the start or the end of the file, and one newline at the end.
- A line longer than 100 characters is broken. A pipeline is broken before each `|`. Any other line is broken at its largest bracket group (call arguments, parameters, a list, a record or `needs [...]`), whose elements each get a line of their own and a trailing comma.
- Comments stay where they are. A comment at the end of a line keeps its column, so aligned comments stay aligned.

The formatter copies string literals and the commands after `run` byte for byte, because a change there would change what the program does.

Where a space changes the meaning, the formatter keeps it. `f (x)` passes `(x)` as a juxtaposed argument and `f(x)` is a call. `reset-db` is one name and `reset - db` is a subtraction.

### The formatter checks its own result

After formatting a file, ok parses the result and compares its syntax tree and its comments with those of the original. If they differ, ok reports an internal error and leaves the file as it was, so `ok self fmt` cannot change what your code does.

A file that does not parse is reported with its syntax error and left unchanged, and the exit code is 1.

## Checking with `ok self check`

`ok self check` reads ok files and reports mistakes without running any of their code. It needs no trust, and the programs that your scripts call do not have to be installed. This script has three problems:

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

Running the script would find only the first problem, and only when `args` is not empty. The checker reports every finding in every file, in the order of the files and of the source.

### What the checker finds

- Syntax errors, and code at the top level of a project file that is not a declaration.
- Unknown names. These are a variable or function that is not declared where it is used, an assignment to a name that was never declared with `let` or `var`, a method call for which no function of that name exists, a name that an imported module does not export, and an `env` method that does not exist. The help line suggests a similar name, or says that the name is declared further down.
- Wrong arguments in a call of a function that the checker knows: a `fn` of the file or of an import, a closure bound with `let`, or a builtin. It reports too many or too few positional arguments, a missing required flag, and a named argument that is not a flag of the function.
- An import of a file that does not exist.

The checker also has two lints, which are warnings about code that runs but is almost certainly wrong. One reports a call of `write` with its arguments swapped, as in the sample above. The other reports a `\` at the end of a command line that has spaces or tabs after it. Such a `\` does not continue the command on the next line. It is passed to the program as an argument.

### What the checker does not find

A finding always means that the code fails when it runs. Where the outcome depends on values, the checker reports nothing. It does not check:

- the types of arguments;
- the fields of records, such as `r.name` or `env.HOME`;
- calls of functions held in variables or parameters;
- code inside `try` or before `?`, where errors are meant to become values.

The checker does not follow calls either. It reports an unknown name inside a function even if nothing calls that function.

A check without findings therefore does not show that a script works. Run or test the script as well.

### Checking a project

Without file arguments, `ok self check` checks the project around the current directory. It first applies the rules for loading a project: namespaces, mounts, the tasks and arguments named in `needs`, and dependency cycles. Then it checks every project file and every file they import.

```ok file=.ok/ok.ok
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
 --> .ok/ok.ok:7:24
  |
7 | task ship needs [build(relase: true)] {
  |                        ^^^^^^
  |
  = help: did you mean `release`?
ok: the project failed to load: 1 error
```

An error in loading the project, like this one, stops the check, because ok knows the files of a project only after the project loads. For a project without findings, ok prints a summary of one line that also counts the tasks:

```ok file=.ok/ok.ok
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

`ok self check` exits with 1 when it found an error, and with 0 when it found only warnings or nothing. A lint therefore does not fail your build. With `--strict`, a warning also gives exit code 1:

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

Findings and their summary go to stderr. The summary of a check without findings goes to stdout. Bad usage, such as an unknown option, exits with 2.

## Starting a project with `ok self init`

`ok self init` sets up a project in the current directory. It writes a starter `.ok/ok.ok` and nothing else:

```output run="ok self init"
wrote .ok/ok.ok
run ok hello
```

The starter file has one task, and it requires the version of ok that wrote it (here 0.4):

```ok
requires "0.4"

# Say hello
task hello(name: string = "world") {
  print("hello, {name}!")
}
```

`ok self init` does not overwrite a file. If the directory already has a root file (`ok.ok`, `.ok` or `.ok/ok.ok`), it stops with an error and writes nothing.

### Adding the wrappers

With `--wrappers`, `ok self init` also writes the three [`./ok` wrappers](#the-ok-wrappers), so that people without ok installed can run the tasks:

```text
$ ok self init --wrappers
wrote .ok/ok.ok, ./ok, ok.cmd and ok.ps1 (pinned to ok 0.4.1)
run ./ok hello (or ok.cmd hello on Windows)
```

In a project that already has a root file, `ok self init --wrappers` leaves the root file alone and writes only the three wrappers. It stops with an error, and writes nothing, if `ok`, `ok.cmd` or `ok.ps1` exists. A deprecated `ok/` directory counts as well, because the `./ok` wrapper needs that name.

The wrappers hold the checksums of the release binaries. `ok self init --wrappers` downloads these checksums from the release of the ok version you are running: the file `SHA256SUMS` of that GitHub release. Without `--wrappers`, `ok self init` does not use the network.

Two settings change where the checksums come from:

- `--checksums FILE` reads them from a `SHA256SUMS` file that you downloaded yourself. Use it on a machine without network access.
- `OK_DOWNLOAD_URL` makes `ok self init` and the wrappers download from a mirror instead of GitHub. `ok self init` downloads checksums over `https://` only. For a mirror with plain HTTP or `file://`, check a `SHA256SUMS` file yourself and pass it with `--checksums`.

Every entry must be a SHA-256 digest of 64 hex digits, because the entries become part of the code of the wrappers.

## Moving to `.ok/` with `ok self promote`

`ok self promote` moves a project whose root file is a single `.ok` file to the `.ok/` directory, where it can have namespaces and helper files. The file moves to `.ok/ok.ok` with its content unchanged:

```ok file=.ok
# Say hello
task hello {
  print("hello")
}
```

```output run="ok self promote"
moved .ok to .ok/ok.ok
the project files moved, so the next task run asks for trust again (or run ok trust)
```

`ok self promote` also moves the files of a deprecated `ok/` directory into `.ok/` and removes the empty `ok/`. The warning that ok prints about `ok/` asks for this move. A root file named `ok.ok` stays where it is.

ok checks every move before it makes one. If a file would replace an existing file, ok moves nothing. In a project that already uses `.ok/ok.ok`, `ok self promote` says that there is nothing to promote.

### After the move

An import is relative to the file it is in, so an import in a moved file can point at another place than before. `import "lib/util.ok"` in the file `.ok` means `lib/util.ok`, and in `.ok/ok.ok` it means `.ok/lib/util.ok`. `ok self promote` does not edit your files. It lists each import that changed its meaning, with the path to write instead (here `"../lib/util.ok"`).

Paths in `sources`, in `outputs` and in commands are relative to the project root. They stay correct, unless they name files in `ok/`. After it moves `ok/`, `ok self promote` lists each string and command argument that still names `ok/`, with its file and line. Check each one and write `.ok/` where the path means the moved directory.

ok trusts files under their paths, so the project needs [trust](trust.md) again after the move. Review the files and answer the prompt on the next task run, or run `ok trust`.

## The `./ok` wrappers

With the wrappers, a person with a fresh checkout and no ok installed can run your tasks. [`ok self init --wrappers`](#adding-the-wrappers) writes them. Commit them in the project root:

- `ok` is a POSIX `sh` script for Linux and macOS: `./ok build`.
- `ok.ps1` is a PowerShell script for Windows: `.\ok.ps1 build`.
- `ok.cmd` is for `cmd.exe` on Windows: `ok.cmd build`. It only starts `ok.ps1`.

Each wrapper pins one ok version and the SHA-256 checksum of each release binary. On its first run, a wrapper does this:

1. It downloads the binary for the current OS and architecture into the user cache.
2. It compares the checksum of the download with the pinned checksum. If they differ, it deletes the download and stops.
3. It runs the binary with all your arguments.

Later runs find the binary in the cache and download nothing.

The cache directory is `~/.cache/ok/VERSION-SHA/` on Linux and macOS and `%LOCALAPPDATA%\ok\VERSION-SHA\` on Windows. `SHA` is the first 16 digits of the pinned checksum, so only wrappers that pin the same binary share a cached one.

Because the version is pinned, everyone on the project runs the same ok, and so does CI. If the `requires` of the project is newer than the ok that someone has installed, the error names the wrapper as the fix.

To move a project to a newer ok, install that version, delete the three wrappers and run `ok self init --wrappers` in the project root.

### The wrappers run before the trust check

`./ok` is a shell script from the repository, and your shell runs it before ok starts. The [trust](trust.md) check happens only when the downloaded ok runs a task, so it cannot cover the wrapper itself. A repository that you do not trust could contain a changed wrapper that does anything. In a repository that you have not reviewed, run your installed `ok` and not `./ok`. The trust prompt then protects you as the [Trust](trust.md) page describes.

The wrappers are the only shell code that ok produces, and they are short, so you can read them before you run them.

## Other `ok self` commands

| Command | What it does |
|---|---|
| `ok self tasks` | Prints the task listing, like `ok` with no arguments. `--all` includes [hidden tasks](tasks.md#hidden-tasks). |
| `ok self completions SHELL` | Prints a completion script for `bash`, `zsh`, `fish` or `powershell` (see [Tasks](tasks.md#shell-completions)). |
| `ok self complete -- WORDS...` | Prints the completion candidates for the last word. The completion scripts call it. |
| `ok self version` | Prints the version, such as `ok 0.4.1`. `ok --version` does the same. |

`ok self --help` prints the list of all `ok self` commands.

`ok self complete` shows what Tab offers without a shell. This sample asks for the completions of `-` after the task name `deploy`:

```ok file=.ok/ok.ok
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

After the task name, with nothing typed yet, it offers the flags and the choices for the first argument:

```output run="ok self complete -- deploy ''"
--help
--verbose
production
staging
```

## In your editor and in CI

Both `ok self fmt` and `ok self check` take file arguments, so an editor can run them on one file.

To format on save, make the editor run `ok self fmt FILE` on the saved file. The command prints `formatted FILE` when it changed the file, and it leaves a file that does not parse as it was.

To show problems, make the editor run `ok self check FILE`. Each finding has a `FILE:LINE:COLUMN` location after `-->`, which the problem matchers of most editors can read.

In CI, check the format and the code before running anything else. Neither command needs trust, so both also work in a job that never runs a task:

```yaml
steps:
  - uses: actions/checkout@v4
  - run: ./ok self fmt --check
  - run: ./ok self check --strict
  - run: ./ok test
    env:
      OK_TRUST: "1"
```

`ok self fmt --check` fails the job and names the files that are not formatted. `ok self check --strict` also fails on warnings. A git pre-commit hook can run the same two commands. The [pre-commit example](../examples/pre-commit/) is such a hook, written in ok.
