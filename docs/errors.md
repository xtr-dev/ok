# Errors

Read ok's error messages, catch errors with `try`, `catch`, `finally` and `?`, raise your own with `fail`, and choose an exit code.

## Reading an error message

When a script fails, ok stops and prints an error message that says what happened and where. When ok knows the fix, the message also says how to fix the problem. This script has a typo in a variable name:

```ok file=typo.ok
let release = true
if relase {
  print("building a release")
}
```

```output exit=1
error[name]: unknown name `relase`
 --> typo.ok:2:4
  |
2 | if relase {
  |    ^^^^^^ not defined here
  |
  = help: did you mean `release`?
```

Every error has the same parts:

- `error[name]` is the kind of error, here a name that is not defined. The kinds are listed [below](#error-kinds).
- After the colon comes the message.
- `--> typo.ok:2:4` is the location: file, line and column.
- The snippet shows the line with the problem underlined.
- Lines that start with `=` add notes, a call trace and a help line. ok adds the help line when it knows the fix, so read that line first.

A misspelt name, field, flag or task gets a "did you mean" suggestion. An operator or keyword from another language or from the shell gets a help line with the ok form:

```ok nocheck file=habits.ok
let ready = true && false
```

```output exit=1
error[syntax]: `&&` is not an ok operator
 --> habits.ok:1:18
  |
1 | let ready = true && false
  |                  ^^
  |
  = help: use `and`, as in `a and b`
```

On a terminal the message is in colour. `NO_COLOR=1` or `--color never` turns colour off.

## Errors inside functions

When an error happens inside a function, the message ends with a call trace, innermost call first, which shows how the script reached that line:

```ok file=trace.ok
fn check_port(port: int) {
  if port < 1 or port > 65535 {
    fail("port {port} is out of range (1-65535)")
  }
  port
}

fn start(port) {
  print("starting on {check_port(port)}")
}

start(8080)
start(70000)
```

```output exit=1
starting on 8080
error[fail]: port 70000 is out of range (1-65535)
 --> trace.ok:3:5
  |
3 |     fail("port {port} is out of range (1-65535)")
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = in `check_port`, called at trace.ok:9:23
  = in `start`, called at trace.ok:13:1
```

The trace shows at most eight frames. Recursion that repeats the same frame shows it once, marked `(repeated)`.

## Error kinds

The kind says what sort of problem the error is. It is also the value you test when you catch an error (see [The error value](#the-error-value)). ok has these kinds:

| Kind | Raised for |
|---|---|
| `syntax` | code that does not parse, or a declaration in the wrong place (for example code at the top level of a library) |
| `name` | an unknown variable or function, an assignment to a `let`, an import clash or cycle |
| `type` | a value of the wrong type, such as `1 + "a"` or a non-bool `if` condition |
| `value` | a value of the right type that is not acceptable, such as `int("abc")` or division by zero |
| `index` | a list index out of range |
| `key` | a record field that does not exist |
| `arg` | a wrong argument to a function, such as `exit(300)` |
| `io` | a file that is missing or cannot be read or written |
| `format` | data that is not valid JSON, CSV, TOML, YAML and so on |
| `command` | a program that failed, could not be found or could not start (see [Running commands](commands.md)) |
| `timeout` | a command that ran longer than its `timeout` option |
| `net` | a `fetch` that failed |
| `fail` | your own error, raised with `fail` |
| `trust` | a project that has not been trusted (see [Trust](trust.md)) |

Syntax errors and load errors (a bad import, a library that runs code) are found before any of the script runs. Every other error happens while the script runs. `ok self check` finds many of the runtime errors, such as unknown names and wrong argument counts, without running anything. See [Tools](tools.md).

## Catching errors with try and catch

`try` runs a block. If an error is raised inside it, the `catch` block runs instead, with the error bound to the name after `catch`. `try` is an expression: its value is the value of whichever block finished.

```ok file=catch.ok
let n = try {
  int("forty-two")
} catch e {
  print("not a number: {e.message}")
  0
}
print(n)
```

```output
not a number: cannot convert "forty-two" to an int
0
```

The name after `catch` is optional. Leave it out when you do not need the error:

```ok file=catch-short.ok
let n = try { int("x") } catch { -1 }
print(n)
```

```output
-1
```

`catch` catches every kind of error. To handle only some kinds, test `e.kind` and raise the other errors again with `fail(e)`. `fail(e)` raises the error unchanged, with its original kind and location:

```ok file=rethrow.ok
fn load_settings(file) {
  try {
    read(file)
  } catch e {
    if e.kind != "io" { fail(e) }
    eprint("{file} not found, using defaults")
    {port: 8080, debug: false}
  }
}

let settings = load_settings("settings.toml")
print(settings.port)
```

```output
settings.toml not found, using defaults
8080
```

A missing file gives the defaults. A file with a syntax error in it still stops the script, because that error has the kind `format` and is raised again.

## The error value

A caught error is an ordinary record. Print it to see what it holds:

```ok file=record.ok
let e = try { read("missing.txt") } catch e { e }
print(e)
print(e.kind)
```

```output
{kind: "io", message: "cannot read missing.txt: no such file", file: "record.ok", line: 1, column: 15}
io
```

| Field | Holds |
|---|---|
| `kind` | the kind, as a string (see [Error kinds](#error-kinds)) |
| `message` | the message, without the location |
| `file`, `line`, `column` | where the error was raised, when it has a location |
| `cmd`, `code`, `stderr` | for `command` errors: the command as a list, its exit code, and the end of its stderr when it was captured |
| `status` | for `net` errors with an HTTP response: the status code |

Read a field that only some errors have with `?`, for example `e.code? ?? 1`.

## Defaults with ? and ??

The catch operator `?` turns an error into `null`. Put `?` at the end of an expression: if anything in the expression raises an error, the result is `null`. Combine `?` with `??`, which replaces `null` with a default, when you need a fallback value and do not need to look at the error:

```ok file=question.ok
let cfg = read("config.toml")? ?? {}
let port = cfg.port? ?? 8080
let retries = int("three")? ?? 3
print(port, retries)
```

```output
8080 3
```

`?` covers the whole chain to its left: in `cfg.server.port?` a missing `server` and a missing `port` both give `null`. ok has no optional chaining, so `cfg?.port` is an error whose help line says where to put the `?`. `?` does not catch `exit()`.

Use `?` when any failure means the same thing ("not there"), and `try`/`catch` when you need to tell failures apart or report them.

## Cleaning up with finally

A `finally` block runs after the `try` block (and after `catch`, if there is one), however the block ends: with a value, an error, `return`, `break`, `continue` or `exit()`. Use it for cleanup that must happen even when something fails:

```ok file=finally.ok
let work = path("build-tmp")
mkdir(work)
try {
  write("half done", work / "state.txt")
  fail("the build broke")
} finally {
  remove(work)
  print("cleaned up: {exists(work)}")
}
```

```output exit=1
cleaned up: false
error[fail]: the build broke
 --> finally.ok:5:3
  |
5 |   fail("the build broke")
  |   ^^^^^^^^^^^^^^^^^^^^^^^
```

A `try` without `catch` catches nothing. The error continues after the `finally` block has run. When a `try` has both, write `catch` first and `finally` last.

If the `finally` block itself fails, its error replaces the original one, and a note keeps the original message so it is not lost:

```ok file=finally-error.ok
try {
  fail("the original failure")
} finally {
  fail("cleanup failed")
}
```

```output exit=1
error[fail]: cleanup failed
 --> finally-error.ok:4:3
  |
4 |   fail("cleanup failed")
  |   ^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: this error in a finally block replaced the error: the original failure
```

`return`, `break` and `continue` directly inside a `finally` block are syntax errors, because they would throw away the error or value that is on its way out. Ctrl-C ends ok at once, without running `finally` blocks.

## Raising your own errors

`fail(message)` raises an error of kind `fail` at the place where you call it. Use it when your script finds a problem that it cannot continue with. ok shows the message, location and call trace as for any other error, and the script exits with code 1.

```ok file=fail.ok
fn main(--env: string = "dev") {
  let known = ["dev", "staging", "production"]
  if env not in known {
    fail("unknown environment {env}; expected one of {join(known, ", ")}")
  }
  print("deploying to {env}")
}
```

```output run="ok fail.ok --env qa" exit=1
error[fail]: unknown environment qa; expected one of dev, staging, production
 --> fail.ok:4:5
  |
4 |     fail("unknown environment {env}; expected one of {join(known, ", ")}")
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = in `main`, started by ok
```

For a fixed set of values like this one, a choice type does the check for you before the script starts. See [Command-line scripts](script-cli.md#choices).

A caught error passed to `fail` is raised again as it was, with its own kind and location, as shown in [Catching errors](#catching-errors-with-try-and-catch).

## Exit codes

When a script ends normally, ok exits with code 0. When an error is not caught, ok prints it to stderr and exits with a code that tells the caller what kind of failure it was:

| Code | Meaning |
|---|---|
| 0 | success |
| 1 | an error in the script: runtime, syntax or load error, refused trust or confirmation |
| 2 | bad command-line usage: unknown task or flag, missing or malformed argument |
| N | a command run by the script failed with exit code N, and the error was not caught |
| 124 | a command timed out |
| 126 | a program was found but could not be started |
| 127 | a program was not found |
| 128+S | on Unix, a command was killed by signal S |
| 130 | interrupted with Ctrl-C |

Because ok passes on the exit code of a failed command, `ok test.ok` in CI fails with the same code as the tool that the script ran. The full table is in the [spec](../SPEC.md#23-exit-codes).

## Ending a script with exit

`exit(code)` ends the script at once with the given code, from 0 to 255. Without an argument it exits with 0. Use it when the result of a script is a yes or no that other programs read from the exit code:

```ok file=exit.ok
let missing = ["README.md", "LICENSE"] | where(f => not exists(f))
if missing != [] {
  eprint("missing: {join(missing, ", ")}")
  exit(3)
}
print("all files present")
```

```output exit=3
missing: README.md, LICENSE
```

`exit` is not an error: `catch` and `?` do not stop it. `finally` blocks still run on the way out, and ok removes its temporary files. An out-of-range code is an error:

```ok file=exit-range.ok
exit(256)
```

```output exit=1
error[arg]: exit codes go from 0 to 255, got 256
 --> exit-range.ok:1:1
  |
1 | exit(256)
  | ^^^^^^^^^
```

## Where to go next

- [Running commands](commands.md) shows what a failing command's error looks like and how `run!` lets you handle exit codes yourself.
- [Command-line scripts](script-cli.md) covers the argument errors that end with exit code 2.
- The spec has the exact rules: [errors](../SPEC.md#10-errors), [`try`/`catch`](../SPEC.md#67-try--catch) and [the `?` operator](../SPEC.md#62-catch-operator-).
