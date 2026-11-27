# Coming from bash and make

This page shows how common bash idioms and Makefile patterns are written in ok, and where ok behaves differently. ok runs commands without a shell, so there is no quoting and no globbing in a command. Errors stop the script, and tasks take the place of make targets. The page assumes that you know bash or make. To learn ok from the start, read [Your first script](first-script.md).

## ok runs commands without a shell

In bash, a command line is a string that the shell takes apart. The shell splits the string into words on spaces, expands `$VAR`, `*.txt` and `~`, and interprets `&&`, `;` and `|`. Most of the common bash mistakes come from that step. A file name with a space becomes two arguments, an empty variable disappears, and a `*` that matches nothing is passed on literally.

ok never runs a shell, on any OS. `run` starts the program directly with a list of arguments. Every word that you write is one argument, and every `{expression}` is exactly one argument, whatever it contains. You never need quotes to keep an argument together.

```ok file=commit.ok
let msg = "fix the build"
let files = ["src/main.rs", "my notes.txt"]

run git add -- {...files}
run git commit -m {msg}
run echo *.txt $HOME ~
```

To see the arguments that each program would get, run the script with `--dry-run`. ok then prints each command and does not run it. It puts quotes around an argument that contains spaces or special characters:

```output run="ok --dry-run commit.ok" browser=no
~ git add -- src/main.rs "my notes.txt"
~ git commit -m "fix the build"
~ echo "*.txt" "$HOME" ~
```

- `{msg}` is one argument although it contains spaces. In bash you need `"$msg"` for that.
- `{...files}` spreads a list into one argument per element, so `my notes.txt` stays one file name. In bash you need an array and `"${files[@]}"` for that.
- `*.txt`, `$HOME` and `~` are passed on literally. Write `glob("*.txt")`, `env.HOME` or `home` when you mean the files, the variable or the home directory.

Because ok uses no shell, the same script runs on Windows without Git Bash, WSL or Cygwin. ok looks up a program on `PATH`. On Windows it also tries the extensions in `PATHEXT`, so `run npm ci` finds `npm.cmd`. [Running commands](commands.md) covers what this page only touches: captured output, pipes between programs, redirects and options such as `timeout`.

## Shell syntax that ok rejects

When a command contains shell syntax, ok does not start the script and tells you what to write instead. These are syntax errors, so no line of the script has run when you see them. `&&` between two commands:

```ok file=build.ok nocheck
run make && make test
```

```output exit=1
error[syntax]: `&&` is not allowed in a command
 --> build.ok:1:10
  |
1 | run make && make test
  |          ^^
  |
  = help: ok commands are not shell: put the next command on its own `run` line (a failure already stops the script)
```

An environment assignment in front of a command:

```ok file=build.ok nocheck
run FOO=1 make
```

```output exit=1
error[syntax]: `FOO=1` looks like a shell environment assignment
 --> build.ok:1:5
  |
1 | run FOO=1 make
  |     ^^^^^
  |
  = help: set environment variables with options: run cmd with {env: {FOO: "..."}}
```

A command substitution:

```ok file=build.ok nocheck
let today = run echo $(date)
```

```output exit=1
error[syntax]: `$(...)` is shell syntax
 --> build.ok:1:22
  |
1 | let today = run echo $(date)
  |                      ^^
  |
  = help: capture the output first: let x = (run cmd).out, then use {x}
```

ok rejects `;`, `||`, `&` and backticks in a command in the same way. Outside commands, ok also recognises syntax from other languages and names the ok form: `and` for `&&`, `not x` for `!x` and `else if` for `elif`.

## Errors stop the script

A bash script continues after a failed command unless you remember `set -euo pipefail`. Even then bash ignores some failures, for example a failure inside `$(...)` in a `local` declaration. In ok, a failure stops the script unless you say otherwise:

- A command that exits with a nonzero code stops the script. The error shows the command, its exit code and the last lines of its stderr, and ok exits with the command's own exit code.
- A missing file, a missing record field, an unset environment variable or a value of the wrong type is an error too. None of them becomes an empty string.

For a command whose exit code is an answer, such as `grep -q` or `git diff --quiet`, use `run!`. It does not fail on a nonzero exit code, and its result record has the fields `.ok` and `.code`:

```ok
let r = run! git diff --quiet
if not r.ok {
  print("you have uncommitted changes")
}
```

In bash, an unset variable such as `$TARGET` becomes an empty string. In ok, it is an error that names the variable:

```ok file=env.ok
print("deploying to {env.TARGET}")
```

```output exit=1
error[key]: the environment variable TARGET is not set
 --> env.ok:1:26
  |
1 | print("deploying to {env.TARGET}")
  |                          ^^^^^^
  |
  = help: give a default with env.get("TARGET", "..."), or test it with env.has("TARGET")
```

With the variable set, the script runs:

```output run="TARGET=staging ok env.ok"
deploying to staging
```

ok has no truthiness either. `if` needs a `bool`, so an empty string or a zero is never taken for "false". `if count` is an error that suggests `count > 0`, and `if files` suggests `files != []`.

The exit codes follow the usual conventions. ok exits with 0 for success, 1 for an error in the script, 2 for wrong command-line usage, 127 for a program that is not found, and the program's own code when a command fails. [Errors](errors.md) explains `?`, `??` and `try`, for the places where a script should continue after an error.

## Bash idioms in ok

### Variables, arguments and the environment

| bash | ok |
|---|---|
| `name="world"` | `let name = "world"` (or `var` if you reassign it) |
| `echo "hello $name"` | `print("hello {name}")` |
| `echo "oops" >&2` | `eprint("oops")` |
| `$1`, `${1:-default}` | `fn main(name: string = "default")`, see [Arguments and help](#arguments-and-help) |
| `"$@"` | a rest parameter: `fn main(...files: path)` |
| `$#` | `len(args)` in a script without `main` |
| `$HOME`, `${PORT:-8080}` | `env.HOME`, `env.get("PORT", "8080")` |
| `[ -z "${TOKEN:-}" ]` | `not env.has("TOKEN")` |
| `export STAGE=test` | `env.set("STAGE", "test")` |
| `FOO=1 make` | `run make with {env: {FOO: "1"}}` |
| `$(( a + b * 2 ))` | `a + b * 2` |
| `dirname "$0"` | `script_dir` |
| `uname -s` | `os.name` (`"linux"`, `"macos"`, `"windows"`) |
| `command -v docker` | `which("docker")` (a path, or `null`) |

Arithmetic needs no special syntax. Unlike `$(( 7 / 2 ))`, `/` on two integers gives a float, so `7 / 2` is `3.5`. Use `floor(7 / 2)` for `3`.

### Running programs

| bash | ok |
|---|---|
| `make test` | `run make test` |
| `out=$(git rev-parse HEAD)` | `let out = (run git rev-parse HEAD).out` |
| `if grep -q TODO notes.txt; then` | `if (run! grep -q TODO notes.txt).ok {` |
| `cmd \|\| true` | `run! cmd` |
| `echo $?` | `let r = run! cmd` then `r.code` |
| `cd frontend && npm ci` | `run npm ci with {cwd: "frontend"}` |
| `cmd > build.log 2>&1` | `run cmd > build.log 2>&1` |
| `git log \| head -n 5` | `run git log \| lines \| take 5` |
| `ls \| grep x` | `run ls \| run grep x` |
| `timeout 60 ./slow.sh` | `run ./slow.sh with {timeout: 60s}` |
| `cmd >/dev/null 2>&1` | `run cmd with {quiet: true}` |
| `set -x` | `ok --trace script.ok` |

`cd` in a bash script changes the directory for every command after it. ok has no `cd`. Each command that needs another directory names the directory with `cwd`.

Where a `run` stands decides whether its output is shown or captured. A `run` on its own line shows the program's output while the program runs. A `run` whose value you use (in a `let`, as an argument, in `{...}`) captures the output in `.out` and `.err`, and removes trailing newlines as `$(...)` does.

### Files and directories

| bash | ok |
|---|---|
| `cat config.json` | `read("config.json")` (parsed, see [Data formats](data.md)) |
| `echo "$text" > out.txt` | `write(text, "out.txt")` |
| `echo "$line" >> log.txt` | `write("{line}\n", "log.txt", append: true)` |
| `[ -f x ]`, `[ -d x ]`, `[ -e x ]` | `is_file("x")`, `is_dir("x")`, `exists("x")` |
| `mkdir -p build/assets` | `mkdir("build/assets")` |
| `cp -r src dst` | `copy("src", "dst")` |
| `mv old new` | `move("old", "new")` |
| `rm -rf build` | `remove("build")` |
| `touch stamp` | `touch("stamp")` |
| `for f in *.txt` | `for f in glob("*.txt")` |
| `find . -name '*.rs'` | `glob("**/*.rs")` |
| `basename "$f"`, `basename "$f" .txt`, `${f##*.}` | `f.name`, `f.stem`, `f.ext` (for a path `f`) |
| `tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT` | `let tmp = tempdir()` (removed when the script exits) |
| `sha256sum file` | `hash(path("file"))` |
| `tar czf out.tar.gz dir` | `tar("dir", "out.tar.gz")` |
| `curl -fsSL "$url"` | `fetch(url).body` |

A `glob` pattern that matches nothing gives an empty list, where bash passes on the pattern itself. Matching is case-sensitive, and the results are sorted the same way on every OS. This script uses several of the file functions:

```ok file=files.ok
mkdir("build/assets")
write("h1 \{ color: navy \}\n", "src/site.css")
write("<h1>hi</h1>\n", "src/index.html")
copy("src", "build/site")
for f in glob("build/**/*.css") {
  print(f, stat(f).size)
}
print(is_dir("build"), exists("nope.txt"))
print(remove("build"))
```

```output
build/site/site.css 19
true false
true
```

Paths print with `/` on every OS, including Windows. Like commands, the file functions are printed by `--trace` and skipped by `--dry-run`. [Files and paths](files.md) describes all of them.

The `\{` and `\}` in the CSS text are escapes, because `{` starts an interpolation in a double-quoted string. A single-quoted string has no escapes and no interpolation, which suits regular expressions and Windows paths.

### Text processing

The `grep`, `cut`, `sort`, `uniq` and `wc` pipelines of a shell script become pipelines of functions in ok. The functions work on lines and records, where the shell tools work on bytes. A large file is streamed through the pipeline and is not read into memory as a whole. In this script, the comment above each pipeline is the shell command that it replaces:

```text file=access.log
GET /index.html 200
GET /about.html 200
POST /login 401
GET /index.html 200
GET /missing 404
POST /login 200
GET /index.html 304
```

```ok file=text.ok
let log = read("access.log")

# wc -l < access.log
print(log | lines | count)

# grep -c POST access.log
print(log | lines | count .contains("POST"))

# grep ' 404$' access.log
log | lines | where .ends_with(" 404") | each(print)

# cut -d' ' -f2 access.log | sort | uniq -c | sort -rn | head -n 2
log
  | lines
  | map(l => l.split(" ")[1])
  | group(p => p)
  | entries
  | sort(e => len(e.value), desc: true)
  | take 2
  | each(e => print("{len(e.value):>4} {e.key}"))

# echo /about.html | sed 's/\.html$//' | tr a-z A-Z
print("/about.html".sub('\.html$', "").upper())
```

```output
7
2
GET /missing 404
   3 /index.html
   2 /login
/ABOUT
```

The table lists more text idioms:

| bash | ok |
|---|---|
| `grep ERROR app.log` | `read("app.log") \| lines \| where .contains("ERROR")` |
| `grep -E '^v[0-9]+' f` | `... \| where .matches('^v\d+')` |
| `head -n 5`, `tail -n +6` | `take 5`, `skip 5` |
| `sort -u` | `sort \| uniq` |
| `cut -d, -f2` | `.split(",")[1]` (for real CSV, `read("x.csv")`) |
| `sed 's/old/new/g'` | `.replace("old", "new")`, or `.sub(regex, "new")` |
| `tr a-z A-Z` | `.upper()` |
| `printf '%-10s %5d\n' "$n" "$c"` | `print("{n:<10} {c:>5}")` |
| `jq -r .version package.json` | `read("package.json").version` |
| `while read -r line; do ...; done < f` | `for line in read("f") \| lines { ... }` |
| `case "$x" in a\|b) ...;; esac` | `match x { "a" \| "b" => ... }` |
| `read -p "Name? " name` | `let name = prompt("Name? ")` |
| `sleep 5` | `sleep(5s)` |
| `date +%Y-%m-%d` | `now().format("%Y-%m-%d")` |

Regular expressions use the syntax of Rust's `regex` crate. It is close to what `grep -E` and Perl accept, but it has no backreferences.

## `.env` files are data

In ok, `.env` is a data format. `read(".env")` gives a record of strings and runs nothing. Many bash projects load a `.env` file with `source .env` or `set -a`, which runs the file as shell code, so a value with a space or a `$` in it changes meaning.

```text file=.env
# local settings
export DATABASE_URL=postgres://localhost/app
GREETING="hello world"
CACHE_DIR=${HOME}/cache   # not expanded
```

```ok file=dotenv.ok
let cfg = read(".env")
print(cfg.DATABASE_URL)
print(cfg.GREETING)
print(cfg.CACHE_DIR)
```

```output
postgres://localhost/app
hello world
${HOME}/cache
```

ok accepts the leading `export`, so existing files work. It removes the quotes and ignores the comments. It does not expand variables, so `${HOME}` stays as written.

In a project, `env file ".env"` in the root file loads the file into the environment of every task, as [From make to tasks](#from-make-to-tasks) shows. A variable that is already set in your environment wins over the file, so `PORT=9000 ok serve` still works.

## Arguments and help

In ok, the parameters of `fn main` declare a script's arguments, and ok checks the arguments and writes `--help` from them. A bash script parses its arguments with `$1`, `shift` and `getopts`, and its `--help` is a hand-written `usage()` function that someone must keep up to date.

```ok file=greet.ok
# Greet someone.
fn main(
  name: string,                  # who to greet
  --greeting: string = "hello",  # the word to use
  --shout: bool,                 # print in capitals
) {
  let line = "{greeting}, {name}"
  print(if shout { line.upper() } else { line })
}
```

```output run="ok greet.ok Anna --shout"
HELLO, ANNA
```

Without the required argument, ok prints an error and the usage line, and `main` does not run:

```output run="ok greet.ok" exit=2
error: missing argument <name>
usage: ok greet.ok <name> [--greeting <value>] [--shout]
run with --help for details
```

`ok greet.ok --help` prints the description, the usage line and every parameter with its comment and default. A script that needs only its raw arguments can leave out `main` and use `args`, a list of strings like `"$@"`. [Command-line scripts](script-cli.md) covers types, choices, short flags and rest parameters.

## From make to tasks

Most Makefiles name a project's commands, such as `make test` and `make deploy`, more than they build files. ok's tasks do that job. You declare tasks in the project's root file, which is `.ok/ok.ok` or a single file named `.ok` in the project root, and you write them in the same language as scripts. This project has three tasks:

```text file=VERSION
1.4.0
```

```text file=.env
PORT=8080
```

```ok file=.ok/ok.ok
env file ".env"

# Write the version file
task gen sources ["VERSION"] outputs ["build/version.txt"] {
  write("version {read("VERSION").trim()}\n", "build/version.txt")
  print("generated build/version.txt")
}

# Build the site
task build(--release: bool) needs [gen] {
  print("building (release: {release}) on port {env.PORT}")
}

# Build and package
task dist needs [build(release: true)] {
  print("packaging")
}
```

`ok dist` runs `gen` and `build` first, because `dist` needs `build` and `build` needs `gen`:

```output run="ok dist"
generated build/version.txt
building (release: true) on port 8080
packaging
```

ok checks a task's arguments as it checks a script's:

```output run="ok build --relase" exit=2
error: unknown flag --relase
  = help: did you mean --release?
usage: ok build [--release]
run with --help for details
```

`ok` without arguments lists the project's tasks with the first line of their comments, so the project needs no hand-written `help` target. When you run `ok dist` a second time, `gen` does not run and prints `gen: up to date`, because its output is newer than its source.

The table maps make to ok:

| make | ok |
|---|---|
| `test:` with a recipe | `task test { ... }` |
| `deploy: build test` | `task deploy needs [build, test] { ... }` |
| `.PHONY: test` | nothing: a task always runs unless it declares `outputs` |
| `out.txt: in.txt` | `task gen sources ["in.txt"] outputs ["out.txt"] { ... }` |
| `make VERSION=1.2 release` | `ok release --version 1.2`, with `task release(--version: string)` |
| `PORT ?= 8080` | `task serve(--port: int = 8080)`, or `env.get("PORT", "8080")` |
| `include .env` | `env file ".env"` |
| `make -n deploy` | `ok --dry-run deploy` |
| `make -j4` | `ok -j4 build` (independent dependencies already run in parallel) |
| `make -B` | `ok --force build` |
| `make help` (hand-written) | `ok` |
| `$(MAKE) -C frontend build` | `import "../frontend/ok.ok" as fe`, then `ok fe build` |
| `@echo` to hide the command | nothing: commands are not echoed; `ok --trace` shows them |
| `$(shell git rev-parse HEAD)` | `(run git rev-parse HEAD).out` inside the task |

### How tasks differ from make targets

- A task is a name, not a file. A make target is a file name, and a target whose file exists does nothing, which is why make has `.PHONY`. ok skips a task only when the task declares `outputs` and they are newer than its `sources`.
- A task body is one piece of ordinary code. Every line of a make recipe runs in its own shell, so `cd` and variables do not carry over to the next line. In a task body, variables, loops, functions and error handling work as they do in a script.
- A task takes parameters where make takes variables. You declare task parameters as you declare the parameters of `main`. ok checks them and lists them in `ok build --help`. A dependency can pass arguments, as in `needs [build(release: true)]`.
- Loading a project file runs nothing. The top level of a project file holds only tasks, functions, constant `let`s and imports, so ok can list tasks, print help and complete names without running project code. Compute a value inside the task that needs it, where a Makefile would use `$(shell ...)` at the top.
- A task runs in the project root, wherever in the project you type `ok`. A relative path that you pass as an argument is resolved against the directory where you typed it.
- ok asks before it runs a project's tasks for the first time. It shows you the project's files and asks whether you trust them. In CI, set `OK_TRUST=1`. See [Trust](trust.md).

[Tasks](tasks.md) covers namespaces (`ok db migrate`), mounted sub-projects, `confirm`, hidden tasks and parallel runs.

The [docker-image](../examples/docker-image/), [codegen-build](../examples/codegen-build/) and [monorepo](../examples/monorepo/) examples are real Makefiles next to their ok versions. The [log-summary](../examples/log-summary/), [pre-commit](../examples/pre-commit/) and [tidy-names](../examples/tidy-names/) examples are bash scripts next to theirs.
