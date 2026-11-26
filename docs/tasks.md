# Tasks

Put an `ok.ok` file in your project and its tasks run with `ok NAME`: arguments, dependencies, parallel runs, up-to-date checks and namespaces.

A task is a named piece of ok code that lives in a project file. It is what `make` targets, `npm run` scripts and `just` recipes are elsewhere, but it is written in the same language as your scripts, it parses its own arguments, and it behaves the same on Linux, macOS and Windows. This page shows how to write tasks and run them. The exact rules are in [section 15 of the spec](../SPEC.md#15-project-mode).

## Your first project

A project is a directory with a file called `ok.ok` in it. Each `task` in that file is something you can run:

```ok file=ok.ok
# Say hello
task hello {
  print("hello from a task")
}

# Write the build stamp
task stamp {
  write("built\n", "out/stamp.txt")
  print("wrote out/stamp.txt")
}
```

```output run="ok hello"
hello from a task
```

```output run="ok stamp"
wrote out/stamp.txt
```

The comment directly above a task is its description. Run `ok` with no arguments to list the tasks of the project:

```text
myapp (/home/you/src/myapp)

  hello        Say hello
  stamp        Write the build stamp
```

The first line is the project's directory name and root. Root tasks come first, in the order you declared them, then each namespace (see [below](#namespaces)) in alphabetical order. `ok self tasks` prints the same listing.

You can run `ok` from any subdirectory: it walks up to the nearest `ok.ok` and treats that directory as the **project root**. Tasks always run with the project root as working directory, so `"out/stamp.txt"` means the same file wherever you started. The directory you started in is in `invocation_dir`. To run a project's tasks from somewhere else, give its directory with `-C`: `ok -C ../other build`.

The first time you run a task in a project, ok asks whether you trust the project's files. That is explained on the [Trust](trust.md) page; for your own projects you can answer once with `ok trust --dir`.

## Project files hold declarations

A project file is read every time you list tasks, ask for help or press Tab, so it must not do anything on its own. At the top level you may only write `task`, `fn`, `let` with a constant value, `import`, and in the root `ok.ok` also `requires` and `env file`. Anything else is an error before any task runs:

```ok file=ok.ok nocheck
let greeting = "hello"

print("loading")

# Greet
task greet {
  print(greeting)
}
```

```output run="ok greet" exit=1
error[syntax]: project files cannot run code at the top level
 --> ok.ok:3:1
  |
3 | print("loading")
  | ^^^^^^^^^^^^^^^^
  |
  = help: move it into a task, or compute it inside the task that needs it
```

A constant is built from literals, lists, records, operators, string interpolation, other constants, `path(...)`, `os.name`, `os.arch` and `os.exe`. If you need a value that comes from a file or a command, compute it inside the task, or in a `fn` the task calls.

In a project file, `task` is public and `fn` is private. A `fn` is a helper that tasks call; it never shows up in the listing and cannot be run from the command line. A task cannot be called like a function: share code through a `fn` instead.

```ok file=ok.ok
let app = "shop"

# Show the version
task version {
  print(banner())
}

fn banner() {
  "{app} on {os.name}"
}
```

`requires "0.2"` at the top of the root `ok.ok` declares the oldest ok version your tasks work with. An older ok refuses to load the project and says which version is needed.

## Task arguments

A task takes parameters exactly like a script's `main` function: positional parameters, flags with `--`, defaults, types, short forms and choices. The command-line rules and help output are described on the [Command-line scripts](script-cli.md) page; everything there applies to tasks.

```ok file=ok.ok
# Greet someone
#
# Prints a greeting, louder on request.
task greet(
  name: string,         # who to greet
  --loud, -l: bool,     # shout it
  --times: int = 1,     # how many times
) {
  let text = if loud { "HELLO, {name.upper()}!" } else { "hello, {name}" }
  for _ in range(times) { print(text) }
}
```

```output run="ok greet bas --times 2"
hello, bas
hello, bas
```

```output run="ok greet bas -l"
HELLO, BAS!
```

`--help` after a task name prints its help, built from the doc comment and the comments at the end of each parameter line:

```output run="ok greet --help"
Greet someone

Prints a greeting, louder on request.

Usage: ok greet <name> [--loud] [--times <int>]

Arguments:
  <name>             who to greet

Options:
  -l, --loud         shout it
      --times <int>  how many times [default: 1]
  -h, --help         print this help
```

Arguments are checked before the task's code runs. A typo gets a suggestion and exit code 2:

```output run="ok greet bas --tims 3" exit=2
error: unknown flag --tims
  = help: did you mean --times?
usage: ok greet <name> [--loud] [--times <int>]
run with --help for details
```

ok's own options, such as `--dry-run` or `--jobs`, go before the task name: `ok --dry-run greet bas`. Everything after the name belongs to the task.

A `path` parameter is resolved against the directory you ran `ok` from, not the project root, so `ok count notes.md` in a subdirectory finds the file you meant even though the task runs at the root.

## Dependencies with `needs`

`needs [...]` lists tasks that must finish before this one starts. A task that several others need runs only once per invocation:

```ok file=ok.ok
# Generate code
task gen {
  print("gen")
}

# Compile
task build needs [gen] {
  print("build")
}

# Run the tests
task test needs [gen, build] {
  print("test")
}
```

```output run="ok -j1 test"
gen
build
test
```

`gen` is needed by both `test` and `build`, but it runs once. A name is looked up in the file's own namespace first, then in the root; `db.migrate` names a task in a namespace.

### Passing arguments to a dependency

A dependency can be given arguments, written straight after its name. They are named, like the named arguments of a call, and that includes positional parameters:

```ok file=ok.ok
# Compile for one target
task build(--release: bool, --target: string = "native") {
  print("build {target} (release: {release})")
}

# Greet someone
task greet(name: string) {
  print("hello, {name}")
}

# Build every release binary
task dist needs [
  greet(name: "release manager"),
  build(release: true),
  build(release: true, target: "wasm32"),
] {
  print("dist done")
}
```

```output run="ok -j1 dist"
hello, release manager
build native (release: true)
build wasm32 (release: true)
dist done
```

A task together with its arguments is an **instance**. Two entries with the same arguments are the same instance and run once; different arguments make different instances, and each of them runs. Defaults are not filled in for this comparison: `build` and `build(release: false)` are two instances.

The values must be constants, so ok can plan the whole run without executing any of your code. Mistakes are found when the project loads, before anything runs:

```ok file=ok.ok
# Deploy to one environment
task deploy(target: string) {
  print("deploying to {target}")
}

# Tag and deploy a release
task release needs [deploy] {
  print("released")
}
```

```output run="ok release" exit=1
error[arg]: task `deploy` needs the argument `target`
 --> ok.ok:7:21
  |
7 | task release needs [deploy] {
  |                     ^^^^^^
  |
  = help: give it here: deploy(target: ...)
```

Misspelt parameter names get a suggestion, and a cycle such as `a needs b, b needs a` is reported as `task dependency cycle: a -> b -> a`. `ok build --help` lists what a task runs first, with the arguments as written (`Runs first: gen, build(release: true)`).

## Parallel runs and `--jobs`

Dependencies that do not depend on each other run at the same time. In the `dist` example above, the two `build` instances and `greet` are independent, so by default they run in parallel, at most as many at once as ok has CPUs. `-j N` (or `--jobs N`) changes the limit, and `-j1` runs everything one at a time, depth first in the order you listed it.

A task that runs in parallel with another runs in a separate ok process, a **worker**, and each line it prints is prefixed with its label so you can tell the outputs apart:

```text
build(release: true)                   | build native (release: true)
build(release: true, target: "wasm32") | build wasm32 (release: true)
greet(name: "release manager")         | hello, release manager
dist done
```

Lines are never split or mixed, but their order from different workers depends on timing. The task you asked for always runs last, in ok's own process, without a prefix. A chain where each step needs the previous one never runs in parallel, so its output looks exactly as with `-j1`.

Workers have no terminal. Their standard input is empty, `prompt()` returns its default or fails, and `confirm()` only succeeds with `--yes`. Commands a worker runs write into a pipe, so some tools turn off their colours. If a dependency needs to talk to you, run with `-j1`.

When a task fails, ok starts nothing new, lets the running workers finish (it never cuts a command off halfway), and then fails with the exit code of the first failure:

```text
lint | lint ok
test | error[fail]: 2 tests failed
test |   --> ok.ok:10:3
...
error: task `test` failed with exit code 1
  = its output is above, each line starting with `test |`
```

## Skipping work that is up to date

A task that turns some files into other files can say which they are with `sources` and `outputs`. ok then skips it when the outputs are newer than the sources, the way `make` does:

```text file=VERSION
1.4.0
```

```ok file=ok.ok
# Write src/version.rs from the VERSION file
task gen sources ["VERSION"] outputs ["src/version.rs"] {
  let version = read("VERSION").trim()
  write("pub const VERSION: &str = \"{version}\";\n", "src/version.rs")
  print("wrote src/version.rs for {version}")
}
```

```output run="ok gen"
wrote src/version.rs for 1.4.0
```

Run it a second time without changing `VERSION`, and ok skips the body:

```text
gen: up to date
```

Both lists hold glob patterns relative to the project root (or to the sub-project for a mounted task). A task is skipped when it declares `outputs`, every output pattern matches at least one file, and the oldest output is newer than the newest source. Only modification times are compared. The check happens after the task's dependencies have run, so a dependency that regenerates a source makes the task run again. `ok --force gen` runs it regardless, along with every other task of that invocation.

List the files that affect the result in `sources`, including a helper module the task imports: if you change the generator, the output is out of date too.

## Namespaces

A larger project can split its tasks over several files. Each `.ok` file directly in an `ok/` directory next to the root `ok.ok` is a **namespace** named after the file:

```text
myapp/
  ok.ok          root tasks: ok build, ok test
  ok/db.ok       namespace db: ok db migrate, ok db seed
  ok/admin.ok    namespace admin: ok admin create-user
  ok/_lib.ok     helpers shared by the files above; not a namespace
```

```ok file=ok.ok
# Compile the server
task build {
  print("build")
}
```

```ok file=ok/db.ok
# Apply pending migrations
task migrate needs [build] {
  print("migrate")
}

# Load the seed data
task seed needs [migrate] {
  print("seed")
}
```

```output run="ok db seed"
build
migrate
seed
```

`migrate` needs `build`, which is not in `db.ok`, so ok finds it in the root. `ok db` on its own lists the tasks of that namespace. A file whose name starts with `_` is never a namespace: use it for functions the other files share, with `import "./_lib.ok"`. Files in subdirectories of `ok/` are only reachable through `import`. A namespace cannot have the same name as a root task, and cannot be called `trust`, `untrust` or `self`, which ok reserves for its own commands.

### Mounting a sub-project

In a monorepo, a subdirectory often is a project of its own, with its own `ok.ok`. The root `ok.ok` can **mount** it as a namespace:

```ok file=ok.ok
import "web/ok.ok" as web

# Build everything
task build needs [web.bundle] {
  print("server built")
}
```

```ok file=web/ok.ok
# Bundle the frontend
task bundle {
  write("bundle\n", "dist/bundle.js")
  print("bundle written")
}
```

```output run="ok build"
bundle written
server built
```

`ok web bundle` runs the same task directly. A mounted task runs with the sub-project's directory as working directory, so `dist/bundle.js` above ends up in `web/dist/`, and its `sources` and `outputs` are relative to `web/` too. Only the root `ok.ok` may mount, and only the sub-project's root tasks are visible: for its own namespaces, use `ok -C web NAME TASK`. The [monorepo example](../examples/monorepo/) puts namespaces, a helper file and a mount together.

## Confirmations

A task that does something hard to undo can ask first. Add `confirm` with a question:

```ok file=ok.ok
# Wipe the local database
task reset-db confirm "This wipes the local database" {
  print("database wiped")
}

# Wipe and reload the sample data
task fresh needs [reset-db] {
  print("sample data loaded")
}
```

On a terminal, `ok fresh` asks `This wipes the local database. Continue? [y/N]` before anything runs, including the dependencies that come before `reset-db`. ok collects the confirmations of every task in the invocation and asks them all up front, so you never answer half a run. `--yes` answers yes for you:

```output run="ok --yes fresh"
database wiped
sample data loaded
```

Without a terminal and without `--yes`, ok does not wait for input; it fails straight away and tells you what to do:

```output run="ok fresh" exit=1
error: cannot ask "This wipes the local database. Continue?": there is no terminal
  = note: task `reset-db` asks for confirmation
  = help: pass --yes before the script or task name to answer yes
```

### Hidden tasks

`hidden` keeps a task out of the listing and out of shell completion, for internal steps that other tasks need but people rarely run by hand. It can still be run by name, and `ok self tasks --all` lists it.

```ok
# Install dependencies when the lockfile changed
task deps hidden sources ["package-lock.json"] outputs ["node_modules/.package-lock.json"] {
  run npm ci
}
```

Modifiers (`needs`, `sources`, `outputs`, `confirm`, `hidden`) may come in any order, each at most once.

## Environment files

Many projects keep settings in a `.env` file. `env file ".env"` in the root `ok.ok` loads it into the environment before any task runs:

```text file=.env
GREETING=hello
DATABASE_URL=postgres://localhost/dev
```

```text file=.env.staging
DATABASE_URL=postgres://staging.internal/app
```

```ok file=ok.ok
env file ".env"

# Show the settings
task show {
  print("{env.GREETING}: {env.DATABASE_URL}")
}
```

```output run="ok show"
hello: postgres://localhost/dev
```

`--env NAME` also loads `.env.NAME`, whose values override the base file:

```output run="ok --env staging show"
hello: postgres://staging.internal/app
```

A variable that is already set in your environment wins over both files, so `DATABASE_URL=... ok show` works as you would expect. A missing `.env` is ignored, but a file asked for with `--env` must exist. The format (quotes, comments, no variable expansion) is described in [section 13.7 of the spec](../SPEC.md#137-env).

## Trying a task with `--dry-run`

`--dry-run` prints the commands a task would run and the file changes it would make, without doing either. It is a good way to read what an unfamiliar task does:

```ok file=ok.ok
# Package the release
task package {
  write("1.4.0\n", "dist/VERSION")
  run cargo build --release
}
```

```output run="ok --dry-run package" browser=no
~ write dist/VERSION
~ cargo build --release
```

With `--dry-run`, tasks always run one at a time, so the printed plan has a stable order. `--trace` is the other way round: it runs everything and prints each command before it starts.

## Shell completions

ok can complete task names, namespaces, flags and choice values when you press Tab. Print the script for your shell and load it from your shell's start-up file:

```sh
# bash, in ~/.bashrc
eval "$(ok self completions bash)"

# zsh, in ~/.zshrc
eval "$(ok self completions zsh)"

# fish
ok self completions fish > ~/.config/fish/completions/ok.fish
```

```powershell
# PowerShell, in $PROFILE
ok self completions powershell | Out-String | Invoke-Expression
```

Completion reads the project files but never runs any of their code, and it does not need trust.

## Walkthrough: replacing a Makefile

Here is a typical small Makefile for a Rust project that generates a source file before it builds:

```make
VERSION := $(shell cat VERSION)

.PHONY: all build release test clean

all: build

src/version.rs: VERSION
	echo 'pub const VERSION: &str = "$(VERSION)";' > src/version.rs

build: src/version.rs
	cargo build

release: src/version.rs
	cargo build --release

test: src/version.rs
	cargo test

clean:
	rm -rf target src/version.rs
```

Take it one rule at a time.

1. The file rule `src/version.rs: VERSION` becomes a task with `sources` and `outputs`. The shell commands (`cat`, `echo`, a redirect) become `read` and `write`, which work the same on Windows.
2. `build` and `release` only differ by a flag, so they become one task with a `--release` parameter.
3. Each prerequisite becomes a `needs` entry.
4. `.PHONY` disappears: tasks are never confused with files.
5. `rm -rf` becomes `remove`, which does not need a shell and does not fail when the file is already gone.
6. The comments above the rules become descriptions, so `ok` lists them.

```text file=VERSION
1.4.0
```

```ok file=ok.ok
requires "0.2"

# Write src/version.rs from the VERSION file
task gen sources ["VERSION"] outputs ["src/version.rs"] {
  let version = read("VERSION").trim()
  write("pub const VERSION: &str = \"{version}\";\n", "src/version.rs")
}

# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}

# Run the tests
task test needs [gen] {
  run cargo test
}

# Remove build output and generated code
task clean {
  remove("target")
  remove("src/version.rs")
}
```

Check what `ok build --release` would do without running it:

```output run="ok --dry-run build --release" browser=no
~ write src/version.rs
~ cargo build --release
```

`make` with no target ran `all`; `ok` with no task lists what there is instead, so a newcomer sees `build`, `test`, `gen` and `clean` with their descriptions. Arguments are real flags with help (`ok build --help`) rather than `make build RELEASE=1`, and a misspelt one is an error rather than a silently ignored variable. The [codegen-build example](../examples/codegen-build/) is a longer version of the same rewrite, and [docker-image](../examples/docker-image/) shows tasks with several parameters.

Before you commit, run `ok self check` and `ok self fmt` (see [Formatter, checker and wrappers](tools.md)), and add the `./ok` wrapper so that people without ok installed can run your tasks.
