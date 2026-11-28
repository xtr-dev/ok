# Values and variables

Bind names with `let` and `var`, and use the kinds of value ok has: numbers, strings, booleans, `null`, lists, records, paths and durations.

Every value in ok has a kind: `null`, `bool`, `int`, `float`, `string`, `bytes`, `list`, `record`, `path`, `duration`, `time` or `fn`. ok has no classes. Values behave as immutable values, so a function that receives a list cannot change your copy of it. This page covers the kinds that most scripts use and the operators that work on them. Functions have [their own page](functions.md). The exact rules for every kind are in [section 5 of the spec](../SPEC.md#5-values-and-types).

## Naming values with let and var

`let` binds a name to a value, and that binding cannot change. `var` binds a name that you can assign to again. Use `let` unless you need to assign again, because a `let` tells the reader that the name keeps its value.

```ok file=bindings.ok
let name = "ok"
var count = 0
count += 1
count += 1
print(name, count)
```

```output
ok 2
```

Assigning to a `let` is an error, and the message tells you what to change:

```ok file=reassign.ok
let attempts = 1
attempts = 2
```

```output exit=1
error[name]: cannot assign to `attempts`: it was declared with let
 --> reassign.ok:2:1
  |
2 | attempts = 2
  | ^^^^^^^^
  |
  = help: declare it with `var attempts = ...` to change it
```

A name can be declared only once in a scope. A block (the `{ ... }` of an `if`, a loop or a function) is a new scope. A name declared inside a block can reuse an outer name, and it hides (shadows) the outer name until the block ends:

```ok file=shadow.ok
let level = "outer"
if true {
  let level = "inner"
  print(level)
}
print(level)
```

```output
inner
outer
```

`let` and `var` can also take a list or a record apart. See [Lists](#lists) and [Records](#records).

## Numbers

ok has two kinds of number: `int`, a 64-bit signed integer, and `float`, a 64-bit floating-point number. Integer literals may use `_` to group digits, and the prefixes `0x`, `0o` and `0b` for hexadecimal, octal and binary. A float needs digits on both sides of the dot: `0.5`, not `.5`.

```ok file=numbers.ok
print(42, 1_000_000, 0x1F, 0o755, 0b1010)
print(1.5, 2.0, 1e3, 2.5e-3)
print(2 + 3 * 4, 2 * (3 + 4))
print(7 / 2, 6 / 3, 7 % 3, -7 % 3)
print(floor(7 / 2), ceil(7 / 2), round(2.5), int(3.9))
```

```output
42 1000000 31 493 10
1.5 2.0 1000.0 0.0025
14 14
3.5 2.0 1 -1
3 4 3 3
```

Two things differ from many languages:

- `/` always gives a float, also for two ints: `7 / 2` is `3.5`, not `3`. When you want a whole number, say how to round with `floor`, `ceil`, `round` or `int` (which truncates toward zero).
- Arithmetic never wraps and never produces infinity or NaN. An int that overflows, a division by zero and a float result that is not a finite number are errors, so the script stops at the calculation that went wrong.

```ok file=divide.ok
let total = 10
let parts = 0
print(total / parts)
```

```output exit=1
error[value]: division by zero
 --> divide.ok:3:7
  |
3 | print(total / parts)
  |       ^^^^^^^^^^^^^
```

Mixing an int and a float gives a float. Floats always print with a decimal point, so you can tell `2.0` from `2`.

## Strings

A double-quoted string can contain escapes (`\n`, `\t`, `\"`, `\\`, `\u{1F600}`) and interpolation: `{name}` inserts the value of `name` (see [Interpolation and format specs](#interpolation-and-format-specs)). A single-quoted string is raw. It has no escapes and no interpolation, so use it for regular expressions and Windows paths, which contain backslashes and braces.

```ok file=strings.ok
let name = "Ana"
print("Hello, {name}!\tWelcome.")
print('raw: {name} \d+ C:\Users')
print("braces: \{not interpolated\}")
```

```output
Hello, Ana!	Welcome.
raw: {name} \d+ C:\Users
braces: {not interpolated}
```

Strings are UTF-8. `len` counts characters (Unicode scalar values), not bytes, and indexing returns a one-character string. A negative index counts from the end.

```ok file=chars.ok
let word = "héllo"
print(len(word), word[0], word[1], word[-1])
print(word.upper(), word.contains("ll"))
```

```output
5 h é o
HÉLLO true
```

Text functions such as `upper`, `trim`, `split` and `replace` are listed in the [standard library](stdlib.md). They are usually written as methods, `word.upper()`, which is the same call as `upper(word)` (see [Functions](functions.md#method-calls)).

### Multi-line strings

A string in triple quotes can span several lines. Write `"""..."""`, or `'''...'''` for raw text. ok removes the indentation that all lines share, so you can indent the string along with your code:

```ok file=letter.ok
let name = "Bas"
let msg = """
  Hello {name},
    your build finished.
  Bye.
  """
print(msg)
```

```output
Hello Bas,
  your build finished.
Bye.
```

The newline right after the opening quotes is dropped, and because the closing quotes are on their own line, the string ends with a newline.

### Joining strings

`+` joins two strings. Adding a string and a value of another kind is an error:

```ok file=plus.ok
let count = 3
print("found " + count + " files")
```

```output exit=1
error[type]: cannot use `+` on a string and an int
 --> plus.ok:2:7
  |
2 | print("found " + count + " files")
  |       ^^^^^^^^^^^^^^^^
  |
  = help: build strings with interpolation: "{a}{b}"
```

Interpolation converts the value for you. Written as `"found {count} files"`, the same line works.

## Interpolation and format specs

Inside a double-quoted string, `{expr}` inserts the value of any expression, such as a name, a calculation, a field or a method call. A string is inserted as it is. A list or a record is inserted in its display form, in which strings are quoted.

```ok file=interp.ok
let files = ["a.txt", "b.txt"]
let user = {name: "ana", age: 34}
print("{len(files)} files: {files}")
print("{user.name.upper()} is {user.age + 1} next year")
```

```output
2 files: ["a.txt", "b.txt"]
ANA is 35 next year
```

To control width, alignment and number formatting, add `:spec` after the expression. A format spec is a subset of Python's: an optional fill and alignment (`<`, `>`, `^`), a width, `,` for thousands separators, `.N` for precision, and a type (`d`, `f`, `e`, `x`, `X`, `o`, `b`, `%`).

```ok file=specs.ok
let n = 1234567
print("{n:,}  {3.14159:.2f}  {0.256:.1%}  {255:x}  {5:03}")
print("[{"left":<8}] [{"right":>8}] [{"mid":^9}] [{"x":*^5}]")
for row in [{name: "build", secs: 12.5}, {name: "test", secs: 3.4}] {
  print("{row.name:<8}{row.secs:>6.1f}s")
}
```

```output
1,234,567  3.14  25.6%  ff  005
[left    ] [   right] [   mid   ] [**x**]
build     12.5s
test       3.4s
```

Strings align left by default and numbers align right, which suits tables. The `fmt(value, spec)` function applies the same format specs outside a string: `fmt(42, "05")` is `"00042"`. A format spec that does not fit the value is an error. For example, `{2.5:d}` fails because `d` needs an int. The full grammar is in [section 12.6 of the spec](../SPEC.md#126-format-specs).

## Booleans and the absence of truthiness

`true` and `false` are combined with `and`, `or` and `not`. `and` and `or` stop as soon as the answer is known.

ok has no truthiness. `if`, `while`, `and`, `or`, `not` and match guards all need a `bool` (see [Control flow and patterns](control-flow.md) for `if`, `while` and `match`). An empty list, `0` or `""` does not count as `false`, so a script cannot mistake `0` or an empty string for a missing value. A condition of another kind is an error:

```ok file=truthy.ok
let files = []
if files {
  print("nothing to do")
}
```

```output exit=1
error[type]: `if` needs a bool, got a list
 --> truthy.ok:2:4
  |
2 | if files {
  |    ^^^^^
  |
  = help: to test for an empty list use `if len(files) > 0` or `if files != []`
```

Write the test you mean: `if files == []` or `if len(files) == 0`. The symbols `!`, `&&` and `||` are not ok operators. The error for each of them names the ok operator to use: `not`, `and` or `or`.

## null, ?? and ?

`null` means "no value". The operator `??` replaces `null` with a default, and the operator `?` turns an error into `null`.

`a ?? b` gives `a`, or `b` when `a` is `null`. Only `null` is replaced: `0`, `false` and `""` are kept.

`expr?` is the catch operator. It turns an error into `null`, and it applies to the whole chain of fields, indexes and calls to its left. So `user.email?` is `null` when the record has no `email` field, and `int(text)?` is `null` when `text` is not a number. Together, `?` and `??` give a default for a value that may be missing or malformed:

```ok file=null.ok
let user = {name: "ana"}
let ports = ["8080", "not a port"]
print(user.email?, user.email? ?? "no email")
print(int(ports[0])? ?? 80, int(ports[1])? ?? 80)
print(0 ?? 5, false ?? true)
```

```output
null no email
8080 80
0 false
```

Without the `?`, a missing field is an error whose help line lists the fields that the record has. That error is useful while you write a script, so add `?` only where a missing value is expected.

ok has no optional chaining. `a?.b` is a syntax error whose help line suggests `a.b?` (catch the whole chain) or `(a?).b`. An expression that mixes `??` with `and` or `or` needs parentheses, because readers disagree about how it groups. To catch an error and look at it, use `try` and `catch`. See [Errors](errors.md).

## Lists

A list holds values of any kind, in order. The first element has index `0`, and a negative index counts from the end. Reading past the end is an error, and `xs[i]?` gives `null` instead. `+` joins two lists, `...xs` spreads a list into a list literal, and `in` tests whether a list contains a value.

```ok file=lists.ok
let xs = [10, 20, 30]
print(xs[0], xs[-1], len(xs))
print(xs + [40], [0, ...xs])
print(20 in xs, 99 not in xs)
print(xs[5]?)
```

```output
10 30 3
[10, 20, 30, 40] [0, 10, 20, 30]
true true
null
```

You can change a `var` list by index and extend it with `+=`. A list is a value, so a change to a copy leaves the original as it was:

```ok file=copy.ok
let original = [1, 2, 3]
var copy = original
copy[0] = 99
copy += [4]
print(original, copy)
```

```output
[1, 2, 3] [99, 2, 3, 4]
```

A list pattern on the left of `let` takes a list apart. Without `...rest`, the list must have exactly as many elements as the pattern:

```ok file=unpack.ok
let [major, minor, patch] = "1.4.2".split(".")
let [first, ...rest] = ["build", "--release", "--verbose"]
print(major, minor, patch)
print(first, rest)
```

```output
1 4 2
build ["--release", "--verbose"]
```

Most work on lists is done in pipelines, with functions such as `where`, `map` and `sort`. See [Pipelines](pipelines.md).

## Records

A record maps string keys to values and keeps the keys in the order they were added. Read a field with `r.key`, or with `r["key"]` when the key is not a plain name or is in a variable. In a record literal, write a key that is not a plain name as a string. `in` tests for a key, and `len` counts the fields.

```ok file=records.ok
let user = {name: "ana", age: 34, "e-mail": "ana@example.com"}
let field = "age"
print(user.name, user[field], user["e-mail"])
print("age" in user, len(user))
```

```output
ana 34 ana@example.com
true 3
```

A record literal has two shorthands. `{name}` means `{name: name}`, and `...r` copies the fields of the record `r`. When a key appears twice, the later value wins, so `{...r, key: value}` builds a copy of `r` with one field changed:

```ok file=update.ok
let name = "bo"
let defaults = {port: 80, debug: false}
let cfg = {...defaults, debug: true}
print({name}, cfg)
```

```output
{name: "bo"} {port: 80, debug: true}
```

A `var` record can be changed field by field, at any depth. As with lists, the change never reaches other copies:

```ok file=nested.ok
var cfg = {server: {port: 80}, tags: []}
let before = cfg
cfg.server.port = 8080
cfg["log-level"] = "debug"
cfg.tags += ["web"]
print(cfg)
print(before)
```

```output
{server: {port: 8080}, tags: ["web"], "log-level": "debug"}
{server: {port: 80}, tags: []}
```

Reading a field that does not exist is an error. Its help line lists the record's fields or suggests a close match. Add `?` to get `null` instead (see [null, ?? and ?](#null--and-)).

A record pattern on the left of `let` takes the fields you name out of a record. Other fields are ignored, and a pattern can contain another pattern:

```ok file=destructure.ok
let repo = {name: "ok", stars: 12, owner: {login: "xtr-dev", id: 1}}
let {name, owner: {login}} = repo
print(name, login)
```

```output
ok xtr-dev
```

## Paths

A `path` is a file-system path. Make one with `path("...")` and join parts with `/`. ok normalises paths without reading the file system: repeated separators collapse, `.` disappears and `a/..` cancels out. Paths always print with `/`, so a script prints the same on Windows as on Linux. ok converts a path to the native form when it reaches a command or the file system.

```ok file=paths.ok
let src = path("src")
let main = src / "app" / "main.rs"
print(main, main.name, main.stem, main.ext, main.parent)
print(path("a//b/./c/../d/"))
print(main.with_ext("o"), type(main))
```

```output
src/app/main.rs main.rs main rs src/app
a/b/d
src/app/main.o path
```

Joining two strings with `/` is an error, so a path always starts from `path(...)` (or from a value that is already a path, such as `cwd` or the result of `glob`):

```ok file=join.ok
let out = "build" / "out"
```

```output exit=1
error[type]: cannot use `/` on a string and a string
 --> join.ok:1:11
  |
1 | let out = "build" / "out"
  |           ^^^^^^^^^^^^^^^
  |
  = help: to join paths, start with a path: path("build") / "out"
```

A path never equals a string, even with the same text: `path("a") == "a"` is `false`. Compare `p.name` or `str(p)` when you mean text. Functions that take a path accept a string too. Working with files is covered in [Files and paths](files.md).

## Durations

A duration literal is a number followed by a unit: `ms`, `s`, `m`, `h` or `d` (24 hours). Units can be combined from largest to smallest, as in `1h30m`. `sleep`, timeouts and retry delays take a duration, so the unit is always written in the script.

```ok file=durations.ok
print(250ms, 1.5s, 90s, 1h30m, 36h, -2m)
print(1h + 30m, 2m - 30s, 3 * 20s, 1h / 4)
print(1h / 30m, (90s).seconds, (1500ms).millis)
print(5s > 1s)
```

```output
250ms 1.5s 1m30s 1h30m 1d12h -2m
1h30m 1m30s 1m 15m
2.0 90.0 1500
true
```

You can add and subtract durations, and multiply or divide a duration by a number. Dividing one duration by another gives a float. The properties `.seconds` and `.millis` give a duration as a number. `duration("1h30m")` parses the same syntax from a string. Adding a plain number to a duration is an error, because ok cannot know the unit you meant. A `time` (from `now()`, `time("2026-11-24")` or a file's modification time) minus another `time` is a duration. See [section 5.9 of the spec](../SPEC.md#59-durations-and-times).

## Operators

The table lists the operators from loosest to tightest binding:

| Operators | Meaning |
|---|---|
| `\|` | pipeline ([Pipelines](pipelines.md)) |
| `??` | null fallback |
| `or`, `and`, `not` | boolean logic |
| `==` `!=` `<` `<=` `>` `>=` `in` `not in` | comparison and membership |
| `+` `-` | add and subtract numbers and durations; `+` also joins strings and lists |
| `*` `/` `%` | multiply, divide, remainder; `/` also joins paths |
| `-` (prefix) | negate |
| `.field` `[index]` `(call)` `?` | postfix |

So `not a == b` means `not (a == b)`, and `a + b * c` means `a + (b * c)`. When in doubt, add parentheses. `ok self fmt` keeps them.

`in` tests for an element of a list, a key of a record or a substring of a string:

```ok file=in.ok
print(2 in [1, 2, 3], "port" in {port: 80}, "ell" in "hello", 4 not in [1, 2])
```

```output
true true true true
```

## Equality and comparison

`==` and `!=` compare values by structure. Two lists are equal when their elements are equal in order, and two records are equal when they have the same keys and values, in any order. Values of different kinds are never equal, except that an int and a float compare as numbers.

```ok file=equal.ok
print([1, "a"] == [1, "a"], {a: 1, b: 2} == {b: 2, a: 1})
print(1 == 1.0, 1 == "1", 0 == false, null == null)
```

```output
true true
true false false true
```

`<`, `<=`, `>` and `>=` work between two numbers, two strings (by Unicode code point, so `"B" < "a"`), two durations, two times, two paths, two bools (`false < true`) and two lists (element by element). Any other pair, such as a number and a string, is an error.

Comparisons do not chain. `1 < x < 10` is a syntax error that tells you to write `1 < x and x < 10`.

## Types and conversions

`type(x)` returns the name of a value's kind as a string. The functions `int`, `float` and `str` convert a value to that kind. `int` and `float` fail on text that is not a number, so add `?` and `??` when the text may be malformed.

```ok file=types.ok
print(type(1), type(1.0), type("s"), type([]), type({}), type(null))
print(type(path(".")), type(5s), type(true), type(print))
print(int("42") + 1, float("2.5"), int(-3.9), str(12) + "!")
print(int("forty-two")? ?? 0)
```

```output
int float string list record null
path duration bool fn
43 2.5 -3 12!
0
```

ok has no type annotations on variables. Function parameters can declare types, and ok checks them at every call. See [Functions](functions.md#parameter-types).
