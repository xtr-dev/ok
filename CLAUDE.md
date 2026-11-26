# CLAUDE.md: maintainer handbook

Working notes for whoever maintains this repository next (usually a future
Claude Code session). Keep this file current; it is the source of truth for
how the repo is run. The language itself is defined in `SPEC.md`.

## What this is

`ok`: a portable scripting language and task runner, one static Rust binary.
The project owners (the `xtr-dev` maintainers) decide what the language is;
the maintainer working on the code decides how the repo and code are built. The original brief was `PROMPT.md`;
it was removed before the first public release (it is in the git history).
Its rules are kept below, and the language it described is now `SPEC.md`.

## Current state

- All milestones M0 to M7 are implemented; `v0.1.0` is released
  (2026-11-25).
- GitHub: public repo `xtr-dev/ok`. Publishing a release and publishing to
  crates.io are done by hand by a project owner; never do either yourself.
  CI runs on every push to `main` and every pull request (Linux, macOS,
  Windows).
- The release workflow (started by hand) builds the six binaries, runs
  `tests/smoke/smoke.ok` and the bootstrap wrappers on each target's own OS
  (job `smoke`), and only then creates the draft release.

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

## Local checks

CI (`.github/workflows/ci.yml`) runs on every push to `main` and every pull
request, on Linux, macOS and Windows. Run the same checks locally before
every commit, in order:

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check
cargo about generate about.hbs > THIRD_PARTY.md && git diff --exit-code THIRD_PARTY.md
```

and at the end of a milestone also `cargo +nightly fuzz run lex -- -max_total_time=60`
and the same for `parse`. Local checks cover only the machine they run on;
CI is what covers macOS and Windows, so check its result after pushing.

## Security upkeep

`fetch` (ureq, rustls) and the archive functions (zip, tar, flate2) are built
in by decision (DECISIONS.md, after v0.1.0), so their security is ours to
keep: run `cargo deny check` (advisories) at the start of every session
and before every release, update a crate with an advisory promptly, and
ship a patch release. Keep the zip-slip and symlink tests in `archive.rs`
passing and extend them when extraction code changes.

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
site/                   landing page, playground (playground.js, playground-worker.js)
                        and install.sh / install.ps1, published to
                        https://okrun.dev/ by pages.yml (after the install
                        scripts pass on Linux, macOS and Windows); pages.yml
                        adds ok.wasm, built from the commit, and three
                        examples/ folders the playground loads
.cargo/config.toml      stack size for the WebAssembly build
tests/wasm/smoke.mjs    runs the WebAssembly build under Node (CI job `wasm`)
SPEC.md                 normative language spec
DECISIONS.md            dated design decisions
IDEAS.md                out-of-scope ideas
THIRD_PARTY.md          generated licence notices (cargo-about); shipped with binaries
deny.toml, about.toml, about.hbs   licence tooling
.github/workflows/      ci.yml (fmt, clippy, test x3 OS, wasm, deny, notices, fuzz), release.yml (6 targets, draft release), pages.yml (site)
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
| `command.rs` | `run`: modes (live/captured/piped), program lookup, pipes, redirects, options |
| `files.rs` | file and path functions, `glob` |
| `text.rs` | string and regex functions |
| `system.rs` | `env`, time, `prompt`/`confirm`, `hash` |
| `net.rs` | `fetch` (ureq + rustls + platform verifier) |
| `archive.rs` | `zip`/`unzip`/`tar`/`untar` with zip-slip checks |
| `sig.rs` | command-line parsing and `--help` from a signature (scripts and tasks) |
| `project.rs` | project loading, namespaces, mounts, `needs`, running tasks, listing, completions |
| `jobs.rs` | dependency plan (task instances with arguments), sequential and parallel runs, `--task-worker` processes |
| `trust.rs` | fingerprints, trust store, prompt, diff, `ok trust`/`untrust` |
| `fmt.rs` | `ok self fmt`: token layout guided by the syntax tree, line breaking, self-check of tree and comments |
| `check.rs` | static checks of `ok self check`: scope model, unknown names, argument counts, lints |
| `tools.rs` | `ok self check`, `fmt`, `init` and the bootstrap wrapper templates |
| `cli.rs` | `ok` command line |

Tests: unit tests next to the code; `tests/golden.rs` (cases in
`tests/cases/`, libraries for import tests in `tests/cases/lib/`, data
fixtures in `tests/cases/data/` (kept byte-exact by `.gitattributes`), and
files the cases write go to the ignored `tests/cases/out/`),
`tests/cli.rs`, `tests/examples_parse.rs` (every example parses and is in
canonical format), `tests/examples_run.rs` (examples with fake tools,
expectations in `tests/examples/`), `tests/project.rs` (project mode and
trust), `tests/bootstrap.rs` (`ok self init` and the `./ok` wrapper),
`tests/no_panic.rs`, `tests/format_repo.rs` (formatting every `.ok` file of
the repository keeps its syntax tree and comments and is idempotent, also
at narrow widths) and `tests/check_repo.rs` (`ok self check` finds nothing
in valid code). `golden.rs` also runs `ok self check` on `tests/check/`
(libraries in `tests/check/lib/`) and the formatter on `tests/fmt/NAME.ok`
against `NAME.formatted`.

Builtin signatures for `ok self check` live in `builtins::SIGNATURES`; a
new builtin needs an entry there (a test calls every builtin to compare). `crates/ok-fixture` is the test-only program behind
`run` tests and the fake tools. Bless everything with `OK_BLESS=1 cargo
test --workspace` and review the diff.

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
- Each milestone ends with the local checks and CI on all three OSes green,
  `SPEC.md` and `CHANGELOG.md` updated, and a report.
