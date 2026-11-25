# ok

`ok` is a portable scripting language and task runner in one static binary.
It replaces hard-to-read bash scripts, Python or JS scripts that need a
managed runtime, and Makefiles. The same script behaves the same on Linux,
macOS and Windows, on x86-64 and arm64. Commands are never run through a
shell, data formats (JSON, CSV, TOML, YAML, ...) are built in, and a script's
`main` signature is its command-line interface.

```
# Convert a CSV export to JSON, keeping only active users
fn main(input: path, --out: path = "active.json") {
  read(input)
    | where .active == "true"
    | sort .name
    | write(out)
  let n = read(out) | count
  print("wrote {n} users to {out}")
}
```

Inside a project, `ok` is the task runner. Tasks live in `ok.ok`:

```
# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}
```

`ok` lists the tasks, `ok build --release` runs one. See [`SPEC.md`](SPEC.md)
for the full language and [`examples/`](examples/) for real scripts rewritten
in `ok`, each next to the original.

## Status

All milestones of the v0.1.0 plan are implemented: the language, data
formats and pipelines, commands and the standard library, script
arguments, project mode, trust, and the release tooling. The language is
described in `SPEC.md`; design choices are logged in `DECISIONS.md`. The
first release is [v0.1.0](https://github.com/xtr-dev/ok/releases/tag/v0.1.0).

## Install

Each release ships single static binaries for Linux (musl), macOS and
Windows on x86-64 and arm64, plus `SHA256SUMS`, `THIRD_PARTY.md` and the
licence files:

```
ok-x86_64-unknown-linux-musl      ok-aarch64-unknown-linux-musl
ok-x86_64-apple-darwin            ok-aarch64-apple-darwin
ok-x86_64-pc-windows-msvc.exe     ok-aarch64-pc-windows-msvc.exe
```

Download the one for your machine, check it against `SHA256SUMS`, rename it
to `ok` (`ok.exe` on Windows) and put it on your PATH.

**In a project, nobody needs to install anything.** Run `ok self init` once:
it writes a starter `ok.ok` and the wrappers `./ok` (POSIX sh), `ok.cmd` and
`ok.ps1`, which pin this version of ok and the checksum of every release
binary. A contributor or CI job then runs `./ok build` (or `ok.cmd build`);
the wrapper downloads the right binary into the user cache once, refuses it
if the checksum does not match, and runs it. `OK_DOWNLOAD_URL` points the
wrappers (and `ok self init`) at a mirror, which is also how to use them
while the repository is private.

Projects you clone are not trusted automatically: the first task run shows
the project files and asks. In CI, set `OK_TRUST=1`.

Shell completions: `eval "$(ok self completions bash)"` (or `zsh`, `fish`,
`powershell`).

## Build from source

Requires Rust (the version is pinned in `rust-toolchain.toml`; rustup picks
it up automatically).

```
cargo build --release      # binary at target/release/ok
cargo test --workspace
```

The crate is named `okrun` because `ok` is taken on crates.io; the binary
is `ok`. Because the name is short, a shell alias or another tool you
installed could also be called `ok`; `which ok` shows which one runs.

## Licence

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option. Third-party notices for the
dependencies compiled into the binary are in [`THIRD_PARTY.md`](THIRD_PARTY.md).
