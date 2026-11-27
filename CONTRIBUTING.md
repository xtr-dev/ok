# Contributing

This file covers building, testing and committing. The maintainer handbook
with all conventions is [`CLAUDE.md`](CLAUDE.md).

## Ground rules

- Behaviour is defined by [`SPEC.md`](SPEC.md). A change in behaviour
  changes `SPEC.md` in the same commit or the one before.
- A design choice that the spec did not settle gets a dated paragraph in
  [`DECISIONS.md`](DECISIONS.md).
- Ideas outside the current milestone go into [`IDEAS.md`](IDEAS.md), not
  into the code.
- Nothing may depend on a Unix shell or coreutils. If something cannot work
  on Windows, design it differently.
- Dependencies must be permissively licensed, and `cargo deny check`
  enforces this.
- No `unsafe` without a reason logged in `DECISIONS.md`.

## Building and testing

The toolchain is pinned in `rust-toolchain.toml`. The repository's own tasks
are in `ok.ok`, so each check runs with `ok` or with cargo:

| What | With ok | With cargo |
|---|---|---|
| build | `ok build` | `cargo build --workspace` |
| test | `ok test` | `cargo test --workspace` |
| format | `ok fmt` | `cargo fmt --all` |
| lint | `ok lint` | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` |
| licences | `ok deny` | `cargo deny check` |
| notices | `ok third-party` | `cargo about generate -m crates/okrun/Cargo.toml about.hbs > THIRD_PARTY.md` |

CI runs on every push to `main` and on every pull request: the format
check, clippy and the tests on Linux, macOS and Windows, cargo-deny, a
check that `THIRD_PARTY.md` is current, a WebAssembly build and a short
fuzzing run. Run the checks listed under "Local checks" in `CLAUDE.md`
before you commit.

## Golden tests

Most tests are golden tests in `tests/cases/`: `name.ok` with the expected
`name.stdout`, and optionally `name.stderr` and `name.exit`. `ok bless` (or
`OK_BLESS=1 cargo test --test golden`) regenerates the expectations. Review
every regenerated diff before you commit it.

## Commits

Use [Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
`fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `ci:`. Keep commits small.
Never rewrite pushed history. Add user-visible changes to `CHANGELOG.md`
under `Unreleased`.

## Licence

ok is licensed under either of MIT (`LICENSE-MIT`) or Apache-2.0
(`LICENSE-APACHE`), at your option. Unless you explicitly state otherwise,
any contribution you intentionally submit for inclusion in ok, as defined in
the Apache-2.0 licence, is dual licensed as above, without any additional
terms or conditions.
