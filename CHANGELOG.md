# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- A juxtaposed pipeline stage (`| take 5`) takes exactly one argument, so
  `print(xs | count, ys | count)` passes two values to `print`.
- `range` is lazy and no longer limited to ten million numbers.

### Added

- M3 commands: `run`, `run!` and the function form; live, captured and
  piped output by position; `|` between commands and into commands;
  `>`, `>>` and `<` redirects; options `cwd`, `env`, `stdin`, `timeout`,
  `bytes`, `quiet`; failures carry the command, exit code and stderr tail
  and exit with the child's code; `--trace` and `--dry-run`; PATHEXT lookup
  on Windows.
- M3 standard library: files (`copy`, `move`, `remove`, `mkdir`, `exists`,
  `is_file`, `is_dir`, `glob`, `list_dir`, `stat`, `touch`, `tempdir`,
  `tempfile`, `chmod`), paths (`abs`, `relative_to`, `with_ext`, `parts`,
  `is_abs`, `home`, `cwd`, `script_dir`, `invocation_dir`), environment
  (`env.NAME`, `env.get/has/set/remove/all`, `which`), text (`trim`,
  `split`, `replace`, `contains`, `starts_with`, `ends_with`, `upper`,
  `lower`, `chars`, `repeat`, `pad`, regex `match`/`matches`/`find`/`sub`),
  time (`now`, `time`, `format`, `sleep`, `duration`), `prompt`, `confirm`,
  `hash`, `fetch`, `zip`/`unzip`/`tar`/`untar`.
- M2 data: `read`, `write`, `parse`, `to` and `stdin` for JSON, JSON Lines,
  CSV, TSV, lines, text, TOML, YAML (including merge keys and `yaml_all`),
  `.env` and bytes, with the format taken from the file name or given
  explicitly; CSV/TSV options `header` and `delimiter`, JSON `compact`,
  `append` for writes.
- Streaming: line- and row-oriented reads, `lines`, `range` and `stdin()` are
  lazy; `where`, `map`, `take`, `skip`, `uniq`, `flat`, `enumerate` and
  `lines` stream, and consumers stop reading early.
- List and record functions: `where`, `map`, `each`, `sort`, `reverse`,
  `group`, `uniq`, `count`, `first`, `last`, `take`, `skip`, `sum`, `min`,
  `max`, `any`, `all`, `join`, `flat`, `enumerate`, `keys`, `values`,
  `entries`, `from_entries`, `get`, `has`, `lines`.
- M1 core interpreter: lexer (with command mode), parser for the full
  grammar, and a tree-walking evaluator for values, operators, strings with
  interpolation and format specs, lists, records, paths, durations,
  functions with typed, named, default and rest parameters, closures,
  implicit `.field` closures, pipelines, `if`, `for`, `while`, `match`,
  `try`/`catch`, `?`, `??`, destructuring and imports.
- Errors with file, line, column, an underlined source snippet, notes, help
  with suggestions for misspelled names and habits from other languages, and
  a call trace.
- `ok SCRIPT [ARGS...]` and `ok -e SOURCE`; `args` for scripts.
- Core builtins: `print`, `eprint`, `str`, `int`, `float`, `type`, `len`,
  `fail`, `exit`, `path`, `range`, `floor`, `ceil`, `round`, `fmt`, `bytes`,
  `text`, and the `os` record.
- Golden tests (`tests/cases/`, regenerate with `OK_BLESS=1 cargo test --test
  golden`), lexer/parser/path/format unit tests, a random-input no-panic test,
  and cargo-fuzz targets for the lexer and parser.

- Repository setup: Cargo workspace (`okrun` package, `ok` binary), CC0
  licence, cargo-deny licence policy, cargo-about third-party notices, CI for
  format, clippy and tests on Linux, macOS and Windows, and a release workflow
  for six targets.
- `SPEC.md`: the full language specification draft for v0.1.0.
- `DECISIONS.md`: log of design decisions the brief left open.
- `examples/`: real-world scripts and Makefiles rewritten in ok, each next to
  the original.
- `ok.ok`: the repository's own build and admin tasks (runnable from M5; use
  the documented `cargo` commands until then).
- Placeholder `ok` binary that answers `ok self version`.
- `quiet` command option to hide a command's output unless it fails.
