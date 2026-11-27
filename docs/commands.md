# Running commands

Start programs with `run`, use their output, pipe and redirect them, set options such as a timeout, and see what a script would run.

## Running a program

`run` followed by a space starts a program. The rest of the line is the command, written much as you would type it in a terminal:

```ok
run cargo build --release
run git status
```

While the command runs, its output appears on your terminal as it is written. When it ends, the script carries on with the next line. If it fails, which means that it exits with a code other than 0, the script stops with an error. You do not need `set -e` or `&&` for that.

Most examples on this page start programs such as `git` and `cargo`, so they are shown without output. The playground in your browser cannot start programs. There, `run` reports that it needs the native `ok` binary. To see what a script would run without running it, use [`--dry-run`](#seeing-what-runs-trace-and-dry-run).

## No shell: each word is one argument

ok starts the program directly with a list of arguments. It never hands the line to `sh`, `bash`, `cmd.exe` or PowerShell. The same script therefore behaves the same on Linux, macOS and Windows, and a file name with a space in it cannot change what your command means.

ok splits a command into arguments by these rules:

- Words are separated by spaces or tabs. Each word is one argument.
- Quotes group a word. `"two words"` and `'two words'` are one argument each, and `""` is an empty argument. Double quotes allow `{...}` interpolation and single quotes are raw, as in ordinary ok strings.
- `*`, `?`, `~`, `$` and `[ ]` are ordinary characters. ok does not expand file patterns, `~` or variables, so `run ls *.txt` passes the text `*.txt` to `ls`. Use [`glob`](files.md#finding-files-with-glob) to find files.
- A backslash is an ordinary character too, so `C:\tools\x.exe` works as written.
- A `#` at the start of a word starts a comment. In the middle of a word, as in `http://host/#top`, it is literal.
- A `\` at the very end of a line continues the command on the next line.

Shell syntax that would mean something else in ok is an error that says what to write instead. This is the error for joining commands with `&&`:

```ok nocheck file=and.ok
run make && make install
```

```output exit=1
error[syntax]: `&&` is not allowed in a command
 --> and.ok:1:10
  |
1 | run make && make install
  |          ^^
  |
  = help: ok commands are not shell: put the next command on its own `run` line (a failure already stops the script)
```

The error for setting a variable in front of the command:

```ok nocheck file=env.ok
run FOO=1 make
```

```output exit=1
error[syntax]: `FOO=1` looks like a shell environment assignment
 --> env.ok:1:5
  |
1 | run FOO=1 make
  |     ^^^^^
  |
  = help: set environment variables with options: run cmd with {env: {FOO: "..."}}
```

The error for command substitution:

```ok nocheck file=subst.ok
run echo $(date)
```

```output exit=1
error[syntax]: `$(...)` is shell syntax
 --> subst.ok:1:10
  |
1 | run echo $(date)
  |          ^^
  |
  = help: capture the output first: let x = (run cmd).out, then use {x}
```

`;`, `||`, `&`, an unquoted `(` and backticks are refused the same way. To pass one of these characters to a program, quote it: `run grep ";" notes.txt`.

## Putting values into a command

`{expr}` inside a command puts the value of an ok expression into the command. The value is always exactly one argument, also when it contains spaces or is empty, so you do not need to quote it:

```ok file=tag.ok
let version = "1.4.0"
let message = "Release {version}, with fixes"
run git tag "v{version}" -m {message}
run cargo build --target={os.arch}-unknown-linux-musl
```

A word made of several parts, like `"v{version}"` or `--target={...}`, is still one argument. Strings pass as they are, numbers and bools as you would print them, and paths in the form the OS expects (with `\` on Windows).

A list is several arguments, so you spread it with `{...list}`. The spread must be a whole word. An empty list adds no arguments, so a list can hold flags that are only sometimes given. In this script, the parameters of `main` are the script's own command-line arguments, which [Command-line scripts](script-cli.md) explains:

```ok file=spread.ok
fn main(--release: bool, ...packages: string) {
  var flags = ["--locked"]
  if release { flags += ["--release"] }
  run cargo build {...flags} {...packages}
}
```

A list without the `...` is an error:

```ok file=list.ok
let files = ["a.txt", "b.txt"]
run cat {files}
```

```output exit=1 browser=no
error[type]: a list cannot be one command argument
 --> list.ok:2:10
  |
2 | run cat {files}
  |          ^^^^^
  |
  = help: spread it into several arguments with {...list}, or join it: {join(list, ",")}
```

`null`, records and functions cannot be arguments either. Convert them to a string first.

### Values that come from outside the script

ok makes each value one argument, but the program decides what that argument means. Most programs treat a word that starts with `-` as an option. Take a value that comes from outside your script, such as a command-line argument or a line of a file. In `run git clone {url}`, a `url` of `--upload-pack=touch /tmp/pwned` is an option for git, and git does what the option says although no shell is involved. Put `--` before such values, which most programs read as "only arguments from here on", or check the value first:

```ok file=clone.ok
fn main(url: string, dir: path = "checkout") {
  if url.starts_with("-") { fail("not a URL: {url}") }
  run git clone -- {url} {dir}
}
```

## Showing, capturing or piping the output

The place where you write `run` decides what happens to the output of the command. What the command prints has no influence on it.

| Where `run` stands | Output | Value |
|---|---|---|
| on its own line, value unused (also the last line of a block) | shown live on the terminal | a result record with `out` and `err` set to `null` |
| anywhere its value is used: `let r = run ...`, an argument, `(run ...).out`, `return` | captured | a result record |
| left of a `\|` | stdout flows into the pipeline, stderr is shown live | the text of stdout |

The result record has these fields:

| Field | Holds |
|---|---|
| `out`, `err` | the captured stdout and stderr, as strings |
| `code` | the exit code |
| `ok` | `true` when the code is 0 |
| `cmd` | the command as a list of strings |
| `duration` | how long it ran |

Captured output has its trailing newlines removed, as with `$(...)` in a shell, so you can compare it or put it in a string without trimming it:

```ok
let branch = (run git rev-parse --abbrev-ref HEAD).out
if branch != "main" { fail("releases are cut from main, not {branch}") }

let r = run cargo --version
print("using {r.out}")
```

Captured output must be UTF-8 text. For binary output, pass the [option](#command-options) `bytes: true`. Then `out` and `err` hold the raw bytes, unchanged.

When a command is piped, its stdout streams into the rest of the pipeline as text. Split it into lines to work with it:

```ok
let todo = run git grep -n TODO | lines | count
print("{todo} TODOs left")

run git log --oneline | lines | take(5) | each(print)
```

If the pipeline stops reading early, as `take(5)` does, ok closes the pipe and stops the command, and ignores its exit code. [Pipelines](pipelines.md) describes the stages you can use on the stream.

## When a command fails

A command that exits with a code other than 0 raises a `command` error. The script stops, and the error shows the command and, when stderr was captured, its last lines:

```
error[command]: command failed with exit code 101
  --> release.ok:22:3
   |
22 |   run cargo test --workspace
   |   ^^^^^^^^^^^^^^^^^^^^^^^^^^
   = command: cargo test --workspace
   = stderr (last 3 lines):
     test parser::tests::empty ... FAILED
     failures:
         parser::tests::empty
```

ok then exits with the command's own exit code, here 101, so whoever ran your script sees the same failure. A program that is not installed is a different error, with exit code 127:

```ok file=missing.ok
run definitely-not-installed --version
```

```output exit=127 browser=no
error[command]: command not found: definitely-not-installed (searched PATH)
 --> missing.ok:1:1
  |
1 | run definitely-not-installed --version
  | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = help: check that it is installed and on PATH
```

Some programs use the exit code to give an answer. `git diff --quiet`, for example, exits with 1 when there are changes. For such a program, use `run!`. It never raises an error for a nonzero exit code, and you read the code from the result record:

```ok
let diff = run! git diff --quiet
if not diff.ok {
  print("there are uncommitted changes (exit code {diff.code})")
}
```

`run!` still raises an error when the program cannot be found or started. To handle a failure in another way, catch the error with `try`. The error value has the fields `cmd`, `code` and `stderr`, as [Errors](errors.md#the-error-value) describes.

## Sending input to a command

A command reads from ok's own stdin unless you give it other input. You can pipe a value into the command, redirect a file with `<`, or set the `stdin` option:

```ok
"some text\n" | run tr a-z A-Z            # a string, bytes or text from a pipeline
["one", "two"] | run sort                 # a list of strings, one per line
run psql mydb < "schema.sql"              # a file
run wc -l with {stdin: "a\nb\n"}          # the stdin option
```

### Piping one command into another

`run a | run b` connects the stdout of `a` directly to the stdin of `b`. The data does not pass through ok:

```ok
run git log --format=%an | run sort | run uniq -c
```

A failure of any command in such a pipeline stops the script, as with `set -o pipefail` in bash. If `curl` fails in `run curl -fsSL {url} | run tar -xz`, the script stops with curl's error once tar has finished, also when tar itself succeeded. When several commands fail, the error is about the leftmost one, which is usually the cause.

A command that is ended because the next one stopped reading, as `yes` is in `run yes | run head -1`, is not a failure.

`run!` applies only to the command it is written on. `run! grep TODO {f} | run wc -l` counts the matching lines and does not fail when grep finds none.

## Redirecting to and from files

A redirect writes a stream of the command to a file, or reads stdin from a file. The target is one word, which can be a bare name, a quoted string or `{expr}`.

| Redirect | Effect |
|---|---|
| `> file` | write stdout to the file |
| `>> file` | append stdout to the file |
| `< file` | read stdin from the file |
| `2> file` | write stderr to the file |
| `2>> file` | append stderr to the file |
| `2>&1` | send stderr wherever stdout goes |

```ok
run pg_dump mydb > {backup_dir / "mydb.sql"}
run cargo build 2> "logs/build errors.txt"
let r = run make test 2>&1
```

Redirects differ from those of a shell in four ways:

- The directory of an output file is created if it is missing.
- `2>&1` means the same wherever you write it. `2>&1 > f` and `> f 2>&1` both send both streams to `f`. With captured output, `2>&1` puts both streams into `out`, interleaved as the command wrote them.
- Each stream takes at most one redirect. `> a > b` is an error.
- `2>` is only a redirect at the start of a word. `2 > x` passes the argument `2` and redirects stdout.

When stderr goes to a file and the command fails, the error says where to look: `stderr was written to build.log`.

## Command options

To set options for a command, write `with` and a record after its words:

```ok
run npm ci with {cwd: "frontend", timeout: 10m}
run cargo test with {env: {RUST_LOG: "debug", NO_COLOR: null}}
run cargo fetch with {quiet: true}
```

| Option | Type | Effect |
|---|---|---|
| `cwd` | path | working directory of the command, relative to the script's working directory (in a project, the project root) |
| `env` | record | environment variables to set for the command; `null` removes a variable |
| `stdin` | string, bytes or path | data, or a file, for stdin |
| `timeout` | duration | stop the command, and every process it started, after this long, with a `timeout` error (exit code 124 if not caught) |
| `bytes` | bool | captured `out` and `err` are raw bytes |
| `quiet` | bool | hide the output; if the command fails, the error still shows the end of stderr |

A `timeout` also stops the processes that the command started, such as the server that a test script launches in the background. It covers reading the output too, so a process that is still running cannot keep the script waiting. A command with a `timeout` runs in a process group of its own, so on Unix it cannot read from the terminal. Give it `stdin` if it asks questions.

Use `quiet` for a step that prints a lot and whose output you need only when the step fails.

The command inherits ok's environment, with the changes that `env` makes. Variables that the script set with `env.set` are passed on as well.

`with` is only special when a `{` follows it, so `run echo with care` passes `with` and `care` as arguments. A misspelled option, such as `cdw`, is an error that asks whether you meant `cwd`.

## Building a command from values with run(...)

`run(...)`, with the parenthesis right after `run`, is the function form. It takes ordinary expressions as arguments and options as named arguments. Use it when the whole command is computed:

```ok
let msg = "fix: handle empty input"
run("git", "commit", "-m", msg)
run("npm", "ci", cwd: "frontend", timeout: 10m)
let found = run!("git", "rev-parse", "--verify", "--quiet", "v1.0.0")
```

Lists are spread with `...`, as in any call. A single string is never split into words, because that would bring back the quoting problems of a shell:

```ok file=one-string.ok
run("make test")
```

```output exit=1 browser=no
error[command]: no program named "make test": ok never splits a string into words
 --> one-string.ok:1:1
  |
1 | run("make test")
  | ^^^^^^^^^^^^^^^^
  |
  = help: pass each word separately: run("make", "test") or run make test
```

The function form has no redirects. For a computed command that needs one, write a `run` line and spread the list of words: `run {...argv} > "out.txt"`.

## Seeing what runs: trace and dry run

`ok --trace` prints each command to stderr before it starts, prefixed with `+`:

```
+ cargo build --release --target x86_64-unknown-linux-musl
+ (in frontend) npm ci
+ make test < input.txt > build.log 2>&1
```

In these lines:

- An argument with spaces or quotes is quoted.
- A control character in an argument is shown as an escape, such as `\r` or `\u{1b}`.
- The password of a URL is shown as `***`.
- The working directory is shown when it is not the default.
- Redirects come last.

`ok --dry-run` prints the same lines with `~` and runs nothing. Calls that change the file system, such as `write`, `remove` and `mkdir`, are skipped and printed too. A call is printed also when it would have nothing to do, such as a `remove` of a file that does not exist. Reads still happen, so that the script can make its decisions.

Both options go before the script name:

```ok file=deploy.ok
fn main(--version: string = "1.4.0") {
  let flags = ["--release", "--locked"]
  run cargo build {...flags}
  run npm ci with {cwd: "frontend"}
  run git tag "v{version}" -m "Release {version}"
  mkdir("dist")
  let sha = (run git rev-parse HEAD).out
  print("tagged v{version} at [{sha}]")
}
```

```output run="ok --dry-run deploy.ok --version 2.0.0" browser=no
~ cargo build --release --locked
~ (in frontend) npm ci
~ git tag v2.0.0 -m "Release 2.0.0"
~ mkdir dist
~ git rev-parse HEAD
tagged v2.0.0 at []
```

A skipped command returns a result record of a success with empty output: `out` and `err` are `""`, and `code` is 0. That is why `sha` is empty in the last line. A dry run therefore does not show every branch as a real run takes it. A decision that depends on the output of a command may go another way in a real run.

## How ok finds programs, and Windows command lines

ok finds programs the same way on every OS, with one addition for Windows:

- A name with a path separator, like `./tools/gen` or `bin/check`, is relative to the command's working directory.
- Any other name is searched for in the directories of `PATH`, in order. The current directory is never searched, on any OS, so to run a program next to your script write `./name`.
- On Windows, a name without an extension also tries each extension in `PATHEXT` (by default `.COM`, `.EXE`, `.BAT` and `.CMD`). That is how `run npm install` finds `npm.cmd`.

Windows passes a program one command line, not a list of arguments. ok therefore quotes each argument by the rules that Windows programs use to split the command line again. Batch files (`.bat`, `.cmd`) have stricter rules of their own. An argument that cannot be passed to a batch file safely, such as one with a newline in it, is refused with an error.

## Where to go next

- [Command-line scripts](script-cli.md) shows how to give a script that runs commands its own flags and `--help`.
- [Tasks](tasks.md) shows how to run commands as project tasks with dependencies.
- The spec defines [commands](../SPEC.md#9-commands-run) exactly, including the [rules for splitting a command into words](../SPEC.md#92-command-mode-lexing).
