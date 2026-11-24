# Ideas

Things that are deliberately **not** in v1. They are collected here instead of
in the code. Each needs a spec change and review before it is built.

- Short flags for script and task parameters (`--verbose` as `-v`).
- Choice types for parameters (`--role: "admin" | "user"`), shown in `--help`
  and completions.
- Content-hash mode for `sources`/`outputs` up-to-date checks.
- Running independent `needs` dependencies in parallel.
- Nested namespaces (`ok/db/seed.ok`) and reaching a mounted sub-project's
  namespaces.
- Passing arguments to dependencies: `needs [build(release: true)]`.
- Stderr redirection operators (`2>`, `2>&1`) in command mode.
- `finally` for `try`.
- Reading compressed data directly: `read("x.json.gz")`.
- Record and list patterns in `match` arms.
- A permission system for tasks (`task x allow [net, env "KEY"]`); the syntax
  is reserved.
- A REPL beyond `ok -e`.
- A lint in `ok self check` for `write(value, path)` called with the arguments
  swapped.
