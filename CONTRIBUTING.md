# Contributing

Thanks for helping with ok. This file covers the practical parts; the
maintainer handbook with all conventions is [`CLAUDE.md`](CLAUDE.md).

## Ground rules

- **Spec first.** Behaviour is defined by [`SPEC.md`](SPEC.md). A change in
  behaviour changes `SPEC.md` in the same commit or the one before.
- **Decisions are logged.** A design choice the spec did not settle gets a
  dated paragraph in [`DECISIONS.md`](DECISIONS.md).
- **No scope creep.** Ideas outside the current milestone go into
  [`IDEAS.md`](IDEAS.md).
- **Portability.** Nothing may depend on a Unix shell or coreutils. If
  something cannot work on Windows, design it differently.
- **Dependencies** must be permissively licensed; `cargo deny check` enforces
  this. No `unsafe` without a logged reason.

## Building and testing

The toolchain is pinned in `rust-toolchain.toml`.

| What | With ok (from M5) | With cargo |
|---|---|---|
| build | `ok build` | `cargo build --workspace` |
| test | `ok test` | `cargo test --workspace` |
| format | `ok fmt` | `cargo fmt --all` |
| lint | `ok lint` | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` |
| licences | `ok deny` | `cargo deny check` |
| notices | `ok third-party` | `cargo about generate about.hbs > THIRD_PARTY.md` |

CI (format, clippy and tests on Linux, macOS and Windows, cargo-deny, a
`THIRD_PARTY.md` check and fuzzing) is currently switched off and runs only
when started by hand from the Actions tab. Run the checks listed in
`CLAUDE.md` locally before committing.

## Golden tests

From M1, most tests are golden tests in `tests/cases/`: `name.ok` with the
expected `name.stdout`, and optionally `name.stderr` and `name.exit`. One
command regenerates the expectations; review every regenerated diff before
committing it.

## Commits

[Conventional Commits](https://www.conventionalcommits.org/): `feat:`,
`fix:`, `docs:`, `test:`, `refactor:`, `chore:`, `ci:`. Keep commits small.
Never rewrite pushed history. Add user-visible changes to `CHANGELOG.md`
under `Unreleased`.

## Licence

ok is licensed under either of MIT (`LICENSE-MIT`) or Apache-2.0
(`LICENSE-APACHE`), at your option. Unless you explicitly state otherwise,
any contribution you intentionally submit for inclusion in ok, as defined in
the Apache-2.0 licence, is dual licensed as above, without any additional
terms or conditions.
