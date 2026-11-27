# ✅ ok

`ok` is a scripting language and task runner in one static binary, for the
small programs around a project: the bash script that prepares a release,
the Python or JavaScript script that converts an export, the Makefile that
builds and tests. A script gives the same output on Linux, macOS and
Windows, on x86-64 and arm64. `ok` runs each command from a list of
arguments and never through a shell, reads and writes JSON, CSV, TOML, YAML
and `.env` files with built-in functions, and takes a script's command line
from the parameters of its `main` function.

This script reads a CSV file, keeps the active users, sorts them by name and
writes them as JSON. Save it as `active.ok`:

```
# Write the active users, sorted by name, to a JSON file.
fn main(input: path, --out: path = "active.json") {
  let users = read(input)
    | where .active == "true"
    | sort .name

  write(users, out)
  print("wrote {len(users)} users to {out}")
}
```

With this `users.csv`:

```
name,email,active
Visser,anna@example.com,true
De Vries,bram@example.com,false
Martin,chloe@example.com,true
```

`ok active.ok users.csv` prints:

```
wrote 2 users to active.json
```

`ok active.ok --help` prints the usage, built from the comment and the
parameters of `main`.

In a project, `ok` is the task runner. Tasks are declared in the file
`.ok/ok.ok` (a small project can use a single file named `.ok`). This
fragment declares a task `build` that needs another task, `gen`:

```
# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}
```

`ok` lists the tasks, and `ok build --release` runs `gen` and then `build`.

## Limits

- Trust is not a sandbox. `ok` asks before it runs the tasks of a project
  you have not approved, but a script or a trusted task can do anything
  that you can.
- `ok` has no package manager, no user-defined types, no async and no REPL
  beyond `ok -e`. Section 20 of [`SPEC.md`](SPEC.md) has the full list, and
  [`IDEAS.md`](IDEAS.md) collects the ideas that were left out.
- A task cannot declare permissions. The `allow` modifier is reserved for a
  permission system, and `ok` rejects it.

## Install

To try `ok` without installing it, use the playground at
[okrun.dev](https://okrun.dev/#try). It runs the examples in your browser.

Linux and macOS:

```
curl -fsSL https://okrun.dev/install.sh | sh
```

Windows (PowerShell):

```
irm https://okrun.dev/install.ps1 | iex
```

The scripts download the binary for your machine from the latest release,
check it against `SHA256SUMS`, and install it to `~/.local/bin` (Windows:
`%LOCALAPPDATA%\ok\bin`, added to your PATH). `OK_VERSION` pins a version and
`OK_INSTALL_DIR` changes the folder.

To install by hand, download a binary from the
[releases page](https://github.com/xtr-dev/ok/releases/latest). Each release
has one static binary per target, plus `SHA256SUMS`, `THIRD_PARTY.md` and
the licence files:

```
ok-x86_64-unknown-linux-musl      ok-aarch64-unknown-linux-musl
ok-x86_64-apple-darwin            ok-aarch64-apple-darwin
ok-x86_64-pc-windows-msvc.exe     ok-aarch64-pc-windows-msvc.exe
```

Check the one for your machine against `SHA256SUMS`, rename it to `ok`
(`ok.exe` on Windows) and put it on your PATH.

Because the name is short, a shell alias or another tool you installed could
also be called `ok`. `which ok` shows which one runs.

Shell completions: `eval "$(ok self completions bash)"` (or `zsh`, `fish`,
`powershell`).

### Projects without an installed ok

Run `ok self init --wrappers` once in a project. It writes a starter
`.ok/ok.ok` and the wrappers `./ok` (POSIX sh), `ok.cmd` and `ok.ps1`, which
pin this version of `ok` and the checksum of every release binary. A
contributor or CI job then runs `./ok build` (or `ok.cmd build`). The
wrapper downloads the right binary into the user cache once, refuses it if
the checksum does not match, and runs it. `OK_DOWNLOAD_URL` points the
wrappers (and `ok self init`) at a mirror.

Projects you clone are not trusted automatically: the first task run shows
the project files and asks. In CI, set `OK_TRUST=1`.

## Build from source

Requires Rust. The version is pinned in `rust-toolchain.toml`, and rustup
installs it when you run cargo.

```
cargo build --release      # binary at target/release/ok
cargo test --workspace
```

The crate is named `okrun` because `ok` is taken on crates.io. The binary
is `ok`.

## Documentation and status

- The guide at [okrun.dev/docs](https://okrun.dev/docs/) (source in
  [`docs/`](docs/)) explains the language with examples that run in the
  browser.
- [`SPEC.md`](SPEC.md) is the full definition of the language.
- [`examples/`](examples/) has real bash scripts, Makefiles and Python
  scripts rewritten in `ok`, each next to the original.
- [`CHANGELOG.md`](CHANGELOG.md) lists the changes per version, and
  [`DECISIONS.md`](DECISIONS.md) logs the design choices.
- [`CONTRIBUTING.md`](CONTRIBUTING.md) says how to build, test and send a
  change. [`SECURITY.md`](SECURITY.md) says how to report a vulnerability.

These parts are implemented: the language, data formats and pipelines,
commands and the standard library, script arguments, project mode with
parallel tasks, trust, the static checker (`ok self check`), the formatter
(`ok self fmt`), a WebAssembly build and the release tooling. The latest
release is on the
[releases page](https://github.com/xtr-dev/ok/releases/latest).

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option. Third-party notices for the dependencies compiled into the
binary are in [`THIRD_PARTY.md`](THIRD_PARTY.md).
