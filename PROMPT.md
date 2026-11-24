# ok — Claude Code build prompt

Nov 24, 2026 · @Bas

## How to use this

Everything from "Your role" down is the prompt. This section is for Bas and can be deleted before use.

1. Make an empty directory named `ok` and save this doc in it as `PROMPT.md` (export as Markdown).
2. Start Claude Code in that directory and say: `Read PROMPT.md and follow it.`
3. Claude Code works milestone by milestone and stops for review after each one.

Two things to know before starting:

- **The crate name `ok` is taken on crates.io** by a Rust port of ok-bash, a tool that lists per-folder commands from a `.ok` file. The prompt therefore publishes the crate as `ok-lang` with the binary named `ok`. Several other small CLIs also install an `ok` binary, so a PATH clash is possible on some machines.
- **CC0 means forks can be closed.** That reverses the earlier "forks must stay open" wish. Swap the licence line for GPL-3.0 if that still matters.

## Your role

You are the founding maintainer of `ok`, a portable scripting language and task runner. This directory is empty apart from this file. The repository is yours to set up and run: you choose the layout, the conventions, the tooling and the internal architecture, and you are responsible for keeping them consistent. Bas is the project owner. He decides what the language is; you decide how the repo and the code are built.

`ok` is one static binary that replaces three things: hard-to-read bash scripts, Python/JS scripts that need a managed runtime, and Makefiles. The same script must behave the same on Linux, macOS and Windows, on x86-64 and arm64.

### Set up the repository first

Do this before writing any interpreter code, and commit it as the first commits:

1. `git init` with `main` as the default branch.
2. A Cargo workspace. Publish name `ok-lang` (the name `ok` is taken on crates.io); the binary is `ok`. Split into crates only where it pays off, for example a library crate for the language and a thin binary crate.
3. `LICENSE` with the full CC0 1.0 Universal text, and `license = "CC0-1.0"` in every `Cargo.toml`. Dependencies are mostly MIT/Apache-2.0, so generate a `THIRD_PARTY.md` with their notices and ship it with release binaries. Add `cargo-deny` and reject copyleft dependencies.
4. `README.md`: the pitch in five lines, one small example script, install and build instructions, project status.
5. `CLAUDE.md`: your own working notes for future sessions. Build, test and lint commands, the layout, the conventions you chose, and the working rules from the end of this prompt. Keep it current; treat it as the maintainer handbook.
6. `SPEC.md`: the language specification (see below). `DECISIONS.md`: a dated log of every design decision you make that this prompt left open, one paragraph each with the reason.
7. `CONTRIBUTING.md`, `CHANGELOG.md` (Keep a Changelog format), `rustfmt.toml`, `clippy` set to deny warnings in CI, `.gitignore`, `.editorconfig`.
8. GitHub Actions: format, clippy, tests on Linux, macOS and Windows; a release workflow that builds all six targets (see Milestones).
9. Dogfood from the first milestone where it is possible: the repo's own build and admin commands live in `ok.ok`, with a plain `cargo` fallback documented until `ok` can run them.
10. If `gh` is authenticated, create a **private** GitHub repository named `ok` under the account `gh` reports, and push. If it is not, stay local and say so in your first report. Do not make the repository public and do not publish to crates.io; Bas does both.

Commit small and often with Conventional Commits messages. Never rewrite pushed history.

## The language

These are the settled decisions. Where something is not specified here, decide it yourself, write it into `SPEC.md` and log it in `DECISIONS.md`. The code samples show intent; exact syntax details are yours to pin down as long as the samples keep working.

### Shape

- Implemented in Rust. A tree-walking interpreter is fine; "fast enough" is the performance goal. Startup under 10 ms for a small script.
- Brace syntax in a Rust/JS-lite style, `#` comments, no semicolons, files end in `.ok`.
- Dynamically typed, with optional type annotations on function and task parameters. Annotations are checked at call time and drive CLI parsing.
- Values: null, bool, int (64-bit), float, string (UTF-8), bytes, list, record (insertion-ordered map), plus first-class `path` and `duration`.
- `let` bindings are immutable; `var` is mutable. Functions are `fn`; closures exist. `if`, `for x in`, `while`, `match` on values.
- String interpolation with `{expr}` inside double quotes; single-quoted strings are raw.

### Formats are built in

```
let users = read("users.csv")              # format from the extension: list of records
let cfg   = read("config.toml")
read("log.txt") | lines | where .contains("ERROR") | count
users | where .active | sort .name | write("active.json")
```

- Formats in v1: JSON, JSON Lines, CSV, TSV, plain lines, TOML, YAML, `.env`.
- `read` and `write` infer the format from the extension and accept an explicit one: `read(p, "csv")`. `parse(text, "json")` and `to(value, "csv")` work on in-memory data.
- `|` passes the left value as the first argument of the right call. `.field` on its own is shorthand for a one-argument closure.
- List functions: `where`, `map`, `sort`, `group`, `uniq`, `count`, `first`, `last`, `take`, `skip`, `sum`, `min`, `max`, `join`, `flat`, `each`.
- Lines and CSV rows stream; a pipeline over a large file must not load it whole.
- CSV values stay strings unless the script converts them. No guessing of numbers or dates.

### Running commands

```
run cargo build --release --target {target}   # each word is one argv entry; {x} is always one argument
let r = run git rev-parse --abbrev-ref HEAD   # r.out, r.err, r.code, r.ok
let t = run! make test                        # `!`: do not fail on a nonzero exit
run curl -s {url} | parse("json") | .items    # stdout feeds the pipeline
run pg_dump {db} > {out_dir / "dump.sql"}     # redirect to a path value, handled by ok
run("git", "commit", "-m", msg)               # function form, same semantics
```

- `run` is a keyword. The rest of the line is lexed in command mode: whitespace-separated words, quotes, `{}` interpolation, and a trailing `|`, `>`, `>>` or `<` that returns to expression mode. A list interpolated with `{...args}` spreads into several arguments. Design the grammar so this is unambiguous and document it precisely.
- Never a shell. Spawn directly with an argv list on every OS; never through `sh` or `cmd.exe`. No word splitting, no glob expansion by accident; globbing is the explicit `glob()` function.
- A nonzero exit raises an error by default. The error carries the command, exit code and the last lines of stderr.
- Without capture, the child's stdout and stderr go to the terminal live. With `let r =`, they are captured.
- Options as a trailing block or named arguments, your choice: `cwd`, `env`, `stdin`, `timeout`.
- `--trace` prints each command before it runs. `--dry-run` prints commands without running them.
- On Windows, resolve `PATHEXT` so `run npm install` finds `npm.cmd`, and handle the argument-quoting rules for `.cmd`/`.bat` safely.

### Portable standard library

Scripts must not need coreutils. Provide at least:

- Files: `copy`, `move`, `remove`, `mkdir`, `exists`, `is_file`, `is_dir`, `glob`, `list_dir`, `stat`, `touch`, `tempdir`, `tempfile`, `chmod` (a no-op with a documented meaning on Windows).
- Paths: `/` joins, separators are normalised, `.parent`, `.name`, `.stem`, `.ext`, `.abs()`, `.relative_to()`. `home`, `cwd`, `script_dir`.
- Environment: `env.NAME`, `env.get(name, default)`, `os.name`, `os.arch`, `which(cmd)`.
- Text: `trim`, `split`, `replace`, `contains`, `starts_with`, `upper`, `lower`, regex `match`/`find`/`sub`, `pad`, `fmt`.
- Other: `hash(path_or_bytes, "sha256")`, `now()`, durations like `5s` and `2m`, `sleep`, `print`, `eprint`, `fail(msg)`, `exit(code)`, `prompt`, `confirm`.
- Network: `fetch(url)` for GET/POST with headers, returning status, headers and body. Use rustls, not OpenSSL, to keep the binary static.
- Archives: `zip`/`unzip` and `tar`/`untar` with gzip.

### Script CLI from the signature

```
# Convert a CSV export to JSON
fn main(input: path, --out: path = "out.json", --verbose: bool) { ... }
```

Positional and `--flag` parameters come from `main`'s signature. `--help` is generated from the signature and the comment above it. Bad arguments give a clear error and exit code 2. `#!/usr/bin/env ok` works on Unix.

### Errors

- Every error shows file, line, column and a source snippet with the span underlined.
- `try { } catch e { }`, and `?` after an expression to turn a failure into null.
- Uncaught errors exit nonzero. An uncaught command failure exits with the child's code.
- Error messages are part of the product. Suggest the fix when it is knowable, for example a misspelled name.

### Modules

`import "./lib.ok"` and `import "./lib.ok" as lib`, relative paths only. No package manager, no remote imports.

### Not in v1

An interactive shell or REPL beyond a minimal `ok -e 'expr'`, a package manager, user-defined types, async, and sandboxing. Reserve the syntax `task name allow [net, env "KEY"]` for a later permission system and reject it with "not implemented yet".

## Project mode

Inside a project, `ok` is the task runner for build and admin commands.

### Discovery and layout

```
myapp/
  ok.ok              # root tasks: build, test, dev
  ok/admin.ok        # namespace "admin"
  ok/db.ok           # namespace "db"
  ok/_lib.ok         # leading underscore: helpers, never a namespace
```

- `ok` walks up from the current directory to the nearest `ok.ok`. That directory is the project root.
- Tasks run with the project root as working directory. `invocation_dir` holds where the user was.
- Each file directly under `ok/` is a namespace named after the file. No nesting deeper than one level in v1.
- In a project file, `task` is public and `fn` is private. Top level allows declarations only: `task`, `fn`, `let` with constant expressions, `import`, `requires`, `env file`. Any side effect at top level is a parse-time error. Loading a project must never run code.
- `ok -C path task` runs in another project. `import "frontend/ok.ok" as fe` mounts a sub-project's tasks as namespace `fe`.

### Tasks

```
requires "0.3"
env file ".env"

# Generate code from the schemas
task gen sources ["schema/*.json"] outputs ["src/gen.rs"] {
  run codegen
}

# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}

# Wipe and reseed the local database
task reset-db confirm "This wipes the local database" {
  if env.get("APP_ENV", "dev") == "prod" { fail("refusing to run in prod") }
  ...
}
```

- `ok` alone lists tasks grouped by namespace, with the comment above each task as its description. `ok build --release` and `ok admin create-user bas --role admin` run them. `ok build --help` shows a task's parameters.
- `needs` runs dependencies first, each at most once per invocation. Detect cycles at load time.
- With `sources` and `outputs`, a task is skipped when every output exists and is newer than every source. `--force` overrides. A content-hash mode is optional; decide and log it.
- Modifiers: `confirm "text"` asks before running (`--yes` skips; without a terminal and without `--yes` it fails), `hidden` keeps a task out of the listing.
- `--env name` additionally loads `.env.name`.
- Task parameters use the same typed-signature CLI parsing as `main`.
- If the first argument is a path to an existing `.ok` file, `ok` runs it as a script instead of looking for a task.
- Keep reserved command names minimal so they never collide with task names: `trust`, `untrust` and `self`. Other built-ins sit under `self`: `ok self init`, `ok self completions zsh|bash|fish|powershell`, `ok self fmt`, `ok self check`, `ok self version`.

### Trust

A cloned repository's tasks are someone else's code. Running them needs approval.

- Listing tasks, `--help` and completions only parse files and need no approval.
- Running a task in an untrusted project prompts with the list of project files and a combined SHA-256, and offers view, trust or no.
- Trust is bound to the content of all project `.ok` files, not to the directory. When any of them changes, prompt again and offer a diff against the trusted version, so keep a copy of what was trusted.
- `ok trust --dir` trusts a directory regardless of content, for the user's own projects. `ok trust`, `ok untrust` and `ok trust --list` manage the store.
- The store lives in the platform config directory (`~/.config/ok/` on Linux, the equivalents on macOS and Windows).
- `OK_TRUST=1` or `--trust` skips the prompt for CI. Without a terminal and without either, fail with a clear message; never hang.
- Standalone scripts run by explicit path are not subject to trust.
- Imports may not resolve outside the project root. Refuse symlinks that escape it.
- State plainly in the docs that trust is not a sandbox.

### Bootstrap script

`ok self init` writes a starter `ok.ok` and two small wrapper scripts, `./ok` (POSIX sh) and `ok.cmd`/`ok.ps1`. The wrapper downloads the pinned `ok` release for the current OS and architecture into a user cache, verifies it against SHA-256 checksums pinned in the wrapper, and runs it. A contributor or CI job needs nothing preinstalled. The wrapper is the one place in this project where shell is acceptable; keep it short and readable.

## Milestones

Work through these in order. Each ends with all tests green on the three operating systems in CI, `SPEC.md` and `CHANGELOG.md` updated, and a short report to Bas. Stop after each milestone and wait for a go-ahead.

1. **M0, spec and examples.** Repository setup as described above. Write `SPEC.md` in full: lexical grammar, expression and statement grammar (EBNF), command mode, values and their operations, the standard library signatures, the CLI, project mode, trust. Write at least 15 example programs in `examples/`, each a real-world bash script, Makefile or small Python script rewritten in `ok`, with the original beside it. Cover: a release script, a log summary, CSV to JSON conversion, a multi-step build with code generation, a deploy with confirmation, a backup rotation, a git pre-commit hook, an API call that filters JSON. No interpreter code yet. This milestone is the design review; expect changes.
2. **M1, core interpreter.** Lexer, parser, evaluator, the value types, functions, closures, control flow, string interpolation, imports, and errors with source snippets. `ok file.ok` and `ok -e` work.
3. **M2, data.** All formats, `read`/`write`/`parse`/`to`, pipelines, the list functions, streaming for lines and CSV.
4. **M3, commands and the standard library.** `run` in both forms, redirects and pipes, capture, `--trace`, `--dry-run`, the file, path, environment, text, hash, fetch and archive functions. Windows command resolution and quoting.
5. **M4, script CLI.** `main` signatures, generated `--help`, argument errors, shebang.
6. **M5, project mode.** Discovery, namespaces, tasks with `needs`, `sources`/`outputs`, `confirm`, `hidden`, `--env`, the task listing, completions. From here the repo's own `ok.ok` is the real entry point for build, test, lint and release.
7. **M6, trust.** The trust store, prompts, diff on change, CI behaviour, import confinement.
8. **M7, distribution.** Release workflow building static binaries for `x86_64` and `aarch64` on Linux (musl), macOS and Windows, with checksums. `ok self init` and the bootstrap wrappers. An install section in the README. Tag `v0.1.0` locally and leave publishing to Bas.

### Tests

- Golden tests are the backbone: `tests/cases/name.ok` with `name.stdout`, optional `name.stderr` and `name.exit`. One command regenerates expectations, and every regenerated diff is reviewed before commit.
- Error messages are golden-tested too, including the snippet rendering.
- Every example from M0 runs in CI once the features it needs exist. Use small fixture commands built in the repo instead of system tools, so `run` tests behave identically on all platforms.
- Unit tests for the lexer's command mode, path normalisation, CSV edge cases (quotes, embedded newlines, BOM, CRLF) and Windows argument quoting.
- Fuzz the lexer and parser with `cargo-fuzz`; the parser must never panic on any input.
- A test on each OS proves that loading a project with a side effect at top level fails, and that an untrusted project cannot run a task without approval.

### Definition of done for v0.1.0

- All 15+ examples produce the same output on Linux, macOS and Windows.
- A fresh clone with only the `./ok` wrapper can build and test a sample project.
- The release binary is a single static file under 15 MB with no runtime dependencies.
- `SPEC.md` matches the implementation; nothing is implemented that the spec does not describe.

## Working rules

- **Spec first.** Change `SPEC.md` before changing behaviour, in the same commit or the one before.
- **Decide and log.** For anything this prompt leaves open, pick the option that makes scripts easier to read, record it in `DECISIONS.md` and move on. Do not stop for small choices.
- **Stop and ask** when a decision would contradict this prompt, change syntax already shown in an approved example, add a dependency with a non-permissive licence, or weaken the trust model or the no-shell rule.
- **Readability wins ties.** When two designs are equal, choose the one a person who has never seen `ok` reads correctly on the first try.
- **Portability is a feature, not a port.** No behaviour may depend on the host having a Unix shell or coreutils. If something cannot work on Windows, design it differently instead of adding a platform note.
- **Few dependencies.** Prefer well-maintained crates for formats, regex, TLS and archives; write the lexer, parser and evaluator by hand. No `unsafe` outside dependencies without a logged reason.
- **No scope creep.** Ideas that are not in this prompt go into `IDEAS.md`, not into the code.
- **Report honestly.** Each milestone report lists what was built, what was decided, what is known to be weak, and what you would change in the spec now that it is implemented.

Start with M0.
