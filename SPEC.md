# The ok language specification

Version: draft for v0.1.0 (milestone M0, 2026-11-24).

This document is normative. When the implementation and this document
disagree, one of them is a bug; `SPEC.md` is changed first, in the same
commit as the behaviour change or the one before. Decisions behind the
choices made here are logged with dates in `DECISIONS.md`.

The keywords MUST, MUST NOT, SHOULD and MAY are used in their RFC 2119 sense.

Contents

1. Overview
2. Running ok
3. Lexical structure
4. Grammar
5. Values and types
6. Expressions
7. Statements and declarations
8. Pipelines
9. Commands (`run`)
10. Errors
11. Modules
12. The standard library
13. Data formats
14. Script CLI (`main`)
15. Project mode
16. Trust
17. Built-in commands (`ok self ...`)
18. Bootstrap wrappers
19. Portability rules
20. Reserved for later versions

---

## 1. Overview

`ok` is a dynamically typed scripting language and task runner shipped as a
single static binary. One script behaves the same on Linux, macOS and
Windows, on x86-64 and arm64.

```
# Summarise errors per component in a log file
fn main(log: path, --top: int = 5) {
  read(log)
    | lines
    | where .contains("ERROR")
    | map(line => line.split(" ")[2])
    | group(c => c)
    | entries
    | sort(e => len(e.value), desc: true)
    | take(top)
    | each(e => print("{e.key:<20} {len(e.value):>6}"))
}
```

Design rules that the rest of this document follows:

- **Readability wins ties.** A person who has never seen ok should read a
  script correctly on the first try.
- **Never a shell.** Commands are spawned directly from an argument list.
- **Loud failures.** A failing command, a missing field or a type mismatch is
  an error, not a silent empty value. `?`, `??`, `try` and `run!` opt out.
- **Same output everywhere.** Printed paths use `/`, written files use `\n`,
  sort orders and formats do not depend on the host.

## 2. Running ok

### 2.1 Invocation forms

```
ok [OPTIONS]                              list tasks of the current project (or print usage)
ok [OPTIONS] TASK [ARGS...]               run a project task
ok [OPTIONS] NAMESPACE TASK [ARGS...]     run a task from ok/NAMESPACE.ok
ok [OPTIONS] SCRIPT [ARGS...]             run a standalone script
ok [OPTIONS] -e SOURCE                    evaluate source text and print the result
ok trust | untrust ...                    manage the trust store (section 16)
ok self SUBCOMMAND ...                    built-in tools (section 17)
```

`SCRIPT` is recognised when the first non-option argument is the path of an
existing file **and** either ends in `.ok` or contains a path separator (`/`,
or `\` on Windows). Task names can contain neither, so the two never collide.
`ok deploy` is always a task; `ok ./deploy` and `ok deploy.ok` are scripts.

### 2.2 Global options

Global options MUST come before the task or script name. Everything after the
name belongs to the task or script.

| Option | Meaning |
|---|---|
| `-C DIR` | Discover the project starting from `DIR` instead of the current directory. |
| `-e SOURCE` | Evaluate `SOURCE` as a script; print the value of a final expression unless it is `null`. |
| `--trace` | Print every command (and every file-system mutation, see 12.1) to stderr before running it. |
| `--dry-run` | Print commands and file-system mutations instead of performing them (see 9.9). |
| `--yes` | Answer yes to every `confirm` modifier and `confirm()` call. |
| `--trust` | Run project tasks without a trust prompt, for this invocation only (same as `OK_TRUST=1`). |
| `--force` | Run tasks even when their `sources`/`outputs` say they are up to date. |
| `--env NAME` | Additionally load `.env.NAME` (section 15.7). |
| `-j N`, `--jobs N` | Run at most `N` (at least 1) task dependencies at the same time (15.5). The default is the number of CPUs ok may use; `-j1` runs them one at a time. Also written `-jN` and `--jobs=N`. |
| `--color WHEN` | `auto` (default), `always` or `never`. `NO_COLOR` set to a non-empty value means `never`. |
| `-h`, `--help` | Print usage. After a task or script name, `--help` prints that task's or script's help instead. |
| `--version` | Same as `ok self version`. |

When a task or script receives an argument that is an ok global option and
the task does not declare it, the error says so:

```
error: unknown flag --trace for task `build`
  = help: ok's own options go before the task name: ok --trace build
```

### 2.3 Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. |
| 1 | Runtime error, syntax error, load error, refused trust or confirmation. |
| 2 | Bad command-line usage: unknown task, unknown flag, missing or malformed argument. |
| N | An uncaught failure of a child command exits with the child's exit code N. |
| 124 | An uncaught command timeout. |
| 126 | An uncaught failure to start a program that was found (for example, not executable). |
| 127 | An uncaught "command not found". |
| 128+S | On Unix, an uncaught failure of a child killed by signal S. |
| 130 | Interrupted by Ctrl-C. |

`exit(code)` exits with any code from 0 to 255.

### 2.4 Shebang

A first line starting with `#!` is a comment like any other `#` line. On Unix,
`#!/usr/bin/env ok` makes an executable script; the kernel passes its path as
the first argument, which contains a separator and is therefore a script.

## 3. Lexical structure

### 3.1 Source text

Source files are UTF-8. A leading byte order mark is ignored. Line endings
are `\n` or `\r\n`; a lone `\r` is an error. Files conventionally end in
`.ok`.

### 3.2 Comments

`#` starts a comment that runs to the end of the line, except inside string
literals and except mid-word in command mode (9.2).

A **doc comment** is the block of consecutive comment lines directly above a
`fn` or `task` declaration, with no blank line in between. The text after
`#` and one optional space, joined with newlines, is the declaration's
description. The first line is its summary. A shebang line is never part of
a doc comment.

### 3.3 Newlines and statement termination

Statements are terminated by newlines. There are no semicolons; a `;` is a
syntax error with a hint.

A newline does **not** end a statement when:

1. it is inside `( )` or `[ ]` (but it is significant directly inside
   `{ }` blocks and is ignored inside `{ }` record literals), or
2. the last token on the line is a binary operator (`+ - * / % == != < <= > >=
   and or not in ?? | = += -= =>`), a `,`, `.`, `:`, or an opening `( [ {`, or
3. the first token on the next non-blank line is `|`, `.` followed by an
   identifier (a method chain), `else`, `catch`, or `finally`.

Rule 3 allows the pipeline style used throughout this document:

```
let names = users
  | where .active
  | map(.name)
```

### 3.4 Identifiers and keywords

```
ident      = (letter | "_") { letter | digit | "_" }
letter     = "a".."z" | "A".."Z"
digit      = "0".."9"
```

Identifiers are ASCII. Keywords:

```
and as break catch continue else false finally fn for if import in let
match not null or requires return run task true try var while
```

Contextual words, keywords only in the position shown: `needs`, `sources`,
`outputs`, `confirm`, `hidden`, `allow` (task modifiers), `env file` (project
top level) and `with` (command options, 9.6).

A keyword is accepted as a field name after `.` (`r.match(re)`) and as a
record key before `:` (`{if: 1}`).

Task names (15.3) additionally allow `-` inside: `reset-db`. They are lexed
as task names only after `task`, inside `needs [...]`, and on the command
line.

### 3.5 Literals

**Null and booleans:** `null`, `true`, `false`.

**Integers:** `0`, `42`, `1_000_000`, `0x1F`, `0o755`, `0b1010`. Underscores
may separate digits. A literal that does not fit in a signed 64-bit integer is
a syntax error.

**Floats:** `1.5`, `0.25`, `1e9`, `2.5e-3`. Digits are required on both sides
of `.`: `.5` and `5.` are not floats.

**Durations:** an integer or float immediately followed by a unit, optionally
repeated in decreasing unit order: `250ms`, `5s`, `1.5s`, `2m`, `1h30m`,
`7d`. Units: `ms`, `s`, `m`, `h`, `d` (24 hours). Negative durations come
from the unary minus operator.

**Strings.** Double-quoted strings interpret escapes and interpolation:

| Escape | Meaning |
|---|---|
| `\n` `\r` `\t` `\0` | newline, carriage return, tab, NUL |
| `\\` `\"` | backslash, double quote |
| `\{` `\}` | literal braces |
| `\u{1F600}` | Unicode scalar value, 1 to 6 hex digits |

Any other escape is a syntax error. `{expr}` interpolates an expression;
`{expr:spec}` interpolates with a format spec (12.6). A `:` inside nested
brackets or a nested string does not start a spec. A lone `}` is literal.

Single-quoted strings are raw: no escapes, no interpolation. They cannot
contain `'`. They are the natural form for regexes and Windows paths:
`'\d+\.\d+'`, `'C:\Users'`.

Both forms may span lines. **Triple-quoted strings** `"""..."""` (escapes and
interpolation) and `'''...'''` (raw) are for multi-line text and are
dedented: if the opening quotes are followed directly by a newline, that
newline is removed; the common leading whitespace of all non-blank lines,
including the line of the closing quotes, is removed from every line; if the
closing quotes are on a line of their own, the newline before them is kept
and the indentation before them is removed. Dedenting applies to the
literal's source text; interpolated values are inserted afterwards
unchanged.

```
let msg = """
  Hello {name},
  your build finished.
  """
# msg == "Hello Bas,\nyour build finished.\n"
```

**Lists:** `[1, 2, 3]`, **records:** `{name: "ok", "content-type": "json"}`,
see 6.

### 3.6 Operators and punctuation

```
+  -  *  /  %  ==  !=  <  <=  >  >=  =  +=  -=  =>  ??  ?  |  .  ...  ,  :  ( ) [ ] { }  !
```

`!` appears only in `run!` (9.1). `not` is the boolean negation operator; the
lexer reports `!x`, `&&` and `||` with a hint to use `not`, `and`, `or`.

### 3.7 Command mode

After the keyword `run` or `run!` followed by whitespace, the lexer switches
to command mode until the end of the command. Command mode is specified in
section 9.2.

## 4. Grammar

EBNF notation: `{ x }` zero or more, `[ x ]` optional, `|` alternation,
`"x"` a token. `NL` is a significant newline (3.3). The grammar of command
mode is in 9.2.

### 4.1 Files

```
file          = { NL } { item { NL } } EOF
item          = declaration | statement           (* scripts *)
declaration   = fn_decl | task_decl | let_decl | var_decl | import_decl
              | requires_decl | env_file_decl
import_decl   = "import" STRING [ "as" ident ]
requires_decl = "requires" STRING
env_file_decl = "env" "file" STRING
```

Which items are allowed where:

| File kind | Allowed at top level |
|---|---|
| script (run by path or `-e`) | statements, `fn`, `let`, `var`, `import` |
| library (any imported file that is not a project file) | `fn`, constant `let`, `import` |
| project file (`ok.ok`, `ok/*.ok`, and files they import) | `task`, `fn`, constant `let`, `import`, `requires`, `env file` |

Violations are load-time errors with the offending span underlined.
`requires` and `env file` are only allowed in the root `ok.ok`.

### 4.2 Functions and tasks

```
fn_decl     = "fn" ident "(" [ params ] ")" block
params      = param { "," param } [ "," ]
param       = ident [ ":" type ] [ "=" expr ]
            | "--" flag_name [ "," short_flag ] [ ":" type ] [ "=" expr ]
            | "..." ident [ ":" type ]
flag_name   = ident { "-" ident }                   (* only after "--" *)
short_flag  = "-" letter                  (* no space after "-"; 14.1 *)
type        = base_type [ "?" ] | choice_type
base_type   = "any" | "null" | "bool" | "int" | "float" | "string" | "bytes"
            | "path" | "duration" | "time" | "record" | "fn"
            | "list" [ "[" type "]" ]
choice_type = STRING { "|" STRING } [ "|" "null" ]   (* plain strings, 5.11 *)

task_decl   = "task" task_name [ "(" [ params ] ")" ] { modifier } block
task_name   = ident { "-" ident }
modifier    = "needs" "[" [ task_ref { "," task_ref } [ "," ] ] "]"
            | "sources" list_literal
            | "outputs" list_literal
            | "confirm" STRING
            | "hidden"
            | "allow" "[" ... "]"                    (* reserved; rejected, see 20 *)
task_ref    = task_name [ "." task_name ]
```

A flag parameter `--dry-run` is bound in the body as `dry_run`: dashes
become underscores. Two parameters that map to the same name are an error.
A flag may declare a one-letter short form for the command line,
`--verbose, -v: bool` (14.1); in code it is still passed by its long name.

### 4.3 Statements

```
block       = "{" { NL } [ statement { NL { NL } statement } ] { NL } "}"
statement   = let_decl | var_decl | fn_decl | assignment
            | if_expr | for_stmt | while_stmt | "break" | "continue"
            | "return" [ expr ] | expr
let_decl    = "let" pattern "=" expr
var_decl    = "var" pattern "=" expr
pattern     = ident
            | "[" [ pattern { "," pattern } ] [ "," "..." ident ] "]"
            | "{" [ field_pat { "," field_pat } ] "}"
field_pat   = ident [ ":" pattern ]
assignment  = place ( "=" | "+=" | "-=" ) expr
place       = ident { "." ident | "[" expr "]" }
for_stmt    = "for" for_vars "in" expr_nostruct block
for_vars    = pattern [ "," pattern ]
while_stmt  = "while" expr_nostruct block
```

### 4.4 Expressions

Precedence from lowest to highest. All binary operators are left-associative
except comparisons, which do not chain (`a < b < c` is an error).

| Level | Operators |
|---|---|
| 1 | `\|` (pipeline) |
| 2 | `??` |
| 3 | `or` |
| 4 | `and` |
| 5 | `not` (prefix) |
| 6 | `==` `!=` `<` `<=` `>` `>=` `in` `not in` |
| 7 | `+` `-` |
| 8 | `*` `/` `%` |
| 9 | `-` (prefix) |
| 10 | postfix: call `( )`, field `.name`, index `[ ]`, catch `?` |

Mixing `??` with `and` or `or` without parentheses is an error, because
readers disagree on how it groups.

```
expr        = pipeline
pipeline    = coalesce { "|" stage }
stage       = ident_path [ "(" [ args ] ")" ] [ juxt_arg ]      (* 8.2 *)
            | run_expr
            | coalesce
juxt_arg    = arg                                    (* exactly one *)
coalesce    = or_expr { "??" or_expr }
or_expr     = and_expr { "or" and_expr }
and_expr    = not_expr { "and" not_expr }
not_expr    = "not" not_expr | comparison
comparison  = additive [ comp_op additive ]
comp_op     = "==" | "!=" | "<" | "<=" | ">" | ">=" | "in" | "not" "in"
additive    = term { ( "+" | "-" ) term }
term        = unary { ( "*" | "/" | "%" ) unary }
unary       = "-" unary | postfix
postfix     = primary { call | field | index } [ "?" ]
call        = "(" [ args ] ")"                   (* no whitespace before "(" *)
field       = "." ( ident | keyword )
index       = "[" expr "]"
args        = arg { "," arg } [ "," ]
arg         = expr | ident ":" expr | "..." expr
primary     = literal | ident | "(" expr ")" | list_literal | record_literal
            | if_expr | match_expr | try_expr | fn_expr | arrow_fn
            | implicit_fn | run_expr
list_literal   = "[" [ elem { "," elem } [ "," ] ] "]"
elem           = expr | "..." expr
record_literal = "{" [ entry { "," entry } [ "," ] ] "}"
entry          = ( ident | keyword | STRING ) ":" expr | ident | "..." expr
if_expr     = "if" expr_nostruct block [ "else" ( if_expr | block ) ]
match_expr  = "match" expr_nostruct "{" { NL } { arm ( NL | "," ) { NL } } "}"
arm         = pat_alt [ "if" expr ] "=>" ( block | expr )
pat_alt     = match_pat { "|" match_pat }
match_pat   = literal | "-" number | "_" | ident
            | "[" [ list_mpat ] "]"
            | "{" [ field_mpat { "," field_mpat } [ "," ] ] "}"
list_mpat   = pat_alt { "," pat_alt } [ "," [ "..." ident ] ]
            | "..." ident
field_mpat  = ( ident | keyword | STRING ) [ ":" pat_alt ]
try_expr    = "try" block ( "catch" [ ident ] block [ "finally" block ]
                          | "finally" block )
fn_expr     = "fn" "(" [ params ] ")" block
arrow_fn    = ( ident | "(" [ ident { "," ident } ] ")" ) "=>" coalesce
implicit_fn = "." ident ...                        (* 6.9 *)
run_expr    = ( "run" | "run!" ) command           (* 9.2 *)
            | ( "run" | "run!" ) "(" [ args ] ")"  (* function form, no whitespace *)
```

`expr_nostruct` is `expr` in which a `{` at the outermost level starts the
following block instead of a record literal. Wrap a record literal in
parentheses to use it there: `if x == ({a: 1}) { ... }`. In a `match` arm and
after `=>` of an arm, `{` starts a block.

A call's `(` must follow the callee with no whitespace: `f(x)` is a call,
`f (x)` is not (and in a pipeline stage it is a juxtaposed argument).

## 5. Values and types

### 5.1 Kinds

| Kind | Literal or source | Display (`print`, `"{x}"`) |
|---|---|---|
| `null` | `null` | `null` |
| `bool` | `true`, `false` | `true` |
| `int` | `42` | `42` |
| `float` | `1.5` | `1.5`; integral floats keep `.0`: `2.0` |
| `string` | `"text"` | the text itself |
| `bytes` | `bytes("x")`, `read(p, "bytes")` | `<bytes 12>` |
| `list` | `[1, 2]` | `[1, 2]`; inside lists and records, strings are quoted (`["a"]`) and paths show as `path("a/b")` |
| `record` | `{a: 1}` | `{a: 1, "b-c": 2}` |
| `path` | `path("a/b")` | `a/b` (always `/`) |
| `duration` | `1h30m` | `1h30m`, `1.5s`, `250ms` |
| `time` | `now()`, `time("2026-11-24")` | RFC 3339 with local offset, `2026-11-24T21:05:00+01:00` |
| `fn` | `fn(x) {...}`, `x => ...`, `.name` | `<fn name>` |

`type(x)` returns the kind name as a string.

`time` is not in the original value list of the brief; it is added because
`stat().modified`, `now()` and backup rotation need instants that subtract to
durations. See DECISIONS.md, 2026-11-24.

### 5.2 Mutability and value semantics

All values behave as immutable values. Lists and records are never shared
mutably: `var b = a` followed by `b[0] = 1` leaves `a` unchanged. The
implementation copies on write.

`let` bindings cannot be reassigned. `var` bindings can, including by
element or field: `xs[0] = 1`, `cfg.port = 8080`, `cfg["log-level"] = "debug"`
and `xs += [4]` rebind the `var` to an updated copy. Assigning through a
`let` binding is an error that suggests `var`.

Declaring a name twice in the same scope is an error. An inner scope may
shadow an outer name.

### 5.3 Truthiness

There is none. `if`, `while`, `and`, `or`, `not`, `where` predicates and
match guards require a `bool`. Any other value is a type error with a hint:

```
error: `if` needs a bool, got a list
  = help: to test for an empty list use `if len(files) > 0` or `if files != []`
```

### 5.4 Equality and ordering

`==` and `!=` compare structurally; records are equal when they have the
same keys with equal values, in any order. Values of different kinds are unequal,
except that an `int` and a `float` compare numerically (`1 == 1.0`). Comparing
a `path` with a `string` is always `false`.

`<`, `<=`, `>`, `>=` are defined for two numbers, two strings (by Unicode code
point), two durations, two times, two paths (by their `/` display form) and
two bools (`false < true`). Lists compare lexicographically element by
element. Anything else is a type error.

`x in list` tests membership with `==`. `key in record` tests for a key.
`sub in string` tests for a substring.

### 5.5 Numbers

`int` is 64-bit signed. Overflow in `+ - * %` and in unary minus is an error,
never a wrap. `float` is IEEE 754 binary64.

| Operation | Result |
|---|---|
| `int + - * int` | `int` |
| `int / int` | `float` (`7 / 2 == 3.5`) |
| `int % int` | `int`, sign of the dividend |
| mixed `int` and `float` | `float` |
| division or `%` by zero | error, for ints and floats |

An operation that would produce NaN or infinity is an error. Integer division
is `floor(a / b)`, `ceil`, `round` (halves round away from zero: `round(2.5)
== 3`, `round(-2.5) == -3`), or `int(x)` (truncation toward zero).

### 5.6 Strings

Strings are UTF-8. `len(s)` counts Unicode scalar values. `s[i]` returns the
scalar at index `i` as a one-character string; negative indexes count from
the end. `+` concatenates two strings; adding a string and a non-string is a
type error that suggests interpolation. Iterating a string with `for` is an
error that suggests `.chars()` or `.lines()`.

### 5.7 Lists and records

Lists hold any values. `xs[i]` with a negative `i` counts from the end; an
out-of-range index is an error (`xs[i]?` gives `null`). `+` concatenates two
lists. Spread `...xs` inserts the elements into a list literal or the
arguments of a call.

Records map string keys to values and preserve insertion order. `r.key` and
`r["key"]` read a field; a missing key is an error that lists close matches
(`r.key?` gives `null`). In a record literal, `{name}` is short for
`{name: name}` and `...r` copies the fields of `r`; later keys win. Iterating
a record with one variable yields its keys; with two, key and value.

### 5.8 Paths

A `path` is a file-system path, stored normalised:

- On input, `/` is a separator on every OS; `\` is also a separator on
  Windows only.
- Repeated separators collapse, `.` components are removed, and a `..`
  directly after a normal component cancels it (`a/b/../c` is `a/c`). No
  file-system access is involved; symlinks are not resolved.
- A trailing separator is dropped. The empty path is `.`.
- Windows drive prefixes (`C:/`) and UNC prefixes (`//server/share/`) are
  kept.

Paths display with `/` on every OS so that output is identical everywhere.
When a path leaves ok (a command argument, a redirect target, a file-system
call) it is converted to the native form.

`path / x` joins, where `x` is a string or path. If `x` is absolute the result
is `x`. `string / string` is a type error with the hint to start from
`path(...)`. Path properties and methods are listed in 12.2.

### 5.9 Durations and times

A `duration` is a signed span with nanosecond precision. A `time` is an
instant (UTC internally, displayed in local time).

| Operation | Result |
|---|---|
| `duration ± duration` | `duration` |
| `duration * number`, `duration / number` | `duration` |
| `duration / duration` | `float` |
| `time ± duration` | `time` |
| `time - time` | `duration` |

Properties: `d.seconds` (float), `d.millis` (int); `t.unix` (int seconds),
`t.year`, `t.month`, `t.day`, `t.hour`, `t.minute`, `t.second` (ints, local
time). `t.format(pattern, utc: false)` formats with strftime-style patterns
(12.7).

### 5.10 Functions

Calls nest at most 2000 deep; deeper recursion is an error, not a crash.

Functions are values. `fn name(...) { }` declares a named function; inside a
block it is visible in the whole block (declarations are hoisted), at the top
level in the whole file. `fn(...) { }` is an anonymous function. Closures
capture variables by reference: a closure sees later assignments to a `var`
it captured.

A function returns the value of `return expr`, or else the value of the last
statement of its body if that is an expression, or else `null`.

### 5.11 Type annotations

Parameters of `fn` and `task` may carry types (4.2). They are checked when
the function is called; a mismatch is an error at the call site that names
the parameter:

```
error: argument `input` of `convert` must be a path, got an int
```

Conversions applied while checking:

| Annotation | Also accepts |
|---|---|
| `float` | `int` (converted) |
| `path` | `string` (converted) |
| `list[T]` | a list whose every element satisfies `T` |
| `T?` | `null` |
| `any` | everything (same as no annotation) |

Defaults are evaluated at each call in the function's scope, then checked
against the annotation: `--out: path = "out.json"` gives a `path`. On the
command line, annotations drive parsing (14.3).

A **choice type** lists the strings a parameter accepts:

```
task deploy(env: "staging" | "production", --role: "admin" | "user" = "user") { ... }
```

A value satisfies `"a" | "b"` when it is a string equal to one of the
choices (case-sensitive); anything else is the usual call-site error:

```
error: argument `role` of `grant` must be one of "admin", "user", got "root"
```

A final `| null` also accepts `null`, like `T?`:
`--region: "eu" | "us" | null`. The choices are plain string literals (no interpolation), at least
one, without duplicates. Choice types are allowed wherever a type annotation
is, including `list["a" | "b"]`. A default that is a string literal must be
one of the choices; otherwise it is a load-time error. ok has no type
annotations outside parameters, so `let` takes none.

## 6. Expressions

### 6.1 Operators

Defined in 4.4 and 5. `and` and `or` short-circuit. `a ?? b` evaluates to
`a` unless `a` is `null`, in which case it evaluates `b`.

### 6.2 Catch operator `?`

`expr?` evaluates the postfix chain to its left (the primary and every
`.field`, `[index]` and `(call)` after it); if that raises any error, the
result is `null` instead. It does not catch `exit()` or Ctrl-C.

```
let cfg  = read("config.toml")? ?? {}     # missing or malformed file: {}
let port = cfg.port? ?? 8080              # missing field: 8080
let sha  = (run git rev-parse HEAD).out?  # not a git repository: null
```

`?` may only end a postfix chain. There is no optional chaining: `a?.b` is a
syntax error whose hint suggests `a.b?` (catch the whole chain) or `(a?).b`.

### 6.3 Calls and named arguments

`f(a, b, name: value, ...rest)` passes positional arguments, named arguments
(which bind to `--` parameters, 7.2) and spread lists. Positional arguments
must come before named ones.

### 6.4 Method syntax

`x.name(args)` is a method call. It resolves as follows:

1. If `x` is a record (or module namespace) with a field `name` whose value
   is a function, that function is called with `args`.
2. Otherwise `name` is looked up as a function in scope (user function,
   import or built-in) and called as `name(x, args)`.

So `s.trim()` is `trim(s)` and `users.where(.active)` is `where(users,
.active)`. `x.name` without parentheses is a field or property, never a
call.

### 6.5 `if` as an expression

`if` yields the value of the taken branch's block, or `null` when no branch is
taken. A block's value is its last statement if that is an expression.

### 6.6 `match`

```
let exe = match os.name {
  "windows" => "ok.exe"
  "linux" | "macos" => "ok"
  other => fail("unsupported OS {other}")
}
```

Arms are tried in order. A pattern is a literal (compared with `==`), `_`
(matches anything), an identifier (matches anything and binds it), or a list
or record pattern. A guard `if cond` must be a bool. If no arm matches, it is
an error that shows the value. `match` yields the value of the chosen arm.
`match` does not require the arms to cover every value.

List and record patterns look like the destructuring patterns of `let`
(7.1), but they test instead of fail:

```
let summary = match fetch(url, check: false) {
  {status: 200, body} => "ok: {len(body)} bytes"
  {status: 301 | 302, headers: {location}} => "moved to {location}"
  {status} if status >= 500 => "server error {status}"
  _ => "other"
}

fn describe(xs) {
  match xs {
    [] => "empty"
    [only] => "one: {only}"
    [first, ..._] => "starts with {first}"
  }
}
```

- A **list pattern** `[p1, p2]` matches a list of exactly that length whose
  elements match `p1`, `p2` in order. `[p1, ...rest]` matches a list of at
  least that many elements and binds the remaining elements, as a list, to
  `rest`; `..._` ignores them. `[]` matches only the empty list.
- A **record pattern** `{key: p}` matches a record that has every listed key
  with a value matching its pattern; other keys are allowed. `{key}` is
  short for `{key: key}`: the key must exist, and its value is bound to
  `key`. `{}` matches any record. Keys are written like record literal keys:
  an identifier, a keyword, or a string (`{"content-type": ct}`).
- A list pattern never matches a value that is not a list, and a record
  pattern never matches a value that is not a record; neither is an error.
- Patterns nest, and `|` alternatives may appear inside them
  (`{status: 200 | 201}`).
- A name may be bound only once in a pattern, and every alternative of a
  `|` must bind the same names, so that the guard and the body can rely on
  them. Both are syntax errors.
- Names bound by a pattern are visible only in that arm's guard and body.

### 6.7 `try` / `catch`

```
let cfg = try {
  read("config.toml")
} catch e {
  eprint("using defaults: {e.message}")
  {}
}
```

`try` yields the value of its block, or, if an error is raised inside, the
value of the `catch` block with `e` bound to the error record (10.2). The
binding name is optional. `fail(e)` re-raises a caught error unchanged.

A `try` needs a `catch` block, a `finally` block, or both (in that order):

```
let tmp = tempdir()
try {
  run! tar -xzf {archive} -C {tmp}
  build(tmp)
} finally {
  remove(tmp / "lock")
}
```

The `finally` block runs after the `try` block, and after the `catch` block
if one ran, however they end:

- normally, with a value;
- with an error, including one raised in `catch` (a `try` without `catch`
  does not catch anything; the error continues after `finally`);
- with `return`, `break` or `continue` passing through it;
- with `exit(code)`.

When the `finally` block completes normally, the pending outcome stands: the
`try` expression yields the value of the `try` (or `catch`) block, and the
value of `finally` is discarded, or the pending error, `return`, `break`,
`continue` or `exit` continues outwards. When the `finally` block itself
raises an error (or calls `exit`), that replaces the pending outcome; an
error raised this way carries a note with the message of the error it
replaced. `return`, `break` and `continue` that would leave a `finally`
block are syntax errors, because they would silently discard the pending
outcome; inside a loop or function within the `finally` block they are
allowed as usual.

Ctrl-C ends ok without running `finally` blocks (2.3, code 130).

### 6.8 Closures

`fn(x, y) { ... }` is an anonymous function with a block body. `x => expr`
and `(x, y) => expr` are single-expression closures; the body extends over
`??` level and below but not over `|`, so

```
xs | map(x => x * 2) | sum
xs | map x => x * 2 | sum         # same thing
```

To pipe inside an arrow body, parenthesise it.

### 6.9 Implicit closures (`.field`)

A `.` followed by an identifier in prefix position (where an operand is
expected) refers to the field, property or method of an implicit argument
`it`. The nearest enclosing **argument boundary** becomes a one-argument
closure over the whole expression at that boundary. Argument boundaries are:
a call argument, a pipeline-stage argument, a list element, a record value,
the right side of `let`, `var` or an assignment, and an arrow or `fn` body.

```
users | where .active                       # where(users, it => it.active)
users | where .age >= 18 and .active        # it => it.age >= 18 and it.active
users | sort .name                          # it => it.name
lines | where .contains("ERROR")            # it => it.contains("ERROR")
let by_name = .name                         # a function
```

`.` alone is not an implicit closure; use `x => x`. Implicit closures do not
nest: inside the arguments of a call that is itself inside an implicit
closure, `.x` starts a new closure at the inner boundary.

## 7. Statements and declarations

### 7.1 Bindings

`let pattern = expr` and `var pattern = expr` bind names. Patterns
destructure lists and records:

```
let [major, minor, patch] = version.split(".")
let [first, ...rest] = args
let {out, code} = run! git status --porcelain
let {name, owner: {login}} = repo       # nested record pattern
```

A list pattern without `...rest` requires the exact length. A record pattern
requires every named field.

### 7.2 Function parameters

- Positional parameters `name: T = default` are passed by position.
- Flag parameters `--name: T = default` are keyword-only: callers pass them
  as named arguments `f(x, name: value)`; on the command line they are
  `--name value`. A `bool` flag without a default defaults to `false`.
- One rest parameter `...name: T` collects the remaining positional
  arguments into a list whose elements must satisfy `T`.
- Parameters without a default are required, except `bool` flags (default
  `false`), `T?` flags (default `null`) and `list[T]` flags (default `[]`).

### 7.3 Loops

```
for f in glob("src/**/*.rs") { ... }
for i, line in lines { ... }        # index and element
for key, value in cfg { ... }       # key and value
for key in cfg { ... }              # keys
while not ready() { sleep(1s) }
```

`break` and `continue` apply to the innermost loop. `for` over a lazy list
(13.2) consumes it.

### 7.4 Top level of a script

A script's top-level statements run in order. Then, if the script declares
`fn main`, the command-line arguments are parsed against its signature
(section 14) and `main` is called; its return value is ignored. A script
without `main` gets its arguments as the list of strings `args`, and
`--help` prints the script's leading comment.

`return` at top level is an error; use `exit(code)`.

## 8. Pipelines

### 8.1 Semantics

`a | f(x, y)` means `f(a, x, y)`. `a | f` means `f(a)`. The left value is
passed as the first argument of the right call.

### 8.2 Stages

The right side of `|` (a **stage**) is one of:

1. **A call** `name(args)` or `module.name(args)`: the left value is
   prepended to the arguments.
2. **A bare name** `name`: called with the left value as its only argument.
3. **A juxtaposed call** `name arg`: the name followed by whitespace and
   exactly one argument. Equivalent to `name(left, arg)`. A comma after it
   belongs to the surrounding call or list, so `print(xs | count, ys |
   count)` passes two values to `print`; a stage that needs more arguments
   uses parentheses: `| sort(.size, desc: true)`. Juxtaposition is allowed
   only in pipeline stages, and its argument cannot begin with an operator
   other than `.`, `not` or `-` followed directly by a digit. `| take 5`,
   `| sort .name`, `| join ", "`, `| where not .done`.
4. **A command** `run ...`: the left value becomes the command's stdin
   (9.5).
5. **Any other expression** that evaluates to a function: it is called with
   the left value. `| .items` (an implicit closure) and `| (x => x * 2)`
   are stages of this kind.

Anything else, for example `a | count > 3`, is an error:

```
error: a pipeline stage must be a call
  = help: to compare the result, wrap the pipeline: (a | count) > 3
```

### 8.3 Laziness

`read` of a line-oriented or row-oriented format, `lines`, `range`,
`stdin()` and the stdout of a piped command produce **lazy** values (13.2).
The stages `where`, `map`, `take`, `skip`, `uniq`, `flat`, `enumerate` and
`lines` stream: given a lazy list they return one. `each`, `count`, `sum`,
`min`, `max`, `first`, `last`, `any`, `all`, `join` and `write` consume a
lazy list element by element, and `first`, `any`, `all` and `take` stop
early. A `for` loop also pulls one element per iteration. Everything else,
including `sort`, `group`, `reverse`, indexing, `len` and `print`,
materialises the value first.

## 9. Commands (`run`)

### 9.1 Forms

```
run cargo build --release            # command mode
let r = run git rev-parse HEAD       # captured
run! make test                       # do not fail on a nonzero exit
run("git", "commit", "-m", msg)      # function form
run!("git", "diff", "--quiet")       # function form, no failure
```

`run` and `run!` are keywords. `run` immediately followed by `(` (no space)
is the function form, whose arguments are ordinary expressions (strings,
paths, numbers, or lists spread with `...`) plus the named options of 9.6. In
the function form a single string is never split: `run("make test")` looks
for a program named `make test`, and the error says how to fix that.

`run` followed by whitespace starts command mode.

### 9.2 Command mode lexing

A command is a sequence of **words** separated by spaces or tabs, followed by
optional redirects, an optional `with` clause, and a terminator.

```
command     = word { word } { redirect } [ "with" record_literal ] terminator
word        = segment { segment }                (* no whitespace between segments *)
            | "{..." expr "}"                    (* spread; must be the whole word *)
segment     = bare | dq_string | sq_string | "{" expr "}"
bare        = bare_char { bare_char }
redirect    = ( ">" | ">>" | "<" | "2>" | "2>>" ) target
            | "2>&1"
target      = word
terminator  = NL | EOF | "|" | ")" | "}" | comment
```

Rules:

1. **Bare characters** are any characters except whitespace, `"`, `'`, `{`,
   `}`, `|`, `>`, `<`, `(`, `)` and `;`. Backslash is an ordinary character in
   bare words (`C:\tools\x.exe` works); there are no escapes in bare words.
   `*`, `?`, `~`, `$` and `[ ]` are literal: there is no globbing, no tilde
   expansion and no variable expansion.
2. **Quoted segments** follow the string rules of 3.5. `""` is an empty
   argument.
3. **Interpolation** `{expr}` evaluates an expression. Inside it the lexer is
   in expression mode with balanced brackets, so `{f({a: 1})}` works.
4. A word made of several segments is concatenated into **one argument**:
   `--target={t}` and `"{dir}/out"` are each one argument. `{x}` is always
   exactly one argument, even if `x` contains spaces or is empty.
5. `{...xs}` must be a whole word. It spreads a list into zero or more
   arguments.
6. `#` at the start of a word begins a comment. Mid-word it is literal
   (`http://host/#top`).
7. A word that is exactly `&&`, `||`, `&` or `;`, or contains an unquoted `;`,
   is an error explaining that ok commands are not shell, with the fix
   (separate `run` lines, `and`/`or`, or quoting).
8. An unquoted `(` is an error suggesting quotes or the function form.
9. A first word containing `=` (`FOO=1 make`) is an error suggesting
   `with {env: {FOO: "1"}}`.
10. An unquoted `)` or `}` that does not close something opened inside the
    command ends the command. This is what makes `(run git rev-parse
    HEAD).out` and `if x { run make }` work.
11. A `\` as the last character of a line continues the command on the next
    line.
12. `$(` and backticks are errors suggesting `{run ...}` interpolation or a
    `let`.
13. `2>`, `2>>` and `2>&1` are stderr redirects only where a word would
    start with an unquoted `2` directly followed by `>`. `2 > x` passes the
    argument `2` and redirects stdout; `x2>y` passes `x2`. `2>&1` is written
    without spaces and is followed by a blank or the end of the command;
    `2>&` followed by anything other than `1` is an error.
14. Each of stdin, stdout and stderr takes at most one redirect: `> a > b`
    and `2> err.log 2>&1` are errors. A redirect target starting with `&`
    (`>&2`, `2> &1`) is an error; quote it to name such a file.

Conversion of interpolated values to arguments:

| Value | Argument |
|---|---|
| `string` | as is |
| `path` | native form (`\` on Windows) |
| `int`, `float` | display form |
| `bool` | `true` / `false` |
| `null`, `list`, `record`, `bytes`, `fn`, `duration`, `time` | error (lists: hint to use `{...x}`) |

In a multi-segment word, a path is converted with its display form (`/`), as
in string interpolation.

### 9.3 Program resolution

The first argument names the program.

- If it contains a path separator, it is resolved relative to the command's
  working directory.
- Otherwise each directory of `PATH` is searched in order. On Windows the
  search also tries each extension in `PATHEXT` (default `.COM;.EXE;.BAT;.CMD`)
  when the name has no extension, so `run npm install` finds `npm.cmd`. The
  current directory is **not** searched on any OS.
- Not found is an error: `command not found: cargo (searched PATH)`. On Unix
  a file without execute permission is skipped during the search.

### 9.4 Process creation

Commands are spawned directly with an argument vector. ok never invokes
`sh`, `bash`, `cmd.exe` or PowerShell to interpret a command line.

On Windows, a program's arguments are joined into one command line using the
quoting rules of the Microsoft C runtime (`CommandLineToArgvW`). Batch files
(`.bat`, `.cmd`) are started by `CreateProcess`, which hands them to the
batch interpreter; their arguments are quoted with the stricter batch rules
and an argument that cannot be passed to a batch file safely (for example
one containing a newline, or `%` together with characters the batch parser
treats specially) is refused with an error instead of being passed in a form
the batch parser could misread. This uses the Rust standard library's
batch-file quoting, which was hardened for CVE-2024-24576.

The child inherits ok's environment (plus `env` options, 9.6), and the
working directory is the project root in project mode or ok's working
directory for scripts, unless `cwd` is given.

### 9.5 Input, output and capture

Whether output is captured is decided by **where the `run` expression
stands**, never by what the command does:

| Position | stdout | stderr | Value |
|---|---|---|---|
| expression statement (value unused), including the last statement of a block | terminal, live | terminal, live | result with `out`/`err` `null` |
| anywhere else (`let r = run ...`, argument, interpolation, `return`) | captured | captured | result record |
| left of `\|` (piped) | streamed into the pipeline | terminal, live | lazy text of stdout |
| with `> target` / `>> target` | written / appended to the file | by position | as above |
| with `2> target` / `2>> target` | by position | written / appended to the file | as above; a captured `err` is `""` |
| with `2>&1` | by position | merged into stdout | as above; a captured `err` is `""` |

The **result record** has fields `out` (string), `err` (string), `code`
(int), `ok` (bool, `code == 0`), `cmd` (list of strings) and `duration`. In
captured output, all trailing `\n` and `\r\n` are removed from `out` and `err`
(as in shell `$(...)`); with the option `bytes: true` they are raw bytes,
unmodified. Captured output that is not valid UTF-8 is an error suggesting
`bytes: true`.

Stdin is inherited from ok unless given:

- `< target` reads a file.
- `value | run cmd`: a string, bytes or lazy text is written to stdin; a list
  of strings is written one per line; for other values it is an error.
- `stdin:` option (9.6).

Pipelines of commands, `run a | run b`, connect a's stdout to b's stdin
directly, without buffering in ok.

If a pipeline stops reading a command's stdout early (`run yes | take 3`),
ok closes the pipe and terminates the command; its exit status is then
ignored.

A redirect target is one command-mode word: `> build.log`, `> "my file.txt"`,
`> {out_dir / "dump.sql"}`. Its value must be a string or path; it is
converted to the native form (`\` on Windows), like a whole-word path
argument. Combining `>` with piping stdout is an error. The directory of a
`>`, `>>`, `2>` or `2>>` target is created if it is missing.

`2>&1` sends stderr to wherever stdout goes, whatever the order of the
redirects (unlike a shell, `2>&1 > f` and `> f 2>&1` mean the same): to the
terminal (ok's own stdout) when live, into the captured `out` when captured
(interleaved as the command wrote it, through one pipe), into the pipeline
when piped (`run make 2>&1 | where .contains("warning")`), and into the file
of `>`/`>>`. With `quiet`, merged output is captured and discarded like any
other output.

The function form `run(...)` has no redirects and no stderr options; a
command that needs them is written in command mode, where `{...argv}`
spreads a computed argument list (9.2).

### 9.6 Options

In command mode, options follow the words as `with` and a record literal. In
the function form they are named arguments.

```
run npm ci with {cwd: "frontend", timeout: 10m}
run cargo test with {env: {RUST_LOG: "debug", NO_COLOR: null}}
run("npm", "ci", cwd: "frontend", timeout: 10m)
```

| Option | Type | Meaning |
|---|---|---|
| `cwd` | path | Working directory of the child. Relative to the project root or the script's working directory. |
| `env` | record | Variables to set for the child; a `null` value removes the variable. |
| `stdin` | string, bytes, path | Data (string or bytes) or a file (path) for stdin. |
| `timeout` | duration | Kill the child after this long; raises a `timeout` error (exit 124 if uncaught). |
| `bytes` | bool | Captured `out`/`err` are bytes, unmodified. |
| `quiet` | bool | Hide the command's output. It is captured and discarded instead of shown; on failure the error still includes the last lines of stderr. |

A command that should run without printing anything uses `quiet`:

```
run cargo fetch with {quiet: true}       # nothing on the terminal unless it fails
```

`with` is special only when followed by `{`; elsewhere it is an ordinary
word (`run echo with care` passes `with` and `care`).

### 9.7 Failure

`run` raises a `command` error when the command exits nonzero, cannot be
started, or times out. The error carries the command line, the exit code and
the last 20 lines of stderr when stderr was captured. With `2>&1` and
captured output, it carries the last 20 lines of the merged output instead,
shown as `output (last N lines)` and also given as the error record's
`stderr`. When stderr went to a file (`2>`, `2>>`, or `2>&1` together with
`>`), the error names the file: `stderr was written to build.log`.
Uncaught, ok exits with the child's exit code (2.3).

`run!` never raises for a nonzero exit; the result record carries the code.
It still raises when the program cannot be found or started.

### 9.8 Tracing

With `--trace`, each command is printed to stderr before it starts, prefixed
with `+ `, each argument quoted if it contains whitespace or quotes, and with
the working directory if it is not the default:

```
+ cargo build --release --target x86_64-unknown-linux-musl
+ (in frontend) npm ci
+ make test < input.txt > build.log 2>&1
```

Redirects are shown after the arguments in the order stdin, stdout,
stderr, with targets in display form (`/`).

### 9.9 Dry run

With `--dry-run`, every command and every mutating file-system or network
operation (12.1, 12.9) is printed in trace format with the prefix `~ ` and
not performed. A skipped command yields `{out: "", err: "", code: 0, ok: true}`.
Reads still happen. Because skipped steps produce empty results, a dry run
shows the plan of a script, not a guaranteed preview of every branch.

## 10. Errors

### 10.1 Rendering

Every error shows the kind, message, location and a source snippet with the
span underlined, followed by optional notes, a call trace and a help line.
Errors without a source location (command-line and some load errors) show
only `error: message` and their notes and help.

```
error[name]: unknown name `relase`
  --> ok.ok:14:7
   |
14 |   if relase { args += ["--release"] }
   |      ^^^^^^ not defined here
   |
   = help: did you mean `release`?
```

A command error adds the command and the tail of its stderr:

```
error[command]: command failed with exit code 101
  --> release.ok:22:3
   |
22 |   run cargo test --workspace
   |   ^^^^^^^^^^^^^^^^^^^^^^^^^^
   = command: cargo test --workspace
   = stderr (last 3 lines):
     test parser::tests::empty ... FAILED
     failures:
         parser::tests::empty
```

When an error passes through function calls, a short call trace (innermost
first, at most 8 frames) follows the snippet, one line per frame:
`= in \`check\`, called at deploy.ok:12:3`. Identical consecutive frames, as
produced by recursion, are shown once with `(repeated)`. Notes are prefixed
`note:` and the help line `help:`. Colors are used on terminals
(2.2). Golden tests render without color.

### 10.2 Error records

A caught error `e` is a record:

| Field | Type | Present |
|---|---|---|
| `kind` | string | always: `syntax`, `name`, `type`, `value`, `index`, `key`, `arg`, `io`, `format`, `command`, `timeout`, `net`, `fail`, `trust` |
| `message` | string | always |
| `file`, `line`, `column` | string, int, int | when the error has a source location |
| `cmd`, `code`, `stderr` | list, int, string | command errors |
| `status` | int | `net` errors with an HTTP response |

`fail(msg)` raises an error of kind `fail` at the call site. `fail(e)` with an
error record re-raises it.

### 10.3 Suggestions

Error messages are part of the product. When a fix is knowable, the help line
gives it. Required suggestions:

- misspelled variable, function, field, task, namespace or flag (edit
  distance at most 2, or a case-only difference): `did you mean ...?`
- habits from other languages: `&&`, `||`, `!x`, `;`, `elif`, `elsif`,
  `function`, `def`, `None`, `nil`, `True`, `False`, `===`;
- shell habits in command mode (9.2 rules 7, 8, 9, 12);
- string used as a path in `/` (5.8), non-bool condition (5.3), list
  interpolated without spread (9.2), `let` reassigned (5.2), side effect at
  project top level (15.2), ok options after the task name (2.2).

## 11. Modules

```
import "./lib.ok"             # all exported names into scope
import "./lib.ok" as lib      # names under lib.
import "frontend/ok.ok" as fe # in project mode: mount a sub-project (15.8)
```

- The path is a string literal, relative to the directory of the importing
  file. Absolute paths and URLs are errors. There is no search path, package
  manager or remote import.
- An imported file is a **library** (4.1): it may contain only `fn`,
  constant `let` and `import`. Loading it never runs code.
- Exported names are all top-level `fn` and `let` names not starting with
  `_`.
- `import` without `as` brings every exported name into scope; a clash with a
  local declaration or another import is a load-time error. With `as`, the
  module is a namespace value; `lib.f(x)` calls its function.
- Each file is loaded once per run. Import cycles are a load-time error that
  shows the cycle.
- In project mode, imports may not resolve outside the project root (16.6).

## 12. The standard library

Signatures use the parameter syntax of 4.2; `= v` shows a default. All
functions also work in method form (6.4) and as pipeline stages (8.2).
Relative paths given to file functions are resolved against the working
directory (the project root in project mode).

### 12.1 Files

| Function | Returns | Notes |
|---|---|---|
| `read(p: path, format: string? = null, --gzip: bool? = null)` | value | Format from the extension or `format` (13); a `.gz` file is decompressed (13.1.1). |
| `write(value, p: path, format: string? = null, --append: bool, --gzip: bool? = null)` | `null` | Creates parent directories. A `.gz` file is compressed (13.1.1). Writes `\n` line endings. Streams lazy values. When the value is a short string that looks like a file name (`"out.txt"`) and the path has no extension, the arguments are almost certainly swapped, and `write` refuses with a hint; `ok self check` warns about literal arguments that look swapped before the script runs (17.1). |
| `copy(src: path, dst: path)` | `null` | Files or directories (recursive). `dst` is the exact destination, never "into". Overwrites files. |
| `move(src: path, dst: path)` | `null` | Rename; falls back to copy and remove across file systems. |
| `remove(p: path)` | `bool` | File or directory tree. `false` if nothing existed. Refuses the file-system root, the home directory and the project root. |
| `mkdir(p: path)` | `path` | Creates parents; no error if it exists. |
| `exists(p: path)` | `bool` | |
| `is_file(p: path)`, `is_dir(p: path)` | `bool` | |
| `glob(pattern: string, --root: path = cwd, --hidden: bool, --exclude: list[string] = [])` | `list[path]` | 12.1.1. |
| `list_dir(p: path = cwd)` | `list[path]` | Entries joined to `p`, sorted by name. |
| `stat(p: path)` | record | `{size: int, modified: time, created: time?, is_file, is_dir, is_symlink: bool, mode: int?}`; `mode` is `null` on Windows. |
| `touch(p: path)` | `null` | Create empty or update the modification time. |
| `tempdir()` | `path` | A new directory, removed when the script exits (also on error). |
| `tempfile(--ext: string = "")` | `path` | A new empty file, removed at exit. |
| `chmod(p: path, mode: int)` | `null` | Sets Unix permission bits, `chmod(p, 0o755)`. On Windows there are no permission bits, and `chmod` does nothing and succeeds. |
| `stdin(format: string? = null)` | value | ok's own stdin, lazily, parsed as `format` (default `text`). |

The mutating functions `write`, `copy`, `move`, `remove`, `mkdir`, `touch`,
`chmod`, `zip`, `unzip`, `tar` and `untar` are printed by `--trace` and
skipped by `--dry-run`.

#### 12.1.1 Glob patterns

`*` matches within one path component, `?` one character, `[abc]` and
`[a-z]` a character class, `**` zero or more whole components, `{a,b}`
alternatives. Patterns use `/` on every OS. Matching is case-sensitive on
every OS. Names starting with `.` are skipped, at every level, unless the
pattern has a component that starts with `.` (as in `.*` or `**/.env*`) or
`hidden: true` is given. Results are sorted by their `/`
display form, so the order is the same everywhere. A pattern that matches
nothing returns `[]`.

The pattern is matched against paths relative to `root`; the results are
joined to `root` (so with the default root they are relative to the working
directory and usable directly). `exclude` patterns are matched the same way;
an excluded directory is not descended into, which keeps `**` fast in trees
with `node_modules` or `target`.

### 12.2 Paths

| Member | Returns | Example |
|---|---|---|
| `path(s: string)` | `path` | `path("src/main.rs")` |
| `p.parent` | `path` | `path("a/b.txt").parent == path("a")`; the parent of a root is itself |
| `p.name` | `string` | `"b.txt"` |
| `p.stem` | `string` | `"b"` (`"archive.tar"` for `archive.tar.gz`) |
| `p.ext` | `string` | `"txt"`, without the dot; `""` if none |
| `p.abs()` | `path` | joined to the working directory and normalised |
| `p.relative_to(base: path)` | `path` | lexical; error if `p` is not under `base` |
| `p.with_ext(ext: string)` | `path` | `path("a.csv").with_ext("json")` |
| `p.parts()` | `list[string]` | |
| `p.is_abs()` | `bool` | |

Values: `home` (the user's home directory), `cwd` (ok's working directory:
the project root in project mode), `script_dir` (directory of the file the
running code is in), `invocation_dir` (where the user ran `ok`).

### 12.3 Environment and system

| Member | Returns | Notes |
|---|---|---|
| `env.NAME` | `string` | Error if unset, with a hint to use `env.get`. |
| `env.get(name: string, default = null)` | value | |
| `env.has(name: string)` | `bool` | |
| `env.set(name: string, value: string)` | `null` | Affects `env.NAME` and every command started afterwards. ok keeps the change itself and passes it to children; the operating-system environment of the ok process is not modified. |
| `env.remove(name: string)` | `null` | |
| `env.all()` | `record` | |
| `os.name` | `string` | `"linux"`, `"macos"`, `"windows"`, or another OS name in lowercase |
| `os.arch` | `string` | `"x86_64"`, `"aarch64"`, ... |
| `os.exe` | `string` | `".exe"` on Windows, `""` elsewhere |
| `which(cmd: string)` | `path?` | Same search as 9.3. |
| `args` | `list[string]` | Script arguments when there is no `main`. |

Environment variable names are case-insensitive on Windows and
case-sensitive elsewhere. `get`, `has`, `set`, `remove` and `all` are methods
of `env`; read a variable with such a name through `env.get("get")`.

### 12.4 Text

| Function | Returns | Notes |
|---|---|---|
| `len(x)` | `int` | strings (scalar values), bytes, lists, records |
| `trim(s)`, `trim_start(s)`, `trim_end(s)` | `string` | Unicode whitespace |
| `split(s, sep: string? = null, --limit: int? = null)` | `list[string]` | No `sep`: split on runs of whitespace, dropping empty parts. |
| `lines(x)` | `list[string]` | Split on `\n` and `\r\n`; a final empty line is dropped. On a list of strings, splits each and flattens. Lazy on lazy input. |
| `chars(s)` | `list[string]` | |
| `replace(s, old: string, new: string)` | `string` | All occurrences, literal. |
| `contains(x, item)` | `bool` | Substring for strings, membership for lists, key for records. |
| `starts_with(s, prefix)`, `ends_with(s, suffix)` | `bool` | |
| `upper(s)`, `lower(s)` | `string` | Unicode. |
| `repeat(s, n: int)` | `string` | |
| `pad(x, width: int, --fill: string = " ", --align: string = "left")` | `string` | `align` is `"left"`, `"right"` or `"center"`. `x` is converted with `str`. |
| `fmt(x, spec: string)` | `string` | 12.6 |
| `str(x)` | `string` | Display form (5.1). |
| `int(x)`, `float(x)` | number | From strings (error if malformed), numbers (`int` truncates), bools. |
| `bytes(s: string)`, `text(b: bytes)` | | UTF-8 encode / decode (error on invalid UTF-8). |
| `type(x)` | `string` | |

Regular expressions use the syntax of the Rust `regex` crate (RE2-like:
linear time, no backreferences). Write them in raw strings.

| Function | Returns | Notes |
|---|---|---|
| `s.match(re)` | `record?` | First match `{text, start, end, groups: list, named: record}` or `null`. Method form only, because `match` is a keyword. |
| `matches(s, re)` | `bool` | Whether `re` matches anywhere in `s`. |
| `find(s, re)` | `list[record]` | All non-overlapping matches. |
| `sub(s, re, replacement)` | `string` | Replace all matches. A string replacement may use `$1`, `${name}` and `$$`; a function replacement receives the match record. |

### 12.5 Lists and records

All take the list first, so they read well in pipelines. A `key` or `pred`
argument is a function, usually an implicit closure.

| Function | Returns | Notes |
|---|---|---|
| `where(xs, pred)` | list | Elements for which `pred` returns `true` (must be bool). |
| `map(xs, f)` | list | |
| `each(xs, f)` | `null` | Calls `f` for its effect. |
| `sort(xs, key = null, --desc: bool)` | list | Stable. Without `key`, sorts by value (5.4). |
| `reverse(xs)` | list | |
| `group(xs, key)` | record | Keys are `str(key(x))`, in order of first appearance; values are lists. |
| `uniq(xs, key = null)` | list | Keeps the first of each distinct value or key. Does not require sorted input. |
| `count(xs, pred = null)` | `int` | |
| `first(xs, pred = null)`, `last(xs, pred = null)` | value | `null` if there is none. |
| `take(xs, n: int)`, `skip(xs, n: int)` | list | |
| `sum(xs, key = null)` | number | `0` for an empty list. Durations sum to a duration. |
| `min(xs, key = null)`, `max(xs, key = null)` | value | `null` for an empty list. |
| `any(xs, pred)`, `all(xs, pred)` | `bool` | |
| `join(xs, sep: string = "")` | `string` | Elements converted with `str`; lists, records, bytes are errors. |
| `flat(xs)` | list | One level. |
| `range(n)`, `range(start, end, step = 1)` | list | `end` exclusive, lazy, so `range(1_000_000_000)` costs nothing until consumed. |
| `enumerate(xs)` | list | `[{index, value}, ...]` |
| `keys(r)`, `values(r)` | list | |
| `entries(r)` | list | `[{key, value}, ...]` |
| `from_entries(xs)` | record | Inverse of `entries`. |
| `get(r, key: string, default = null)` | value | |
| `has(r, key: string)` | `bool` | |

### 12.6 Format specs

`fmt(x, spec)` and `"{x:spec}"` use this subset of Python's format
mini-language:

```
spec   = [ [ fill ] align ] [ "0" ] [ width ] [ "," ] [ "." precision ] [ type ]
align  = "<" | ">" | "^"
type   = "d" | "f" | "e" | "x" | "X" | "o" | "b" | "%"
```

Strings default to left alignment, numbers to right. Examples: `"{n:>6}"`,
`"{ratio:.1%}"`, `"{bytes:,}"`, `"{x:08.3f}"`, `"{code:x}"`.

### 12.7 Time

| Function | Returns | Notes |
|---|---|---|
| `now()` | `time` | |
| `time(s: string)` | `time` | RFC 3339, or `YYYY-MM-DD` (local midnight). |
| `format(t, pattern: string, --utc: bool)`, usually `t.format(...)` | `string` | strftime patterns: `%Y %m %d %H %M %S %z %:z %a %b %j %%` and the rest of the usual set. |
| `sleep(d: duration)` | `null` | |
| `duration(s: string)` | `duration` | Parses literal syntax, `"1h30m"`. |

### 12.8 Output, input and control

| Function | Notes |
|---|---|
| `print(...xs)` | Display forms joined by spaces, then `\n`, to stdout. If stdout has been closed (output piped into a program that stopped reading), the script ends quietly with exit code 0. |
| `eprint(...xs)` | The same to stderr. |
| `fail(msg)` | Raise a `fail` error (10.2). |
| `exit(code: int = 0)` | Exit after cleanup (temp files). |
| `prompt(msg: string, --default: string? = null)` | Read a line from the terminal. Without a terminal: the default, or an error if there is none. |
| `confirm(msg: string, --default: bool = false)` | Ask yes/no. With `--yes`: `true`. Without a terminal and without `--yes`: an error. |

### 12.9 Hashing, network, archives

| Function | Returns | Notes |
|---|---|---|
| `hash(x: path \| bytes, algo: string = "sha256")` | `string` | Lowercase hex. `algo`: `sha256`, `sha512`, `sha1`, `md5`. A string argument is an error asking for `path(s)` or `bytes(s)`, because it is ambiguous. Files are hashed streaming. |
| `fetch(url: string, --method = "GET", --headers: record = {}, --body = null, --json = null, --timeout: duration = 30s, --to: path? = null, --check: bool = true, --bytes: bool)` | record | 12.9.1 |
| `zip(src: path \| list[path], dst: path)` | `null` | |
| `unzip(archive: path, dst: path)` | `null` | |
| `tar(src: path \| list[path], dst: path)` | `null` | gzip-compressed when `dst` ends in `.gz` or `.tgz` |
| `untar(archive: path, dst: path)` | `null` | gzip detected from content |

Archive members are stored with `/` paths relative to the source's parent
directory (for a directory `src`, members are `src/...`). Extraction refuses
members whose path is absolute or escapes `dst` ("zip slip"), and does not
create symlinks that point outside `dst`. Unix permission bits are stored and
restored where the OS has them.

#### 12.9.1 fetch

`fetch` performs an HTTP(S) request using rustls with the operating system's
certificate store, follows up to 10 redirects, and honours `HTTPS_PROXY`,
`HTTP_PROXY` and `NO_PROXY`. It returns:

```
{status: int, ok: bool, headers: record, body: string | bytes | null, url: string}
```

Header names are lowercase. `body` is a string, or bytes with `bytes: true`,
or `null` when `to:` streamed it to a file. `json:` serialises a value as the
request body and sets `content-type: application/json`. With `check: true`
(the default), a status of 400 or more raises a `net` error carrying the
status and the start of the body. A request that is not `GET` or `HEAD`, and
any `to:` download, is a mutating operation for `--dry-run`.

## 13. Data formats

### 13.1 Format selection

`read(p)`, `write(v, p)` and `stdin()` choose a format from the file name;
an explicit `format` argument overrides it. `parse(text, format)` and
`to(value, format)` convert in memory (`to` returns a string, or bytes for
`bytes`).

| Format | Extensions | `read` gives | `write` accepts |
|---|---|---|---|
| `json` | `.json` | any value | any JSON-compatible value; pretty, 2-space indent, final newline |
| `jsonl` | `.jsonl`, `.ndjson` | lazy list of values | list; one compact JSON value per line |
| `csv` | `.csv` | lazy list of records of strings | list of records (or lists with `header: false`) |
| `tsv` | `.tsv` | lazy list of records of strings | as csv |
| `lines` | (explicit only) | lazy list of strings | list; one item per line |
| `text` | `.txt`, `.log`, `.md`, anything unknown | lazy string | string as is; a list of strings one per line |
| `toml` | `.toml` | record | record |
| `yaml` | `.yaml`, `.yml` | any value | any value |
| `env` | `.env`, and names starting with `.env.` | record of strings | record of scalars |
| `bytes` | (explicit only) | bytes | bytes |

`read` of a `text` file that is not valid UTF-8 is an error suggesting
`read(p, "bytes")`. `to(v, "json", compact: true)` and `write(..., compact:
true)` write JSON on one line.

#### 13.1.1 Compressed files

A file whose name ends in `.gz` (in any letter case) is gzip-compressed:
`read` decompresses it and `write` compresses it. The format comes from the
name without `.gz`, by the table above:

```
let events = read("events.jsonl.gz")          # jsonl, lazy, decompressed as it is read
write(report, "report.json.gz")               # json, compressed
read("server.log.gz") | lines | where(l => "ERROR" in l) | count
```

- `data.gz` has no inner extension and is `text`. An explicit `format`
  replaces only the format; the file is still decompressed, so
  `read("blob.bin.gz", "bytes")` gives the decompressed bytes.
- `gzip: true` or `gzip: false` overrides the name, for example
  `read("x.gz", "bytes", gzip: false)` for the compressed bytes as stored,
  or `write(v, "upload.dat", gzip: true)`.
- Reading streams for the lazy formats (13.2): decompression happens as the
  data is consumed, in bounded memory. A file of several gzip members one
  after another (as produced by appending) is read as one stream.
- `write(..., append: true)` to a compressed file adds a new gzip member,
  which `read` joins with the existing data. For CSV and TSV the header is
  written only when the file was empty, as for uncompressed files.
- Data that is not valid gzip is an `io` error at the point of reading:
  `cannot read x.json.gz: not valid gzip data (...)`.
- Written files are identical on every OS: the gzip header carries no file
  name and no time, and the compression level is fixed.

`stdin()`, `parse` and `to` do not decompress.

### 13.2 Lazy values

`read` of `jsonl`, `csv`, `tsv`, `lines` and `text` (also compressed,
13.1.1), `stdin()`, `range`, and the stdout of a piped command are lazy:
the data is produced as it is consumed. A lazy value is indistinguishable from a normal one except for
when reading happens and when read or parse errors are raised (at the point
of consumption, with the location of the consuming expression and the line
number in the data).

A lazy value streams through the pipeline that consumes it directly. It is
**materialised** (read completely into an ordinary list or string) as soon as
it is bound to a name, stored in a list or record, passed to a function that
does not stream (any user function, and the builtins not listed in 8.3),
printed, interpolated or compared. So

```
read("big.log") | lines | where(l => "ERROR" in l) | count   # bounded memory
let all = read("big.log") | lines                             # whole file in memory
```

Streaming stages stop pulling as soon as they have what they need, so
`read(p) | lines | first` reads one line, and `range(1_000_000_000) | take 3`
produces three numbers.

### 13.3 JSON

Numbers without a fraction or exponent that fit in 64 bits become `int`,
others `float`. Objects become records in document order. Writing: paths and
times become strings (display form; times in UTC RFC 3339), durations become
strings in literal syntax, bytes and functions are errors. Duplicate keys in
input: the last wins.

### 13.4 CSV and TSV

- The first row is the header; each further row becomes a record keyed by the
  header. With `header: false`, rows are lists of strings.
- All values are strings. ok never guesses numbers, booleans or dates;
  convert explicitly with `int()`, `float()`, `time()`.
- Reading accepts a UTF-8 BOM, `\n` and `\r\n` line endings, RFC 4180 quoting
  with `""` escapes, and newlines inside quoted fields. A row with a
  different number of fields than the header is an error with its line
  number. Empty lines are skipped. Options: `delimiter: ","`, `header: true`.
- Writing: the header is the keys of the first record. A later record
  missing a key writes an empty field; a record with an extra key is an
  error. Values are written with `str`, `null` as an empty field; lists,
  records and bytes are errors. Fields are quoted only when needed. Lines end
  in `\n`.
- TSV is tab-separated with no quoting; on write, a value containing a tab or
  newline is an error.

### 13.5 TOML

TOML 1.0. Offset date-times become `time`; local dates, local times and
local date-times become strings. Writing requires a record.

### 13.6 YAML

YAML 1.2 core schema: only `true`/`false` are booleans (not `yes`/`no`/`on`),
`null`/`~`/empty are null. A file with several documents is an error that
suggests `parse(text, "yaml_all")`, which returns the list of documents.
Anchors and aliases are resolved. Merge keys (`<<: *defaults`), common in
CI and compose files, are applied: the merged mapping's keys are added unless
the mapping already has them.

### 13.7 .env

One `KEY=value` per line; blank lines and `#` comments are ignored; an
optional leading `export ` is accepted. Values may be unquoted (trailing
whitespace trimmed, ` #` starts a comment), single-quoted (literal), or
double-quoted (escapes `\n`, `\t`, `\"`, `\\`; may span lines). There is no
variable expansion: `${HOME}` stays as written. Writing quotes values that
need it.

## 14. Script CLI (`main`)

### 14.1 Signature to CLI

```
#!/usr/bin/env ok
# Convert a CSV export to JSON.
#
# Reads the export, keeps the active rows and writes them sorted by name.
fn main(
  input: path,                  # CSV file to read
  --out: path = "out.json",     # where to write the JSON
  --verbose: bool,              # print each row
) { ... }
```

- Positional parameters become positional arguments in order; a rest
  parameter takes the remaining ones.
- Flag parameters become `--name value` or `--name=value`. A `bool` flag is
  `--name` (true); a bool flag whose default is `true` is turned off with
  `--no-name`. A `list[T]` flag repeats: `--tag a --tag b`.
- `--` ends flag parsing; `-h` and `--help` print help only before it.
- Arguments are checked against the signature before any of the script's
  code runs.

**Short flags.** A flag parameter may declare a short form, one ASCII letter
written after the long name and a comma: `--verbose, -v: bool`,
`--out, -o: path = "out.json"`. Short letters are case-sensitive.

- A switch (`bool` flag) is `-v`. Switches combine: `-vq` is `-v -q`, and
  `-vh` asks for help.
- A flag that takes a value is `-o VALUE`, with the value as the next
  argument; it does not combine with other letters, and `-oVALUE` or
  `-o=VALUE` is an argument error that shows the right form. A `list[T]` flag
  repeats: `-t a -t b`.
- Before `--`, an argument that starts with `-` followed by an ASCII letter
  is a short flag (or a group of switches). `-`, `-5` and `-1.5` are values.
  A value that starts with `-` and a letter is passed after `--`, or as the
  value of a flag (`--pattern -x`), which always takes the next argument.
- These are load-time errors: a short form on a positional or rest
  parameter, a short form that is not a single letter, the same letter
  twice in one signature, a short form for a `bool` flag whose default is
  `true` (such a switch is only turned off, with `--no-name`), and the
  letters `h`, `C` and `e`, which are ok's own options (2.2): `-h` always
  prints help, and `-C`/`-e` are kept free so a short option means the
  same before and after the task name.
### 14.2 Help

`--help` (and `-h`) prints:

```
Convert a CSV export to JSON.

Reads the export, keeps the active rows and writes them sorted by name.

Usage: ok csv-to-json.ok <input> [--out <path>] [--verbose]

Arguments:
  <input>          CSV file to read

Options:
  --out <path>     where to write the JSON [default: out.json]
  --verbose        print each row
  -h, --help       print this help
```

The description is the doc comment above `main` (or, for a script without
`main`, the first comment block of the file after a shebang). A parameter's
help is the comment at the end of its line. A default that is a literal is
shown as its value; any other default is shown as its source text
(`[default: env.get("SRC", "/var/lib/app/data")]`), because printing help
never runs code.

When any parameter has a short form, the Options column shows it first and
indents the other flags to line up with the long names:

```
Options:
  -o, --out <path>   where to write the JSON [default: out.json]
  -v, --verbose      print each row
      --force        overwrite the output
  -h, --help         print this help
```

The usage line always uses the long names, which say what they do.

A flag with a choice type (5.11) shows its choices as the value placeholder,
in the usage line and under Options: `--role <admin|user>`. A positional
parameter with a choice type keeps its name in the usage line and adds
`[choices: staging, production]` to its help under Arguments.

### 14.3 Parsing values

| Type | Accepted text |
|---|---|
| `string`, `any`, none | anything |
| `int` | decimal integer, optional sign |
| `float` | decimal number |
| `bool` | flags only (presence) |
| `path` | anything, converted with `path()` and resolved against `invocation_dir` |
| `duration` | literal syntax: `90s`, `1h30m` |
| `T?` | as `T`; omitted means `null` |
| `list[T]` | rest parameter or repeated flag |
| `"a" \| "b"` | exactly one of the choices; anything else is an argument error that lists them and suggests the closest |

### 14.4 Argument errors

Missing, unknown or malformed arguments print one error line, a hint, and the
usage line to stderr, and exit with code 2:

```
error: unknown flag --verbos
  = help: did you mean --verbose?
usage: ok csv-to-json.ok <input> [--out <path>] [--verbose]
```

A value outside a choice type names the choices and suggests the closest:

```
error: --role expects one of "admin", "user", got "admn"
  = help: did you mean "admin"?
```

### 14.5 Completion

`ok self complete` (17) offers, after a task name: the task's flags
(`--name`), `--help`, the short forms (`-v`) when the word being completed
starts with a single `-`, and, where a value is expected, the choices of a
choice type: after a flag with a choice type (`--role <TAB>`, `-r <TAB>`,
also written `--role=<TAB>`), and for the next positional parameter when it
has a choice type. Completion never runs project code.

## 15. Project mode

### 15.1 Discovery

`ok` walks up from the current directory (or `-C DIR`) to the nearest
directory containing a file named `ok.ok`: the **project root**. Tasks run
with the project root as working directory; `invocation_dir` holds where the
user was.

```
myapp/
  ok.ok          # root tasks
  ok/admin.ok    # namespace "admin"
  ok/db.ok       # namespace "db"
  ok/_lib.ok     # helpers; never a namespace
```

Each `*.ok` file directly in `ok/` whose name does not start with `_` is a
namespace named after the file's stem. Files in subdirectories of `ok/` are
only reachable through `import`. A namespace name must be a valid task name
and must not be `trust`, `untrust` or `self`, or the name of a root task.

### 15.2 Project files are declarations only

The top level of a project file allows only `task`, `fn`, constant `let`,
`import`, and in the root file `requires` and `env file`. A **constant
expression** is built from literals, list and record literals, operators,
string interpolation, other constant `let` names, `path(...)`, `os.name`,
`os.arch` and `os.exe`. Anything else at the top level, such as `print(...)`,
`run`, or `let x = read(...)`, is a load-time error:

```
error[syntax]: project files cannot run code at the top level
  --> ok.ok:3:1
   |
 3 | print("loading")
   | ^^^^^^^^^^^^^^^^
   = help: move it into a task, or compute it inside the task that needs it
```

Loading a project, listing its tasks, printing help and completing never run
any code from it.

### 15.3 Tasks

```
# Build the app
task build(--release: bool) needs [gen] {
  run cargo build {...if release { ["--release"] } else { [] }}
}
```

- A task name is lowercase or uppercase ASCII letters, digits, `_` and inner
  `-`.
- In a project file, `task` is public (runnable from the command line) and
  `fn` is private (callable only from code). Tasks cannot be called as
  functions; share code through `fn`.
- The doc comment is the task's description; its first line is shown in the
  listing.
- Parameters use the same syntax and CLI parsing as `main` (section 14).
- The body's `script_dir` is the directory of the file that declares the
  task.

### 15.4 Running tasks

```
ok build --release
ok admin create-user bas --role admin
ok build --help
```

The first argument names a root task or a namespace; for a namespace, the
second names the task in it. A namespace given without a task prints that
namespace's listing. An unknown name exits with code 2 and suggests close
matches. A task's `--help` also lists the tasks it runs first (with their
arguments as written, `Runs first: gen, build(release: true)`) and its
confirmation question.

Relative `path` arguments of a task are made absolute against
`invocation_dir`, because the task runs in the project root.

### 15.5 Dependencies (`needs`)

```
task build(--release: bool, --target: string = "native") { ... }

task dist needs [gen, build(release: true), build(release: true, target: "wasm32")] {
  ...
}
```

**Entries.** Each entry of `needs [...]` names a task, optionally followed
directly (no space) by arguments in parentheses. An unqualified name is
looked up in the declaring file's namespace first, then in the root;
`ns.task` names a task in a namespace.

**Arguments** bind to the dependency's parameters by name, like named
arguments of a call (6.3), with one difference: positional parameters are
also given by name (`greet(name: "bas")`), and a positional parameter before
a given one that is not given takes its default. Unnamed arguments, a
parameter given twice, and the rest parameter are errors. Each value is a
constant expression (15.2), evaluated in the scope of the file that declares
the entry, so planning an invocation never runs project code. At load time,
an unknown task or parameter (with a suggestion), a non-constant value, a
missing required parameter (7.2; `needs [deploy]` when `deploy` has one) and
dependency cycles are errors with a span:

```
error[arg]: task `c` has no parameter `relase`
 --> ok.ok:2:17
  |
2 | task d needs [c(relase: true)] { }
  |                 ^^^^^^
  |
  = help: did you mean `release`?
```

Cycles are detected between tasks, whatever the arguments, and reported with
the cycle: `error: task dependency cycle: build -> gen -> build`. After
trust and confirmations and before any task runs, every argument of the
invocation is evaluated and checked against its parameter's type; a mismatch
is a `type` error pointing at the value.

**Instances.** A dependency is a task together with the arguments its entry
gives. Two entries anywhere in an invocation that name the same task with
equal arguments (same names, values equal by `==`, in any order) are the
same **instance**, which runs at most once. Different arguments make
different instances, which all run: `needs [build, build(release: true)]`
runs `build` twice. Defaults are not filled in for this comparison, so
`build` and `build(release: false)` are two instances even when `false` is
the default. The invoked task itself runs last, once, with its command-line
arguments.

**Order.** An instance starts only after every instance it needs has
finished, and a task's body runs after all of its dependencies have
finished. Instances that do not depend on each other, directly or
indirectly, may run at the same time, at most `--jobs N` at once (2.2). With
`--jobs 1`, and always with `--dry-run` (so the printed plan has a stable
order), instances run one at a time, depth first in listed order.

**Where instances run.** Whether an instance may overlap with another one is
decided from the dependency graph, not from timing. An instance that is
ordered against every other instance of the invocation (each step of a chain
`a -> b -> c`, or a single dependency) can never overlap, and runs in ok's
own process exactly like a task run one at a time. Every other instance runs
in a **worker**: a separate process of the same ok binary, started in the
directory ok was started in, which runs only that task. A worker:

- shows its output with a prefix: each line it writes to stdout or stderr
  appears on ok's stdout or stderr as `LABEL | line`, where `LABEL` is the
  task (`gen`, `db.migrate`, `build(release: true)`, with values shown as in
  lists) padded to the longest label among the workers of the invocation.
  Lines are never split or mixed; the order of lines from different workers
  is unspecified. A last line without a newline gets one. Commands in a
  worker write to a pipe, not a terminal, so some tools turn off colors.
- has no terminal for input: its stdin, which its commands inherit, is
  empty. `prompt()` returns its default or fails, and `confirm()` answers
  yes with `--yes` and otherwise fails (12.8); the error notes that the task
  runs in parallel and that `--jobs 1` runs dependencies one at a time. The
  `confirm "text"` modifiers of all tasks are still asked by ok before
  anything runs (15.7).
- gets ok's `--trace`, `--yes`, `--env` and `--color` settings and every
  environment change made so far (env files, earlier tasks).
- refuses to run when the project files no longer have the content that was
  trusted (16) for the invocation.
- ends with `exit(code)` like a separate run: `exit(0)` counts as success,
  any other code as a failure of that task.

Up-to-date checks (15.6) of workers' tasks are made by ok before starting
them; a skipped one prints `LABEL: up to date`. ok's own global option that
starts a worker is internal and not part of this interface.

**Failure.** When an instance fails, ok starts no further instance, waits
for the running workers to finish (it does not interrupt them, so their
commands are never cut off halfway), and then fails:

```
broken | error[fail]: it broke
broken |  --> ok.ok:2:3
...
slow   | slow finished
error: task `broken` failed with exit code 1
  = its output is above, each line starting with `broken |`
```

The failing task's own error is part of its prefixed output. Other workers
that failed in the meantime are listed as `also failed: ...`. ok exits with
the exit code of the first failed worker. An instance that fails in ok's own
process fails the invocation as before.

### 15.6 Up-to-date checks (`sources`, `outputs`)

```
task gen sources ["schema/*.json"] outputs ["src/gen.rs"] { run codegen }
```

`sources` and `outputs` are lists of constant glob patterns (12.1.1)
relative to the project root. After a task's dependencies have run, the task
is **skipped** when it declares `outputs`, every output pattern matches at
least one existing file, and the oldest output is newer than the newest
source. A skipped task prints `gen: up to date` to stderr. `--force` disables
skipping for every task in the invocation. Comparison is by modification
time; there is no content-hash mode in v1.

### 15.7 Modifiers and environment

- `confirm "text"` asks `text. Continue? [y/N]` before running. For an
  invocation, all confirmations of the task and its dependencies are asked
  before anything runs. `--yes` answers yes. Without a terminal and without
  `--yes`, ok fails with an error naming `--yes`.
- `hidden` keeps a task out of the listing and completions. It can still be
  run, and `ok self tasks --all` (17) lists it.
- Modifiers may appear in any order, each at most once.
- `env file ".env"` in the root file loads that file (format 13.7) into the
  environment before any task runs. A missing file is ignored. Variables
  already set in the process environment win. `--env NAME` additionally loads
  `<file>.NAME` for each declared env file (or `.env.NAME` at the root if none
  is declared); that file must exist, and its values override the base file.
- `requires "0.3"` declares the minimum ok version. An older ok refuses to
  load the project and says which version is needed.

### 15.8 Mounting a sub-project

`import "frontend/ok.ok" as fe` in the root `ok.ok` mounts the root tasks of
the sub-project as namespace `fe`: `ok fe build`. An import of a file named
`ok.ok` is always a mount; it requires `as` and is only allowed in the root
`ok.ok`. Mounted tasks run with the
sub-project's directory as working directory, and their `sources` and
`outputs` are relative to it. The sub-project's own
namespaces are not reachable through the mount in v1 (use `ok -C frontend db
migrate`). A mount name must not clash with a namespace file.

### 15.9 Listing

`ok` with no arguments prints the tasks of the project, root tasks first,
then each namespace in alphabetical order, tasks in declaration order:

```
myapp (/home/bas/myapp)

  build          Build the app
  gen            Generate code from the schemas
  reset-db       Wipe and reseed the local database

admin
  create-user    Create a user account

db
  migrate        Apply pending migrations
```

Outside a project, `ok` prints usage and exits with code 2.

## 16. Trust

A cloned repository's tasks are someone else's code. ok runs a project's
tasks only after the user approves the exact content of its files.

**Trust is not a sandbox.** A trusted task can do anything the user can.
Trust answers "did I agree to run this code?", not "is this code safe?".

### 16.1 What needs trust

Running a task (including with `--dry-run`) needs trust. Listing tasks,
`--help`, completions, `ok self check` and `ok self fmt` only parse files
and need no trust. Standalone scripts run by explicit path are not subject to
trust.

### 16.2 What is trusted

The **project files** are the root `ok.ok`, the files in `ok/`, and every
file they import, transitively. Their **fingerprint** is the SHA-256 of, for
each file in order of its `/`-separated path relative to the root: the path,
a NUL byte, the content length in decimal, a NUL byte, and the content.

A project is trusted when its current fingerprint has been trusted, or its
root directory has been trusted with `ok trust --dir`.

### 16.3 Prompt

Running a task in an untrusted project on a terminal shows:

```
This project is not trusted yet.

  root:   /home/bas/src/myapp
  files:  ok.ok, ok/admin.ok, ok/db.ok
  sha256: 9f2c4e...

Tasks are code that runs with your permissions. Trust is not a sandbox.
[v]iew files, [t]rust and run, [n]o:
```

`v` prints the files, then asks again. If an earlier version of this root was
trusted, the first line reads `The project files changed since you trusted
them.` and a `[d]iff` choice shows a unified diff against the trusted copy.
`t` records trust and runs; `n` exits with code 1.

### 16.4 Non-interactive use

`--trust` or `OK_TRUST=1` runs without prompting and without recording trust.
Without a terminal on stdin and without either, ok fails immediately (it never
waits for input):

```
error[trust]: this project is not trusted
  = help: review the files, then run `ok trust`; in CI set OK_TRUST=1
```

### 16.5 Commands and store

```
ok trust [DIR]            trust the current content of the project at DIR (default: discovered)
ok trust --dir [DIR]      trust the directory, whatever its content (for your own projects)
ok trust --list           list trusted directories and fingerprints
ok untrust [DIR]          remove trust for the project at DIR
```

The store lives in the platform configuration directory: `~/.config/ok/` on
Linux (or `$XDG_CONFIG_HOME/ok/`), `~/Library/Application Support/ok/` on
macOS, `%APPDATA%\ok\` on Windows. `OK_CONFIG_DIR` overrides the location
(tests use it to get an empty store). `ok untrust` removes every entry for
the project's root, content and directory trust alike. It holds `trust.json` (entries of root,
fingerprint or directory mode, and time) and `snapshots/<fingerprint>/`, a
copy of each trusted version used for diffs. Writes are atomic.

### 16.6 Import confinement

In project mode every project file and every import must resolve, after
following symlinks, inside the project root. An import or a namespace file
that escapes the root is a load-time error, whether by `..` or by a symlink.

## 17. Built-in commands (`ok self ...`)

The only reserved first arguments are `trust`, `untrust` and `self`.

| Command | Meaning |
|---|---|
| `ok self init [--checksums FILE]` | Write a starter `ok.ok` and the wrappers `./ok`, `ok.cmd`, `ok.ps1` (18) in the current directory. The checksums of this version's release are downloaded from the release (`SHA256SUMS`), or read from `FILE`. Refuses to overwrite. |
| `ok self completions SHELL` | Print a completion script for `zsh`, `bash`, `fish` or `powershell`. |
| `ok self complete -- WORDS...` | Used by completion scripts; prints candidates. |
| `ok self fmt [--check] [FILES...]` | Format ok files in the canonical layout (17.2). Default: the project files. `--check` lists files that would change and exits with 1. |
| `ok self check [--strict] [FILES...]` | Check files without running them (17.1). Default: the whole project. |
| `ok self tasks [--all]` | The task listing; `--all` includes hidden tasks. |
| `ok self version` | Print `ok X.Y.Z`. |

### 17.1 `ok self check`

`ok self check` parses and checks files without running any of their code.
Without files it checks the project around the current directory: first the
load-time rules of section 15 (namespaces, mounts, task references and the
arguments in `needs`, dependency cycles, import confinement), then every file
of the project: the root `ok.ok`, `ok/*.ok`, mounted sub-projects and the
files they import. Given files, it checks those; `ok.ok` and a file directly
in an `ok/` directory next to one are checked as project files.

For each file it reports:

1. syntax errors and the top-level rules of 4.1;
2. **unknown names**: a variable or function used where no declaration of
   that name is visible, an assignment to an undeclared name, a method call
   `x.f()` for which there is no function `f` (6.4), a name that an import
   namespace does not export (`lib.f`), and an `env` method that does not
   exist; the help line suggests a close name (10.3) or says that the name is
   only declared further down;
3. **wrong arguments** in a call of a known function: a `fn` of the file or
   of an import, a closure bound with `let`, or a builtin (12). Too many or
   too few positional arguments, a missing flag that has no default, a named
   argument that is not a flag of the function, and a positional parameter
   passed by name. The receiver of a method call and the input of a pipeline
   stage count as the first argument; with a spread `...xs` the number of
   arguments is unknown and only named arguments are checked;
4. an `import` whose file does not exist.

Names are resolved with the interpreter's rules (5.10, 7, 11): builtins and
the globals `os`, `env`, `args`, `cwd`, `home`, `script_dir` and
`invocation_dir`; a block's functions from the start of the block; imports
from the start of the file; `let` and `var` bindings, parameters, loop
variables, match bindings and `catch` names in their scopes. Code that runs
where it stands sees only the bindings made before it: `print(x)` followed by
`let x = 1` is an unknown name. Code that runs later (bodies of functions,
closures and tasks, parameter defaults, `sources` and `outputs`) sees every
name of its enclosing scopes, wherever in them it is declared.

A finding means the code fails whenever it runs. Where the outcome depends on
values, the check stays silent:

- calls of a function held in a variable, a parameter or a record field;
- a method call `x.f()` where `x` may be a record whose field `f` holds a
  function, which the call would use instead of the function `f` (6.4): that
  is the case when the file or one of its imports has a record literal with
  a key `f` whose value is not a plain literal, or assigns `r.f = ...`; a
  computed key (`r[k] = ...`) or `from_entries` makes every name possible.
  Method calls on literals (`"a,b".split(",")`) are always checked;
- the body of a `try` and the expression before `?`, where errors become
  values (6.2, 6.7);
- fields and properties of values (`r.name`, `env.HOME`) and the types of
  arguments;
- the rest of a file with an import that cannot be read or parsed, since it
  could define any name; only the import is reported.

The check does not follow calls: a function with an unknown name in its body
is reported even if nothing calls it.

**Lints** are warnings about code that runs but is almost certainly wrong.
There is one: **swapped `write` arguments** (12.1). `write(value, path)` is
reported when its first argument is a string or `path(...)` literal that
looks like a file name (no spaces or newlines, and the extension of a format
of 13.1, `.txt`, `.log`, `.md` or `.gz`) and its second does not, as in
`write("out.json", data)`; the method form and pipelines count their input
as the first argument.

Findings are rendered like runtime errors (10.1), warnings with
`warning[kind]:` instead of `error[kind]:`. All findings of all files are
shown, in file and source order, followed by a summary on stderr
(`ok: 3 files checked: 2 errors and 1 warning`); without findings the
summary goes to stdout (`ok: 3 files checked, no problems found`). The exit
code is 1 when there is an error, 0 when there are only warnings, or 1 with
`--strict`, and 2 for bad usage.

```
error[name]: unknown name `relase`
 --> deploy.ok:7:4
  |
7 | if relase { deploy("staging") }
  |    ^^^^^^ not defined here
  |
  = help: did you mean `release`?
error[arg]: `greet` takes 1 positional argument, got 2
 --> deploy.ok:9:1
  |
9 | greet("ok", "again")
  | ^^^^^^^^^^^^^^^^^^^^
  |
  = note: `greet` is declared at deploy.ok:2:4
warning[arg]: the arguments of `write` look swapped: "report.json" looks like a path
  --> deploy.ok:12:1
   |
12 | write("report.json", report)
   | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
   = note: write(value, path) writes the value to the file at path
   = help: the value comes first and the path second: write(report, "report.json")
ok: 1 file checked: 2 errors and 1 warning
```

### 17.2 Canonical layout (`ok self fmt`)

`ok self fmt` rewrites files in the canonical layout, the style of
`examples/`. It keeps the author's line breaks, except that it breaks lines
that are too long, and normalises everything else:

- **Indentation** is two spaces per level. A line inside brackets is one
  level deeper than the line that opened the innermost of them, however many
  brackets that line opened (`print(match x {` indents its arms once). A line
  that starts with a closing bracket is at the level of the line that opened
  it. A line that continues the previous statement (it starts with `|` or
  with `.name`, a method chain, or the previous line ends with a binary
  operator; 3.3) gets one more level.
- **Spacing**: one space around binary operators (`=`, `+=`, `=>`, `|`, `??`,
  `and`, `or`, `in`, comparisons and arithmetic), after `,` and `:`, and
  inside the braces of a block on one line (`{ print(x) }`). No space after
  `(`, `[`, `...` and a prefix `-`, before `)`, `]`, `,` and `:`, and inside
  record literals and record patterns (`{a: 1}`). Empty brackets are `()`,
  `[]` and `{}`. Where whitespace changes meaning (before `(` and `[`, which
  decides between a call and a juxtaposed argument, 4.4 and 8.2; around `.`,
  which decides between a field and an implicit closure; around a `-` that
  is not an operator, as in `--dry-run`, `reset-db` and `take -1`), a space
  is kept if there was one, and several become one.
- **Blank lines**: at most one in a row, none at the start of the file,
  after a line that ends with an opening bracket, or at the end. The file
  ends with one newline, and no line ends with blanks outside a string.
- **Comments** stay where they are. An own-line comment takes the
  indentation of the line after it (one level more if that line starts with
  a closing bracket). An end-of-line comment keeps its column, so aligned
  comments stay aligned; if the code now reaches that column, it follows two
  spaces after the code. A shebang line is kept as it is.
- **Verbatim**: string literals (also multi-line and triple-quoted ones)
  and commands in command mode (from `run` to the end of the command,
  including `\` continuations, redirects and `with {...}`) are copied byte
  for byte; only blanks after the end of a command are removed.
- **Long lines**: a line whose code, with its indentation and without an
  end-of-line comment, is longer than 100 characters is broken, unless it
  has an end-of-line comment or contains a string or command that spans
  lines:
  1. if a pipeline runs through the line outside any bracket opened on it,
     the line is broken before each of its `|`;
  2. otherwise the largest bracket group that opens and closes on the line
     is split: the arguments of a call, the parameters of a `fn` or task, a
     list literal, a record literal or `needs [...]` get one element per
     line and a trailing comma; a block on one line gets its statement on a
     line of its own. A group holding a single token (one long string) is
     not split.

  The new lines are checked again. A line that cannot be broken this way,
  such as a long string or command, stays as it is.

```
# before
let files = glob("**/*", root: root, hidden: true, exclude: ["**/.git", "**/node_modules", "**/target"])
let r = {name:x , size:stat(x).size}
if r.size>limit {print( "big" )}

# after
let files = glob(
  "**/*",
  root: root,
  hidden: true,
  exclude: ["**/.git", "**/node_modules", "**/target"],
)
let r = {name: x, size: stat(x).size}
if r.size > limit { print("big") }
```

Formatting never changes what a file means: `ok self fmt` parses its result
and compares the syntax tree (ignoring positions) and the comments with the
original, and if they differ it reports an internal error and leaves the
file unchanged. Formatting a formatted file changes nothing. A file that
does not parse is reported with its syntax error and left unchanged; the
exit code is then 1.

## 18. Bootstrap wrappers

`ok self init` writes `./ok` (POSIX sh) and `ok.cmd` plus `ok.ps1` (Windows).
Each wrapper pins one ok version and the SHA-256 of each release binary. On
first use it downloads the binary for the current OS and architecture into
the user cache (`${XDG_CACHE_HOME:-$HOME/.cache}/ok/<version>/` on Unix,
`%LOCALAPPDATA%\ok\<version>\` on Windows), verifies the checksum, refuses to
run on a mismatch, and then runs it with all arguments. Later runs use the
cached binary. `OK_DOWNLOAD_URL` overrides the base URL for mirrors. The
wrappers are the only shell code in the project and stay short.

## 19. Portability rules

- No behaviour depends on a Unix shell or coreutils being installed.
- Output is identical across OSes: paths print with `/`, `print` and `write`
  use `\n`, `glob` and `list_dir` sort the same way, `os.name` is the only
  intended difference.
- File names are compared case-sensitively by ok even on case-insensitive file
  systems.
- Where an OS lacks a feature (`chmod` on Windows), the function's documented
  behaviour on that OS is part of its specification.

## 20. Reserved for later versions

- `task name allow [net, env "KEY"] { }` is reserved for a permission system.
  ok v1 parses the `allow` modifier and rejects it: `error: task permissions
  (allow) are not implemented yet`.
- Not in v1: a REPL beyond `ok -e`, a package manager, user-defined types,
  async, sandboxing, content-hash up-to-date checks, nested
  namespaces. Ideas are collected in `IDEAS.md`.
