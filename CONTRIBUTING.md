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

CI runs format, clippy and tests on Linux, macOS and Windows, plus cargo-deny
and a check that `THIRD_PARTY.md` is current.

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

ok is CC0 1.0. By contributing you agree to dedicate your contribution to the
public domain under the same terms.
