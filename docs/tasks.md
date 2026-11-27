# Tasks

A task is a named piece of ok code in a project, run with `ok NAME`. This page shows how to write tasks, give them arguments and dependencies, and run them.

Tasks do the work of `make` targets, `npm run` scripts and `just` recipes. They are written in the same language as your scripts, each task parses its own arguments, and a task behaves the same on Linux, macOS and Windows. The exact rules are in [section 15 of the spec](../SPEC.md#15-project-mode).

## Your first project

A project is a directory with a **root file**. The usual root file is `.ok/ok.ok`, inside a hidden `.ok/` directory that holds everything ok needs. You can write the file yourself, or let [`ok self init`](tools.md#starting-a-project-with-ok-self-init) create it. Each `task` in the root file is something you can run:

```ok file=.ok/ok.ok
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

The first time you run a task in a project, ok asks whether you trust the project's files. The [Trust](trust.md) page explains the prompt. For a project of your own, `ok trust --dir` trusts the directory, and ok does not ask again when you change the files.

The comment directly above a task is its description. To list the tasks of the project, run `ok` with no arguments:

```text
myapp (/home/you/src/myapp)

  hello        Say hello
  stamp        Write the build stamp
```

The first line shows the name of the project's directory and its full path. The tasks of the root file come first, in the order you declared them. A project with [namespaces](#namespaces) lists each namespace after them, in alphabetical order. `ok self tasks` prints the same listing.

### Where ok looks for the root file

You can run `ok` from any subdirectory. ok walks up to the nearest directory with a root file, and that directory is the **project root**. Tasks always run with the project root as working directory, so `"out/stamp.txt"` means the same file wherever you started. The directory you started in is available as `invocation_dir`. To run the tasks of a project in another directory, name the directory with `-C`: `ok -C ../other build`.

The root file can have two other forms:

- A file named `.ok` in the project root. This form suits a small project with only root tasks. When the project grows, [`ok self promote`](tools.md#moving-to-ok-with-ok-self-promote) moves the file to `.ok/ok.ok`.
- A file named `ok.ok` in the project root. Projects used this form before the `.ok/` directory existed, and it still works.

A directory has one root file. Two root files in one directory are an error that names both.

## Project files hold declarations

The root file and the other ok files of a project are its **project files**. ok reads them every time you list tasks, ask for help or press Tab, so reading them must not run anything. At the top level of a project file you may write only `task`, `fn`, `let` with a constant value and `import`, and in the root file also `requires` and `env file`. Anything else is an error before any task runs:

```ok file=.ok/ok.ok nocheck
let greeting = "hello"

print("loading")

# Greet
task greet {
  print(greeting)
}
```

```output run="ok greet" exit=1
error[syntax]: project files cannot run code at the top level
 --> .ok/ok.ok:3:1
  |
3 | print("loading")
  | ^^^^^^^^^^^^^^^^
  |
  = help: move it into a task, or compute it inside the task that needs it
```

A constant is built from literals, lists, records, operators, string interpolation, other constants, `path(...)`, `os.name`, `os.arch` and `os.exe`. A value that comes from a file or a command is not a constant. Compute it inside the task, or in a `fn` that the task calls.

In a project file, `task` is public and `fn` is private. A `fn` is a helper that tasks call. It does not appear in the listing and cannot be run from the command line. A task cannot be called like a function, so code that two tasks share goes in a `fn`:

```ok file=.ok/ok.ok
let app = "shop"

# Show the version
task version {
  print(banner())
}

fn banner() {
  "{app} on {os.name}"
}
```

`requires "0.2"` at the top of the root file declares the oldest ok version that your tasks work with. An older ok refuses to load the project and says which version is needed.

## Task arguments

A task takes parameters in the same way as the `main` function of a script: positional parameters, flags with `--`, defaults, types, short forms and choices. The [Command-line scripts](script-cli.md) page describes the command-line rules and the help output, and all of it applies to tasks.

```ok file=.ok/ok.ok
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

`--help` after a task name prints the help of the task. ok builds it from the comment above the task and the comment at the end of each parameter line:

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

ok checks the arguments before the code of the task runs. A misspelt flag gets a suggestion and exit code 2:

```output run="ok greet bas --tims 3" exit=2
error: unknown flag --tims
  = help: did you mean --times?
usage: ok greet <name> [--loud] [--times <int>]
run with --help for details
```

ok's own options, such as `--dry-run` or `--jobs`, go before the task name: `ok --dry-run greet bas`. Everything after the task name belongs to the task.

A relative `path` argument is resolved against the directory you ran `ok` from, although the task runs in the project root. For a task `count` with a `path` parameter, `ok count notes.md` in a subdirectory reads the `notes.md` of that subdirectory.

## Dependencies with `needs`

`needs [...]` lists the tasks that must finish before a task starts. A task that several others need runs once per invocation:

```ok file=.ok/ok.ok
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

Both `test` and `build` need `gen`, and `gen` runs once.

ok looks up a name in `needs` in the namespace of the file first, then in the root. A name with a dot, such as `db.migrate`, is a task in a [namespace](#namespaces).

### Passing arguments to a dependency

A dependency can get arguments, written directly after its name. Every argument is given by name, like a named argument of a call, and that includes positional parameters:

```ok file=.ok/ok.ok
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

A task together with its arguments is an **instance**. Two entries with the same arguments are the same instance, which runs once. Entries with different arguments are different instances, and each of them runs. ok compares the arguments as written and does not fill in defaults, so `build` and `build(release: false)` are two instances.

The values must be constants, so that ok can plan the whole run without running any of your code. ok finds mistakes in `needs` when the project loads, before anything runs:

```ok file=.ok/ok.ok
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
 --> .ok/ok.ok:7:21
  |
7 | task release needs [deploy] {
  |                     ^^^^^^
  |
  = help: give it here: deploy(target: ...)
```

A misspelt parameter name gets a suggestion. A cycle such as `a needs b, b needs a` is reported as `task dependency cycle: a -> b -> a`.

`ok build --help` also lists the tasks that `build` runs first, with their arguments as written: `Runs first: gen, build(release: true)`.

## Parallel runs and `--jobs`

Dependencies that do not depend on each other run at the same time. In the `dist` sample above, the two `build` instances and `greet` are independent, so they run in parallel. By default ok runs at most as many instances at once as it has CPUs. `-j N` (or `--jobs N`) sets another limit. `-j1` runs the instances one at a time, depth first, in the order you listed them.

An instance that can run at the same time as another runs in a separate ok process, a **worker**. ok puts the label of the worker before each line the worker prints, so you can tell the outputs apart:

```text
build(release: true)                   | build native (release: true)
build(release: true, target: "wasm32") | build wasm32 (release: true)
greet(name: "release manager")         | hello, release manager
dist done
```

A line is never mixed with another line, but the order of lines from different workers depends on timing. The task you asked for always runs last, in ok's own process, without a label. In a chain where each task needs the previous one, nothing runs in parallel, and the output is that of `-j1`.

Workers have no terminal:

- Their standard input is empty.
- `prompt()` returns its default or fails, and `confirm()` succeeds only with `--yes`.
- Commands that a worker runs write into a pipe, so some tools turn off their colours.

When a dependency has to ask you something, run with `-j1`.

When a task fails, ok starts no new instance and waits for the running workers to finish. It does not stop a command halfway. Then ok fails with the exit code of the first failure:

```text
lint | lint ok
test | error[fail]: 2 tests failed
test |   --> .ok/ok.ok:10:3
...
error: task `test` failed with exit code 1
  = its output is above, each line starting with `test |`
```

## Skipping work that is up to date

A task that turns files into other files can name them with `sources` and `outputs`. ok then skips the task when the outputs are newer than the sources, as `make` does:

```text file=VERSION
1.4.0
```

```ok file=.ok/ok.ok
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

On a second run with `VERSION` unchanged, ok skips the body of the task and prints:

```text
gen: up to date
```

Both lists hold glob patterns relative to the project root, or relative to the sub-project for a [mounted](#mounting-a-sub-project) task. ok skips a task when all of this is true:

- the task declares `outputs`;
- every output pattern matches at least one file;
- the oldest output is newer than the newest source.

ok compares modification times only, not file contents. It makes the check after the dependencies of the task have run, so a dependency that regenerates a source makes the task run again. `ok --force gen` runs `gen` and every other task of that invocation, up to date or not.

List every file that affects the result in `sources`. That includes a helper module that the task imports, because a change to the generator makes the output out of date too.

## Namespaces

A larger project can spread its tasks over several files. Each `.ok` file directly in the `.ok/` directory, other than `ok.ok`, is a **namespace** named after the file:

```text
myapp/
  .ok/ok.ok      root tasks: ok build, ok test
  .ok/db.ok      namespace db: ok db migrate, ok db seed
  .ok/admin.ok   namespace admin: ok admin create-user
  .ok/_lib.ok    helpers shared by the files above; not a namespace
```

```ok file=.ok/ok.ok
# Compile the server
task build {
  print("build")
}
```

```ok file=.ok/db.ok
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

`migrate` needs `build`. There is no `build` in `db.ok`, so ok takes the one in the root file. `ok db` without a task name lists the tasks of the namespace.

These rules apply to the files in `.ok/`:

- A file whose name starts with `_` is not a namespace. It holds functions that the other files share, and they load it with `import "./_lib.ok"`.
- Files in subdirectories of `.ok/` are reachable only through `import`.
- A namespace cannot have the name of a root task.
- A namespace cannot be named `trust`, `untrust` or `self`, which ok reserves for its own commands.

A project whose root file is `ok.ok` can have namespaces in `.ok/` too. A project whose root file is a file named `.ok` cannot have a `.ok/` directory, so it has only root tasks.

### Namespaces in an `ok/` directory are deprecated

Projects made with an ok older than 0.4.0 keep their namespaces in an `ok/` directory. For now ok still loads them, and it prints a warning on every run, because a directory named `ok` leaves no room for the `./ok` wrapper. [`ok self promote`](tools.md#moving-to-ok-with-ok-self-promote) moves the files to `.ok/`.

### Mounting a sub-project

In a monorepo, a subdirectory is often a project of its own, with its own root file. The root file of the parent project can **mount** that sub-project as a namespace by importing its root file. Like every import, the path is relative to the importing file, so from `.ok/ok.ok` it starts with `../`:

```ok file=.ok/ok.ok
import "../web/ok.ok" as web

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

`ok web bundle` runs the same task directly. A mounted task runs with the directory of the sub-project as working directory, so `dist/bundle.js` above is written to `web/dist/`. Its `sources` and `outputs` are relative to `web/` too.

A mount has three limits:

- Only the root file may mount.
- The mounted file must be the root file of the sub-project: `web/ok.ok`, `web/.ok/ok.ok` or `web/.ok`.
- Only the root tasks of the sub-project are visible. To run a task in one of its namespaces, use `ok -C web NAME TASK`.

The [monorepo example](../examples/monorepo/) combines namespaces, a helper file and a mount.

## Confirmations

A task that does something hard to undo can ask first. Add `confirm` with a question:

```ok file=.ok/ok.ok
# Wipe the local database
task reset-db confirm "This wipes the local database" {
  print("database wiped")
}

# Wipe and reload the sample data
task fresh needs [reset-db] {
  print("sample data loaded")
}
```

On a terminal, `ok fresh` asks `This wipes the local database. Continue? [y/N]` before anything runs, including the dependencies that come before `reset-db`. ok collects the confirmations of every task in the invocation and asks all of them before the first task starts. `--yes` answers yes to each of them:

```output run="ok --yes fresh"
database wiped
sample data loaded
```

Without a terminal and without `--yes`, ok does not wait for input. It fails at once and names the option:

```output run="ok fresh" exit=1
error: cannot ask "This wipes the local database. Continue?": there is no terminal
  = note: task `reset-db` asks for confirmation
  = help: pass --yes before the script or task name to answer yes
```

## Hidden tasks

`hidden` keeps a task out of the listing and out of shell completion. It suits an internal step that other tasks need and people rarely run by hand. A hidden task can still be run by name, and `ok self tasks --all` lists it.

```ok
# Install dependencies when the lockfile changed
task deps hidden sources ["package-lock.json"] outputs ["node_modules/.package-lock.json"] {
  run npm ci
}
```

`needs`, `sources`, `outputs`, `confirm` and `hidden` are the modifiers of a task. They may come in any order, each at most once.

## Environment files

`env file ".env"` in the root file loads the settings of a `.env` file into the environment before any task runs:

```text file=.env
GREETING=hello
DATABASE_URL=postgres://localhost/dev
```

```text file=.env.staging
DATABASE_URL=postgres://staging.internal/app
```

```ok file=.ok/ok.ok
env file ".env"

# Show the settings
task show {
  print("{env.GREETING}: {env.DATABASE_URL}")
}
```

```output run="ok show"
hello: postgres://localhost/dev
```

`--env NAME` also loads `.env.NAME`, and its values override those of `.env`:

```output run="ok --env staging show"
hello: postgres://staging.internal/app
```

A variable that is already set in your environment wins over both files, so `DATABASE_URL=... ok show` uses the value you gave. A missing `.env` is ignored, but a file asked for with `--env` must exist. The format (quotes, comments, no variable expansion) is described in [section 13.7 of the spec](../SPEC.md#137-env).

### Env files and trust

A variable such as `PATH` or `NODE_OPTIONS` changes what every program does that a task starts. For that reason the env files that a run loads are part of what you [trust](trust.md#what-is-trusted). After an edit to `.env`, and on the first run with `--env staging`, ok asks for approval again. The review shows the variable names only, never the values.

An env file cannot set two kinds of variable:

- A variable whose name starts with `OK_`. These variables configure ok itself. `OK_TRUST`, for example, would switch off the trust check for nested runs.
- A variable whose name starts with `LD_` or `DYLD_`. These variables make the system load code into every program.

If you need one of these variables, set it in your own environment.

## Trying a task with `--dry-run`

`--dry-run` prints the commands that a task would run and the file changes it would make, and does neither. This shows what an unfamiliar task does before you run it:

```ok file=.ok/ok.ok
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

With `--dry-run`, tasks always run one at a time, so the printed plan has the same order on every run. `--trace` runs everything and prints each command before it starts.

## Shell completions

ok can complete task names, namespaces, flags and choice values when you press Tab. `ok self completions SHELL` prints the completion script for a shell. Load the script from the start-up file of your shell:

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

Completion reads the project files and never runs their code, so it works in a project you have not trusted.

## Walkthrough: replacing a Makefile

This walkthrough turns a small Makefile into a root file. The Makefile is for a Rust project that generates a source file before it builds:

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

Each part of the Makefile has a counterpart in ok:

- The file rule `src/version.rs: VERSION` becomes a task with `sources` and `outputs`. Its shell commands (`cat`, `echo`, a redirect) become `read` and `write`, which work the same on Windows.
- `build` and `release` differ only by a flag, so they become one task with a `--release` parameter.
- Each prerequisite becomes an entry in `needs`.
- `.PHONY` has no counterpart, because ok never takes a task for a file.
- `rm -rf` becomes `remove`, which needs no shell and does not fail when the file is already gone.
- The comments above the rules become descriptions, which `ok` shows in the listing.

```text file=VERSION
1.4.0
```

```ok file=.ok/ok.ok
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

`--dry-run` shows what `ok build --release` would do:

```output run="ok --dry-run build --release" browser=no
~ write src/version.rs
~ cargo build --release
```

Two things work differently from `make`:

- `make` with no target runs `all`. `ok` with no task lists the tasks, so a newcomer sees `gen`, `build`, `test` and `clean` with their descriptions.
- `make build RELEASE=1` sets a variable, and `make` ignores a misspelt variable name. `ok build --release` passes a flag that has help (`ok build --help`), and a misspelt flag is an error.

The [codegen-build example](../examples/codegen-build/) is a longer version of the same rewrite, and [docker-image](../examples/docker-image/) shows tasks with several parameters.

## Next steps

- [Trust](trust.md) explains the prompt that ok shows before it runs the tasks of a project.
- [Formatter, checker and wrappers](tools.md) covers `ok self fmt` and `ok self check`, and the `./ok` wrappers that let people without ok run your tasks.
