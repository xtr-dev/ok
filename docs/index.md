# Introduction

ok is a scripting language and task runner in one static binary. This guide shows how to install it, write scripts and run a project's tasks.

## What ok is for

Most projects collect small programs around the real code: a bash script that prepares a release, a Python script that converts an export, a Makefile that knows how to build and test. Each one is written in a different language, and each one breaks somewhere: the bash script on macOS or on a file name with a space, the Python script on a machine with the wrong version, the Makefile on Windows.

ok is meant to replace all three with one language that reads well and behaves the same everywhere:

- **Instead of bash scripts.** Commands are run directly from a list of arguments, never through a shell, so a variable is always exactly one argument and there is nothing to quote. A failing command stops the script with an error that shows the command and the end of its output, without `set -euo pipefail`.
- **Instead of Python or JavaScript scripts.** There is no runtime to install and no packages to manage. JSON, CSV, TOML, YAML and `.env` files are read and written by built-in functions, and a script's `main` signature is its command-line interface, with `--help` and argument checks included.
- **Instead of Makefiles.** In a project, the file `ok.ok` declares tasks with parameters, dependencies and up-to-date checks. `ok` lists them, `ok build --release` runs one.

The same script gives the same output on Linux, macOS and Windows, on x86-64 and arm64: paths print with `/`, written files use `\n` line endings, and sorting does not depend on the machine.

## Try it without installing

The playground at [okrun.dev](https://okrun.dev/#try) runs the real ok, compiled to WebAssembly, in your browser. Samples in this guide that have a Run button run the same way, with their files. Starting programs with `run` and network access with `fetch` need the installed ok.

## Install

On Linux and macOS:

```sh
curl -fsSL https://okrun.dev/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://okrun.dev/install.ps1 | iex
```

The scripts download the binary for your machine from the [latest release](https://github.com/xtr-dev/ok/releases/latest), check it against the release's `SHA256SUMS`, and install it to `~/.local/bin` (on Windows to `%LOCALAPPDATA%\ok\bin`, which is added to your PATH). Set `OK_VERSION` to install a particular version and `OK_INSTALL_DIR` to choose another folder. You can read [install.sh](https://okrun.dev/install.sh) and [install.ps1](https://okrun.dev/install.ps1) before running them.

To install by hand, download the binary for your machine from the release page, check it against `SHA256SUMS`, rename it to `ok` (`ok.exe` on Windows) and put it on your PATH. Releases have binaries for Linux (musl, so they run on any distribution), macOS and Windows, each on x86-64 and arm64.

Check that it works:

```sh
ok --version
```

Because the name is short, a shell alias or another program could also be called `ok`. If `ok --version` prints something unexpected, `which ok` shows which one runs.

In a project that uses ok, you may not need to install anything: `ok self init` adds small wrapper scripts (`./ok`, `ok.cmd`, `ok.ps1`) that download the pinned, checksum-verified version on first use. See [Formatter, checker and wrappers](tools.md).

To complete task names and flags in your shell, add `eval "$(ok self completions bash)"` to your shell's start-up file (or use `zsh`, `fish` or `powershell`).

## A first taste

Here is a complete script. It reads a CSV file, keeps some rows, sorts them and writes JSON:

```csv file=users.csv
name,email,active
Visser,anna@example.com,true
De Vries,bram@example.com,false
Martin,chloe@example.com,true
```

```ok file=active.ok
# Write the active users, sorted by name, to a JSON file.
fn main(
  input: path = "users.csv",     # the CSV file to read
  --out: path = "active.json",   # where to write the JSON
) {
  let users = read(input)
    | where .active == "true"
    | sort .name

  write(users, out)
  print("wrote {len(users)} users to {out}")
}
```

```output
wrote 2 users to active.json
```

A few things to notice:

- `read` chooses the format from the file extension, so a `.csv` file becomes a list of records and a `.json` file is written as JSON.
- `|` passes the value on the left to the function on the right, so the data flows from top to bottom.
- `.active` is short for "the `active` field of each row". CSV values are always strings, which is why the comparison is with `"true"`.
- `fn main(...)` declares the command line. The script takes an optional input file and an `--out` flag, and the comments at the ends of the lines become its help:

```output run="ok active.ok --help"
Write the active users, sorted by name, to a JSON file.

Usage: ok active.ok [input] [--out <path>]

Arguments:
  [input]       the CSV file to read [default: users.csv]

Options:
  --out <path>  where to write the JSON [default: active.json]
  -h, --help    print this help
```

Run a script with `ok FILE`, followed by its arguments. On Linux and macOS you can also start the file with `#!/usr/bin/env ok`, make it executable and run it directly.

## Scripts and tasks

ok has two ways to run code.

A **script** is any `.ok` file you run with `ok script.ok`. Its top-level code runs from top to bottom, and if it declares `fn main`, that is called with the command-line arguments.

A **project** is a directory with an `ok.ok` file. That file only declares tasks and helper functions; it runs nothing by itself:

```ok
# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}
```

Inside the project, `ok` lists the tasks with their descriptions and `ok build --release` runs `gen` and then `build`. The first time you run a task of a project you have not seen before, ok shows you its files and asks whether to trust them, because a cloned repository's tasks are someone else's code. See [Tasks](tasks.md) and [Trust](trust.md).

## How this guide is organised

**Getting started**

- [Your first script](first-script.md) builds a small report script step by step, from reading a file to a command-line tool with `--help`.
- [Coming from bash and make](from-bash.md) maps shell and Makefile habits to ok and explains the differences that matter.

**The language**

- [Values and variables](basics.md): numbers, strings, lists, records, paths and durations, `let` and `var`.
- [Control flow and patterns](control-flow.md): `if`, loops and `match`.
- [Functions](functions.md): `fn`, parameters, closures and the `.field` shorthand.
- [Pipelines](pipelines.md): `|`, the list functions and how large files stream.
- [Errors](errors.md): how errors look, `?`, `??`, `try` and `fail`.
- [Modules](modules.md): splitting code over files with `import`.

**Scripts and the system**

- [Data formats](data.md): JSON, CSV, TOML, YAML, `.env` and compressed files.
- [Files and paths](files.md): reading, writing, copying and finding files.
- [Running commands](commands.md): `run`, capturing output, pipes, redirects and options.
- [Command-line scripts](script-cli.md): `fn main`, flags, choices and help.

**Projects**

- [Tasks](tasks.md): `ok.ok`, dependencies, parameters, namespaces and up-to-date checks.
- [Trust](trust.md): why ok asks before running a project's tasks, and how to use it in CI.
- [Formatter, checker and wrappers](tools.md): `ok self fmt`, `ok self check` and `ok self init`.

**Reference**

- [Standard library](stdlib.md): every built-in function.

The guide explains and shows; the [language specification](../SPEC.md) is the precise definition and has the fine print. Real scripts rewritten in ok, each next to the bash, Python or Makefile original, are in [examples](../examples/).
