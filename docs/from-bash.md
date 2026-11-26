# Coming from bash and make

How common bash idioms and Makefile patterns look in ok, and the differences that matter: no shell, no quoting, no globbing, loud errors and tasks.

## The big difference: there is no shell

In bash, a command line is a string that the shell takes apart: it splits words on spaces, expands `$VAR`, `*.txt` and `~`, and interprets `&&`, `;` and `|`. Most of bash's pitfalls come from that step: a file name with a space becomes two arguments, an empty variable disappears, a `*` that matches nothing is passed on literally.

ok never runs a shell, on any OS. `run` starts the program directly with a list of arguments, and you can see exactly what that list is: every word you write is one argument, and every `{expression}` is exactly one argument, whatever it contains. There is nothing to quote for safety.

```ok file=commit.ok
let msg = "fix the build"
let files = ["src/main.rs", "my notes.txt"]

run git add -- {...files}
run git commit -m {msg}
run echo *.txt $HOME ~
```

`--dry-run` prints each command instead of running it, with quotes around arguments that contain spaces or special characters, so you can see the arguments the program would get:

```output run="ok --dry-run commit.ok" browser=no
~ git add -- src/main.rs "my notes.txt"
~ git commit -m "fix the build"
~ echo "*.txt" "$HOME" ~
```

- `{msg}` is one argument even though it contains spaces. In bash you would need `"$msg"`.
- `{...files}` spreads a list into one argument per element, so `my notes.txt` stays one file name. This is what bash arrays and `"${files[@]}"` are for.
- `*.txt`, `$HOME` and `~` are passed through literally. Use `glob("*.txt")`, `env.HOME` or `home` when you mean them.

Because there is no shell, the same script runs on Windows without Git Bash, WSL or Cygwin. Programs are looked up on `PATH` (and with `PATHEXT` on Windows, so `run npm ci` finds `npm.cmd`). [Running commands](commands.md) covers capturing output, pipes between programs, redirects, timeouts and the rest.

## Shell habits ok catches

When you write shell syntax in a command, ok refuses to start and tells you what to write instead. These are syntax errors, so nothing in the script has run yet:

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

The same goes for `;`, `||`, `&` and backticks. Outside commands, ok also points out habits from other languages, such as `&&` instead of `and`, `!x` instead of `not x` and `elif` instead of `else if`.

## Errors stop the script

A bash script carries on after a failed command unless you remember `set -euo pipefail`, and even then some failures slip through (a failure inside `$(...)` in a `local` declaration, for example). In ok, failing is the default:

- A command that exits with a nonzero code stops the script. The error shows the command, its exit code and the last lines of its stderr, and ok exits with the command's own exit code.
- A missing file, a missing record field, an unset environment variable or a value of the wrong type is an error too, never an empty string.
- `run!` runs a command without failing on a nonzero exit code, for commands whose exit code is an answer (like `grep -q` or `git diff --quiet`). The result record has `.ok` and `.code`.

```ok
let r = run! git diff --quiet
if not r.ok {
  print("you have uncommitted changes")
}
```

An unset variable shows how this differs from bash, where `$NOPE` quietly becomes an empty string:

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

```output run="TARGET=staging ok env.ok"
deploying to staging
```

The exit codes follow the usual conventions: 0 for success, 1 for an error in the script, 2 for wrong command-line usage, 127 for a program that is not found, and the program's own code when a command fails. [Errors](errors.md) explains `?`, `??` and `try` for the cases where you do want to carry on.

There is no truthiness either: `if` needs a `bool`. `if count` is an error that suggests `count > 0`, and `if files` suggests `files != []`, so an empty string or a zero can never be taken for "false" by accident.

## Bash idioms in ok

### Variables, arguments and the environment

| bash | ok |
|---|---|
| `name="world"` | `let name = "world"` (or `var` if you reassign it) |
| `echo "hello $name"` | `print("hello {name}")` |
| `echo "oops" >&2` | `eprint("oops")` |
| `$1`, `${1:-default}` | `fn main(name: string = "default")`, see [below](#arguments-and-help) |
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

Arithmetic needs no special syntax, but note that `/` on two integers gives a float: `7 / 2` is `3.5`. Use `floor(7 / 2)` for `3`.

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

`cd` in a bash script changes the directory for everything after it, which is easy to lose track of. ok has no `cd`: each command that needs another directory says so with `cwd`.

Output is captured or shown depending on where the `run` stands. A `run` on its own line shows the program's output live; a `run` whose value you use (in a `let`, as an argument, in `{...}`) captures it into `.out` and `.err`, with trailing newlines removed as `$(...)` does.

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

A pattern in `glob` that matches nothing gives an empty list, never the pattern itself. Matching is case-sensitive and the results are sorted the same way on every OS.

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

Paths print with `/` on every OS, including Windows. The `\{` and `\}` in the CSS text are escapes: in a double-quoted string, `{` starts an interpolation. Single-quoted strings have no escapes and no interpolation, which suits regular expressions and Windows paths. The file functions are printed by `--trace` and skipped by `--dry-run`, like commands. [Files and paths](files.md) has the full set.

### Text processing

The `grep`, `cut`, `sort`, `uniq` and `wc` pipelines of a shell script become pipelines of functions. They work on lines and records instead of bytes, and large files are streamed rather than read into memory.

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

Regular expressions use the syntax of Rust's `regex` crate, which is close to what `grep -E` and Perl accept, but without backreferences.

## .env files

Many projects keep settings in a `.env` file and load it with `source .env` or `set -a`. Sourcing runs the file as shell code, which is why a value with a space or a `$` behaves strangely. In ok, `.env` is a data format: `read(".env")` gives a record of strings and runs nothing.

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

The leading `export` is accepted so existing files work, quotes are removed, comments are ignored, and `${HOME}` stays as written: there is no variable expansion. In a project, `env file ".env"` in `ok.ok` loads the file into the environment of every task, as shown below. Variables that are already set in your environment win over the file, so `PORT=9000 ok serve` still works.

## Arguments and help

A bash script parses its arguments with `$1`, `shift` and `getopts`, and its `--help` is a hand-written `usage()` function that drifts out of date. In ok, `fn main` declares them:

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

```output run="ok greet.ok" exit=2
error: missing argument <name>
usage: ok greet.ok <name> [--greeting <value>] [--shout]
run with --help for details
```

`ok greet.ok --help` prints the description, the usage line and every parameter with its comment and default. A quick script that just wants its raw arguments can leave out `main` and use `args`, a list of strings, like `"$@"`. [Command-line scripts](script-cli.md) covers types, choices, short flags and rest parameters.

## From make to tasks

A Makefile is usually less about building files than about naming the project's commands: `make test`, `make deploy`. ok's tasks are made for that. They live in `ok.ok` at the root of the project and are written in the same language as scripts:

```text file=VERSION
1.4.0
```

```text file=.env
PORT=8080
```

```ok file=ok.ok
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

```output run="ok dist"
generated build/version.txt
building (release: true) on port 8080
packaging
```

```output run="ok build --relase" exit=2
error: unknown flag --relase
  = help: did you mean --release?
usage: ok build [--release]
run with --help for details
```

Running `ok` on its own in the project lists the tasks with the first line of their comments, which replaces the `help` target many Makefiles grow. Run `ok dist` a second time and `gen` prints `gen: up to date` instead of running, because its output is newer than its source.

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
| `$(MAKE) -C frontend build` | `import "frontend/ok.ok" as fe`, then `ok fe build` |
| `@echo` to hide the command | nothing: commands are not echoed; `ok --trace` shows them |
| `$(shell git rev-parse HEAD)` | `(run git rev-parse HEAD).out` inside the task |

The differences that matter:

- **Tasks are not files.** A make target is a file name, and a target whose file exists does nothing, which is why `.PHONY` exists. A task is a name. It is skipped only when it declares `outputs` and they are newer than its `sources`.
- **One recipe, one process.** Every line of a make recipe runs in its own shell, so `cd` and variables do not carry over to the next line. A task body is ordinary code: variables, loops, functions and error handling work as in a script.
- **Parameters, not variables.** Task parameters are declared like `main`'s, are checked, and appear in `ok build --help`. Dependencies can pass arguments: `needs [build(release: true)]`.
- **Project files run nothing when they load.** The top level of `ok.ok` holds only tasks, functions, constant `let`s and imports, so listing tasks, printing help and completing names never run project code. Compute values inside the task that needs them.
- **Tasks run in the project root**, wherever in the project you type `ok`. Relative path arguments are resolved against the directory you typed them in.
- **Trust.** The first time you run a task in a project you have not approved, ok shows you the project's files and asks. In CI, set `OK_TRUST=1`. See [Trust](trust.md).

[Tasks](tasks.md) covers namespaces (`ok db migrate`), mounted sub-projects, `confirm`, hidden tasks and parallel runs. The [docker-image](../examples/docker-image/), [codegen-build](../examples/codegen-build/) and [monorepo](../examples/monorepo/) examples are real Makefiles next to their `ok.ok` versions, and [log-summary](../examples/log-summary/), [pre-commit](../examples/pre-commit/) and [tidy-names](../examples/tidy-names/) do the same for bash scripts.
