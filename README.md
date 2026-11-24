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
described in `SPEC.md`; design choices are logged in `DECISIONS.md`. Not yet
released: Bas publishes the repository and the first release.

## Install

There are no releases yet. From M7 on, each release ships static binaries for
Linux (musl), macOS and Windows on x86-64 and arm64, with SHA-256 checksums,
and `ok self init` writes wrapper scripts (`./ok`, `ok.cmd`) that download a
pinned, checksum-verified `ok` so contributors need nothing preinstalled.

## Build from source

Requires Rust (the version is pinned in `rust-toolchain.toml`; rustup picks
it up automatically).

```
cargo build --release      # binary at target/release/ok
cargo test --workspace
```

The crate is named `okrun` because `ok` is taken on crates.io; the binary
is `ok`. Other tools also install a binary called `ok`, so check `which ok`
if something unexpected runs.

## Licence

Public domain under [CC0 1.0](LICENSE). Third-party notices for the
dependencies compiled into the binary are in [`THIRD_PARTY.md`](THIRD_PARTY.md).
