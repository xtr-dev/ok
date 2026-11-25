# Ideas

Things that are deliberately **not** in v1. They are collected here instead of
in the code. Each needs a spec change and review before it is built.

- Short flags for script and task parameters (`--verbose` as `-v`).
- Choice types for parameters (`--role: "admin" | "user"`), shown in `--help`
  and completions.
- Content-hash mode for `sources`/`outputs` up-to-date checks.
- Nested namespaces (`ok/db/seed.ok`) and reaching a mounted sub-project's
  namespaces.
- `finally` for `try`.
- Questions (`prompt`, `confirm()`) from tasks that run in parallel, asked
  one at a time through the parent ok instead of failing.
- `--keep-going` for parallel dependencies: start the remaining independent
  tasks after a failure and report all failures at the end.
- Reading compressed data directly: `read("x.json.gz")`.
- Record and list patterns in `match` arms.
- A permission system for tasks (`task x allow [net, env "KEY"]`); the syntax
  is reserved.
- A REPL beyond `ok -e`.
- A lint in `ok self check` for `write(value, path)` called with the arguments
  swapped.
- Static detection of undefined names and wrong argument counts in
  `ok self check`.
- A syntax-tree based formatter (line breaking, spacing inside lines).
- A man page (`ok.1`), printed by `ok self man` and attached to releases.
