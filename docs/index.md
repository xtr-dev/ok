# Introduction

ok is a scripting language and task runner in one static binary. This guide shows how to install it, write scripts and run a project's tasks.

## What ok is for

Most projects collect small programs around the main code, such as a bash script that prepares a release, a Python script that converts an export and a Makefile that builds and tests. Each is written in a different language, and each breaks in its own place. The bash script breaks on macOS or on a file name with a space, the Python script on a machine with the wrong Python version, and the Makefile on Windows.

ok replaces all three with one language:

- Instead of a bash script, ok runs each command directly from a list of arguments and never through a shell. A variable is always exactly one argument, so there is nothing to quote. A failing command stops the script with an error that shows the command and the end of its output, and you do not need `set -euo pipefail` for that.
- Instead of a Python or JavaScript script, ok has no runtime to install and no packages to manage. Built-in functions read and write JSON, CSV, TOML, YAML and `.env` files. The parameters of a script's `main` function are its command line, and ok checks the arguments and prints `--help` from them.
- Instead of a Makefile, a project has a file `.ok/ok.ok` that declares tasks with parameters, dependencies and up-to-date checks. `ok` lists the tasks and `ok build --release` runs one.

The same script gives the same output on Linux, macOS and Windows, on x86-64 and arm64. Paths print with `/`, written files use `\n` line endings, and sorting does not depend on the machine.

## Try it without installing

The playground at [okrun.dev](https://okrun.dev/#try) runs ok in your browser, compiled to WebAssembly. Samples in this guide that have a Run button run the same way, with their files. The browser cannot start programs with `run` or use the network with `fetch`, so samples that do need the installed ok.

## Install ok

On Linux and macOS:

```sh
curl -fsSL https://okrun.dev/install.sh | sh
```

On Windows, in PowerShell:

```powershell
irm https://okrun.dev/install.ps1 | iex
```

Each script downloads the binary for your machine from the [latest release](https://github.com/xtr-dev/ok/releases/latest) and checks it against the release's `SHA256SUMS`. On Linux and macOS it installs the binary to `~/.local/bin`. On Windows it installs to `%LOCALAPPDATA%\ok\bin` and adds that folder to your PATH. Set `OK_VERSION` to install a particular version and `OK_INSTALL_DIR` to choose another folder. You can read [install.sh](https://okrun.dev/install.sh) and [install.ps1](https://okrun.dev/install.ps1) before you run them.

To install by hand, download the binary for your machine from the release page, check it against `SHA256SUMS`, rename it to `ok` (`ok.exe` on Windows) and put it on your PATH. Releases have binaries for Linux, macOS and Windows, each for x86-64 and arm64. The Linux binaries are built with musl, so they run on any distribution.

To check the installation, print the version:

```sh
ok --version
```

This prints `ok` and a version number, for example `ok 0.4.0`. A shell alias or another program can also be called `ok`, because the name is short. If the command prints something else, `which ok` shows which program runs.

A project that uses ok can include wrapper scripts (`./ok`, `ok.cmd`, `ok.ps1`), which `ok self init --wrappers` adds. On first use, a wrapper downloads the version of ok that the project pins and checks the download against a pinned checksum. It then runs ok with your arguments, so you can run that project's tasks without installing ok. See [The `./ok` wrappers](tools.md#the-ok-wrappers).

To complete task names and flags in bash, add `eval "$(ok self completions bash)"` to `~/.bashrc`. [Shell completions](tasks.md#shell-completions) has the lines for zsh, fish and PowerShell.

## A complete script

This script reads a CSV file, keeps the active users, sorts them by name and writes them as JSON. Its input is `users.csv`:

```csv file=users.csv
name,email,active
Visser,anna@example.com,true
De Vries,bram@example.com,false
Martin,chloe@example.com,true
```

Save the script as `active.ok` and run it with `ok active.ok`:

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

The script uses four things that the rest of the guide explains:

- `read` and `write` choose the format from the file extension. `read` turns a `.csv` file into a list of records, and `write` writes JSON to a `.json` file.
- `|` passes the value on its left to the function on its right, so the data flows from top to bottom.
- `.active` is short for "the `active` field of each row". CSV values are always strings, so the comparison is with `"true"`.
- `fn main(...)` declares the command line. The script takes an optional input file and an `--out` flag.

The comment above `main` and the comments at the ends of the parameter lines become the script's help:

```output run="ok active.ok --help"
Write the active users, sorted by name, to a JSON file.

Usage: ok active.ok [input] [--out <path>]

Arguments:
  [input]       the CSV file to read [default: users.csv]

Options:
  --out <path>  where to write the JSON [default: active.json]
  -h, --help    print this help
```

To run any script, type `ok`, the file and then the script's arguments. On Linux and macOS you can also start the file with the line `#!/usr/bin/env ok`, make it executable and run it directly.

## Scripts and projects

ok runs code in two ways, as a script or as a task of a project.

A **script** is any `.ok` file that you run with `ok script.ok`. Its top-level code runs from top to bottom. If the script declares `fn main`, ok then calls `main` with the command-line arguments.

A **project** is a directory with a root file. The recommended root file is `.ok/ok.ok`, in a hidden `.ok/` directory that also holds the project's other ok files. A small project can use a single file named `.ok` instead, and a file named `ok.ok` works too. The root file declares tasks and helper functions, and loading it runs nothing. This fragment of a root file declares a task `build` that needs another task, `gen`:

```ok
# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}
```

Inside the project, `ok` lists the tasks with their descriptions, and `ok build --release` runs `gen` and then `build`. See [Tasks](tasks.md).

The tasks of a cloned repository are someone else's code. The first time you run a task of a project that you have not approved, ok shows you the project's files and asks whether you trust them. See [Trust](trust.md).

## How this guide is organised

**Getting started**

- [Your first script](first-script.md): a tutorial that builds a report script step by step, from reading a file to a command-line tool with `--help`.
- [Coming from bash and make](from-bash.md): how bash idioms and Makefile patterns are written in ok, and where ok behaves differently.

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

- [Tasks](tasks.md): the `.ok/` directory, dependencies, parameters, namespaces and up-to-date checks.
- [Trust](trust.md): why ok asks before running a project's tasks, and how to use it in CI.
- [Formatter, checker and wrappers](tools.md): `ok self fmt`, `ok self check`, `ok self init` and `ok self promote`.

**Reference**

- [Standard library](stdlib.md): every built-in function.

The guide explains ok and shows it in use. The [language specification](../SPEC.md) is the exact definition of the language and of the `ok` command. The [examples](../examples/) are real scripts rewritten in ok, each next to its bash, Python or Makefile original.
