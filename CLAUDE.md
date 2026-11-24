# CLAUDE.md: maintainer handbook

Working notes for whoever maintains this repository next (usually a future
Claude Code session). Keep this file current; it is the source of truth for
how the repo is run. The language itself is defined in `SPEC.md`.

## What this is

`ok`: a portable scripting language and task runner, one static Rust binary.
Project owner: Bas (decides what the language is). Maintainer: decides how the
repo and code are built. The original brief is `PROMPT.md`.

## Current state

- Milestone: **M2 (data)** done, waiting for review. Scripts run with
  `ok file.ok` and `ok -e`, with formats, pipelines and list functions;
  `run`, the file/text/env/net stdlib, `main` argument parsing, project mode
  and trust are later milestones and report "not implemented yet
  (milestone Mx)".
- Milestones are in `PROMPT.md`. Stop after each one, report to Bas, and wait
  for a go-ahead.
- GitHub: private repo `xtr-dev/ok`. Never make it public and never publish
  to crates.io; Bas does both.

## Commands

Toolchain pinned in `rust-toolchain.toml` (1.100.0). Until M5, use cargo
directly; from M5, `ok <task>` via the repo's `ok.ok`.

```
cargo build --workspace
cargo test --workspace
cargo fmt --all                     # format
cargo fmt --all --check             # CI format check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check                    # licences, advisories, sources
cargo about generate about.hbs > THIRD_PARTY.md
OK_BLESS=1 cargo test --test golden # regenerate golden expectations, then review git diff
cargo +nightly fuzz run parse -- -max_total_time=300   # needs nightly + cargo-fuzz
```

## Local checks (CI is switched off)

GitHub Actions runs only when started by hand (`workflow_dispatch`); Bas
asked for local checks only for now. Before every commit run, in order:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo about generate about.hbs > THIRD_PARTY.md && git diff --exit-code THIRD_PARTY.md
```

and at the end of a milestone also `cargo +nightly fuzz run lex -- -max_total_time=60`
and the same for `parse`. Local checks cover Linux only: macOS and Windows
behaviour is unverified until CI is switched back on (restore the `push` /
`pull_request` triggers in `.github/workflows/ci.yml`), so mention that in
milestone reports.

## Layout

```
Cargo.toml              workspace; shared version, licence, lints, release profile
crates/okrun/           the published package `okrun`
  src/lib.rs            the language (lexer, parser, evaluator, stdlib from M1)
  src/main.rs           the `ok` binary: thin, calls into the library
  tests/                integration tests that run the real binary
examples/<name>/        real scripts rewritten in ok, original alongside
tests/cases/            golden tests (from M1)
fuzz/                   cargo-fuzz targets for lexer and parser (from M1)
ok.ok                   the repo's own tasks (dogfooding)
SPEC.md                 normative language spec
DECISIONS.md            dated design decisions
IDEAS.md                out-of-scope ideas
THIRD_PARTY.md          generated licence notices (cargo-about); shipped with binaries
deny.toml, about.toml, about.hbs   licence tooling
.github/workflows/      ci.yml (fmt, clippy, test x3 OS, deny, notices), release.yml (6 targets, draft release)
```

Modules in `crates/okrun/src/`:

| Module | Role |
|---|---|
| `source.rs` | `SourceMap`, `Span`, line/column lookup |
| `error.rs` | `OkError`, kinds, rendering with snippets, `suggest` (edit distance) |
| `lexer.rs` | tokens; strings with nested interpolation token lists; command mode; newline rules |
| `ast.rs` | syntax tree |
| `parser.rs` | recursive descent; implicit closures (`wrap_it`/`find_it`); stage parsing |
| `value.rs` | `Value`, `Record` (ordered map), display/repr, equality, ordering |
| `path.rs` | `OkPath` normalisation, display (`/`) and native conversion |
| `format.rs` | format specs for `fmt` and `{x:spec}` |
| `interp.rs` | scopes, evaluation, calls and type checks, imports and library rules |
| `builtins.rs` | core builtins (`CORE` table), `eager`/`streaming` constructors, argument helpers |
| `lazy.rs` | lazy values: `Source` trait, `force`, `cursor`, `adapt`, line sources |
| `list.rs` | list and record functions; `named`/`count_args` helpers |
| `formats.rs` | `read`/`write`/`parse`/`to`/`stdin` and every format converter |

Laziness rule: `Interp::eval` always materialises; only `eval_lazy` (pipeline
inputs, call arguments, `for` iterables) may return `Value::Lazy`, and
`call_value` forces arguments unless the builtin is `streaming`. A new
builtin that should stream must be declared with `streaming(...)` and pull
from `lazy::cursor`.
| `cli.rs` | `ok` command line |

Tests: unit tests next to the code; `tests/golden.rs` (cases in
`tests/cases/`, libraries for import tests in `tests/cases/lib/`, data
fixtures in `tests/cases/data/` (kept byte-exact by `.gitattributes`), and
files the cases write go to the ignored `tests/cases/out/`),
`tests/cli.rs`, `tests/examples_parse.rs` (every example must parse),
`tests/no_panic.rs`.

## Conventions

- Rust edition 2024, rustfmt config in `rustfmt.toml`, clippy warnings are
  errors in CI. `unsafe_code = "forbid"` at the workspace level; any exception
  needs a logged reason in `DECISIONS.md`.
- Lexer, parser and evaluator are hand-written. Use well-maintained crates for
  formats, regex, TLS (rustls only, never OpenSSL), archives.
- Dependencies: permissive licences only (see `deny.toml`). Regenerate
  `THIRD_PARTY.md` whenever `Cargo.lock` changes; CI checks it.
- Errors are part of the product: every error has a span, a snippet, and a
  help line when the fix is knowable. Golden-test the rendering.
- Output must be identical across OSes (paths print with `/`, `\n` line
  endings, deterministic ordering).
- Tests must not depend on system tools; use fixture commands built in the
  repo for `run` tests.
- Comments explain purpose and reasoning so they make sense without the
  conversation that produced them.

## Working rules (from the brief)

- **Spec first.** Change `SPEC.md` before changing behaviour, in the same
  commit or the one before.
- **Decide and log.** For anything the brief leaves open, pick the option that
  makes scripts easier to read, record it in `DECISIONS.md`, move on.
- **Stop and ask** when a decision would contradict the brief, change syntax
  in an approved example, add a dependency with a non-permissive licence, or
  weaken the trust model or the no-shell rule.
- **Readability wins ties.**
- **Portability is a feature, not a port.** No behaviour may depend on a Unix
  shell or coreutils.
- **Few dependencies.** No `unsafe` outside dependencies without a logged
  reason.
- **No scope creep.** Ideas go to `IDEAS.md`.
- **Report honestly.** Each milestone report lists what was built, what was
  decided, what is weak, and what to change in the spec.
- Commit small and often, Conventional Commits. Never rewrite pushed history.
- Each milestone ends with the local checks green (CI on all three OSes
  when it is switched back on), `SPEC.md` and `CHANGELOG.md` updated, and a
  report.
