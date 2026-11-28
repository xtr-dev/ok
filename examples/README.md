# Examples

Real-world bash scripts, Makefiles and Python scripts rewritten in ok. Each
directory holds the original next to the ok version, plus any sample data the
script reads. They test the language's design: if an ok version does not read
more clearly than its original, the language should change.

The test suite runs 15 of them (`crates/okrun/tests/examples_run.rs`) with
fake tools in place of `git`, `docker`, `psql` and the rest, so the results
are the same on every OS; the expected output is in `tests/examples/`.
`deploy`, `stale-bugs` and `install-tool` call fixed internet hosts and are
only parsed by the tests.

| Example | Original | Shows |
|---|---|---|
| [release](release/) | bash | `fn main` CLI, `run` capture and `run!`, regex, text edits, checksums |
| [log-summary](log-summary/) | bash (grep, cut, sort, uniq) | streaming `read \| lines`, `group`, `sort`, format specs |
| [csv-to-json](csv-to-json/) | Python | CSV with BOM and quotes, strings stay strings, `write` by extension |
| [codegen-build](codegen-build/) | Makefile + Python generator | project tasks, `needs`, `sources`/`outputs`, a `_` helper module, triple-quoted strings |
| [deploy](deploy/) | bash | `confirm` modifier, `tar`, `tempdir`, `fetch` health check |
| [backup-rotation](backup-rotation/) | bash | durations and times, `glob`, `sort` by mtime, `--max-age 14d` |
| [pre-commit](pre-commit/) | bash git hook | shebang, `{...list}` spread, `run!` for exit codes |
| [stale-bugs](stale-bugs/) | bash (curl + jq) | `fetch` with headers, JSON filtering, `not in` |
| [env-check](env-check/) | bash | `which` (PATHEXT on Windows), `env.has`, a script without `main` |
| [config-render](config-render/) | Python + PyYAML | YAML in, JSON out, recursive merge, nested `var` updates |
| [largest-files](largest-files/) | Python | `glob` with `exclude`, `stat`, `relative_to`, a `fn` with `return` |
| [docker-image](docker-image/) | Makefile | tasks with parameters, record destructuring, multi-segment words |
| [db-dump](db-dump/) | bash | `> {path}` redirect, piping command output into `lines` |
| [install-tool](install-tool/) | bash | `match` on `os.name`/`os.arch`, `fetch(to:)`, `hash`, `untar`, `chmod` |
| [tidy-names](tidy-names/) | bash | `move`, regex `sub`, `--dry-run` instead of a home-made `-n` flag |
| [test-matrix](test-matrix/) | Python | `r.duration`, `pad`, collecting results, exit codes |
| [wait-for](wait-for/) | bash | rest parameters, `--` on the command line, `?` on `fetch`, `run {...command}` |
| [monorepo](monorepo/) | Makefiles | namespaces (`ok/db.ok`), `_lib.ok`, mounting `frontend/ok.ok`, `env file`, `requires`, `hidden` |

Running one, once the interpreter exists:

```
ok examples/csv-to-json/csv-to-json.ok examples/csv-to-json/users.csv --out active.json
ok -C examples/codegen-build gen
```
