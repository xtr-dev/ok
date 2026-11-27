# Control flow and patterns

Choose with `if` and `match`, repeat with `for` and `while`, and take lists and records apart with patterns.

This page covers `if`, `match`, `for` and `while`. `if` and `match` are expressions, so they produce a value that you can bind with `let`. `for` and `while` are statements. A condition must be a `bool`, because ok has no truthiness (see [Values and variables](basics.md#booleans-and-the-absence-of-truthiness)). `try` and `catch` are covered in [Errors](errors.md). The exact rules are in [section 6](../SPEC.md#6-expressions) and [section 7.3](../SPEC.md#73-loops) of the spec.

## if and else

`if` runs a block when its condition is `true`. Add more conditions with `else if`, and add `else` for the case where no condition is `true`:

```ok file=if.ok
let free_gb = 3
if free_gb < 1 {
  print("disk almost full")
} else if free_gb < 5 {
  print("disk getting full")
} else {
  print("plenty of space")
}
```

```output
disk getting full
```

The braces are always required, even for one statement, and the condition needs no parentheses.

### if as an expression

The value of an `if` is the value of the last expression in the branch that ran, so you can bind the result with `let`:

```ok file=if-value.ok
let n = 7
let size = if n > 5 { "big" } else if n > 2 { "medium" } else { "small" }
print(size)
let note = if n > 10 { "huge" }
print(note)
```

```output
big
null
```

When no branch runs, as in the second `if`, the value is `null`. Add an `else` when you want a different value in that case.

### Records in conditions

Inside the condition of `if`, `while`, `for` and `match`, a `{` starts the block. To compare with a record literal there, wrap it in parentheses: `if cfg == ({debug: true}) { ... }`.

## for loops

`for` runs a block once for each element of a list:

```ok file=for.ok
for name in ["build", "test", "deploy"] {
  print("step: {name}")
}
```

```output
step: build
step: test
step: deploy
```

With two variables, the first is the index:

```ok file=for-index.ok
for i, name in ["build", "test"] {
  print(i + 1, name)
}
```

```output
1 build
2 test
```

Looping over a record gives its keys, or its keys and values with two variables. The order is the order the keys were written in:

```ok file=for-record.ok
let ports = {web: 8080, db: 5432}
for service in ports {
  print(service)
}
for service, port in ports {
  print("{service} listens on {port}")
}
```

```output
web
db
web listens on 8080
db listens on 5432
```

The loop variable can be a pattern, as on the left side of `let`. A record pattern takes the fields out of each record in a list:

```ok file=for-pattern.ok
let users = [{name: "ana", age: 34}, {name: "bo", age: 15}]
for {name, age} in users {
  print("{name} is {age}")
}
```

```output
ana is 34
bo is 15
```

`range(n)` counts from `0` to `n - 1`. `range(start, end, step)` counts from `start` up to, but not including, `end`. A range is lazy: it produces each number when the loop asks for it, so `range(1_000_000)` does not build a list of a million numbers.

```ok file=range.ok
for i in range(3) {
  print("try", i)
}
print(range(2, 10, 3), range(5, 0, -2))
```

```output
try 0
try 1
try 2
[2, 5, 8] [5, 3, 1]
```

A string is not a list, so looping over one is an error. Say whether you mean characters or lines:

```ok file=for-string.ok
let text = "abc"
for c in text {
  print(c)
}
```

```output exit=1
error[type]: cannot loop over a string directly
 --> for-string.ok:2:10
  |
2 | for c in text {
  |          ^^^^
  |
  = help: loop over s.chars() for characters or s.lines() for lines
```

A loop that only filters or transforms a list is shorter as a pipeline, such as `users | where .age >= 18 | map(.name)`. Use `for` when each iteration does something, such as printing, writing a file or running a command. See [Pipelines](pipelines.md).

## while loops

`while` repeats a block as long as its condition is `true`. Use it when you do not know the number of iterations in advance:

```ok file=while.ok
var size = 100
var rounds = 0
while size > 10 {
  size = size / 2
  rounds += 1
}
print("{rounds} rounds, size {size}")
```

```output
4 rounds, size 6.25
```

## break and continue

`break` leaves the innermost loop at once. `continue` skips the rest of the block and starts the next iteration. Both work in `for` and `while`.

```ok file=break.ok
for n in range(1, 20) {
  if n % 2 == 0 {
    continue
  }
  if n > 7 {
    break
  }
  print(n)
}
```

```output
1
3
5
7
```

To leave several loops at once, put them in a function and use `return` (see [Functions](functions.md#returning-a-value)). Outside a loop, `break` and `continue` are syntax errors.

## match

`match` compares a value with a list of patterns, from top to bottom, and evaluates the result of the first pattern that matches. Like `if`, it is an expression:

```ok file=match.ok
for os in ["windows", "linux", "macos", "plan9"] {
  let exe = match os {
    "windows" => "ok.exe"
    "linux" | "macos" => "ok"
    other => "unknown system {other}"
  }
  print(exe)
}
```

```output
ok.exe
ok
ok
unknown system plan9
```

Each `pattern => result` line is an arm. The result is an expression, or a block when it needs several statements. The value of a block is its last expression. Arms are separated by newlines, or by commas when you write a short `match` on one line.

```ok file=match-block.ok
let retries = 3
let message = match retries {
  0 => "no retries"
  1 => "one retry"
  n => {
    let total = n + 1
    "{n} retries, {total} attempts in all"
  }
}
print(message)
print(match retries { 0 => "none", _ => "some" })
```

```output
3 retries, 4 attempts in all
some
```

### Simple patterns

- A literal, such as `0`, `-1.5`, `"linux"`, `true` or `null`, matches a value equal to it with `==`. Ints and floats compare as numbers, so `1` matches `1.0`.
- `_` matches anything and binds nothing. Use it as the last arm for "everything else".
- A name matches anything and binds the value to that name, so the arm can use it. `other => ...` above is such an arm.
- Alternatives `a | b | c` match when any of them does.

### Guards

Add `if condition` after a pattern to accept the match only when the condition is `true`. The condition can use the names the pattern bound:

```ok file=guards.ok
fn grade(score) {
  match score {
    100 => "perfect"
    s if s >= 80 => "good"
    s if s >= 50 => "pass"
    _ => "fail"
  }
}
print(grade(100), grade(85), grade(60), grade(10))
```

```output
perfect good pass fail
```

A guard must be a `bool`, like every condition in ok. Any expression that gives a `bool` works, including a single name: `_ if verbose => "details"`.

### When nothing matches

ok does not check that the arms of a `match` cover every value. When no arm matches, ok raises an error that shows the value:

```ok file=no-match.ok
let code = 5
let name = match code {
  0 => "ok"
  1 => "failed"
}
```

```output exit=1
error[value]: no match arm matches 5
 --> no-match.ok:2:12
  |
2 | let name = match code {
  |            ^^^^^^^^^^^^
  |
  = help: add a `_ => ...` arm for all other values
```

When an unknown value is a mistake in the input, a final arm that calls `fail` gives a clearer message: `other => fail("unsupported platform: {other}")`. `fail` is described in [Errors](errors.md#raising-your-own-errors).

## List patterns

A list pattern looks like a list literal and matches lists by shape:

- `[]` matches only the empty list.
- `[a, b]` matches a list of exactly two elements and binds them.
- `[first, ...rest]` matches a list with at least one element, and `rest` gets the remaining elements as a list. `..._` ignores them.

Elements of a list pattern are patterns themselves, so they can be literals, nested lists or records:

```ok file=list-patterns.ok
fn describe(xs) {
  match xs {
    [] => "empty"
    [only] => "one: {only}"
    ["cmd", name, ..._] => "command {name}"
    [first, ...rest] => "starts with {first}, then {len(rest)} more"
    _ => "not a list: {xs}"
  }
}
print(describe([]))
print(describe([7]))
print(describe(["cmd", "build", "--release"]))
print(describe([1, 2, 3, 4]))
print(describe("text"))
```

```output
empty
one: 7
command build
starts with 1, then 3 more
not a list: text
```

A list pattern does not match a value that is not a list, and that is not an error: `match` goes on to the next arm. `let [a, b] = xs` is different. It raises an error when `xs` is not a list of two elements.

## Record patterns

A record pattern `{key: pattern}` matches a record that has every listed key with a value that matches. Other keys are allowed. `{key}` is short for `{key: key}`: the key must exist, and its value is bound to the same name. `{}` matches any record.

```ok file=record-patterns.ok
fn summary(resp) {
  match resp {
    {status: 200, body} => "ok: {body}"
    {status: 301 | 302, headers: {location}} => "moved to {location}"
    {status} if status >= 500 => "server error {status}"
    {status: _} => "other status"
    _ => "not a response"
  }
}
print(summary({status: 200, body: "hello", time: 12}))
print(summary({status: 302, headers: {location: "/new"}}))
print(summary({status: 301, headers: {}}))
print(summary({status: 503}))
print(summary({status: 404}))
print(summary([200]))
```

```output
ok: hello
moved to /new
other status
server error 503
other status
not a response
```

In the third call the status is 301, but the headers have no `location`. The second arm therefore does not match, and `match` goes on to the next arms. Keys that are not plain names are written as strings, as in a record literal: `{"content-type": ct}`.

A pattern can nest to any depth:

```ok file=nested-patterns.ok
let accounts = [
  {user: {name: "bas", roles: ["admin", "dev"]}},
  {user: {name: "kim", roles: []}},
]
for account in accounts {
  let line = match account {
    {user: {name, roles: ["admin", ...others]}} => "{name} is admin, also {others}"
    {user: {name}} => "{name} is not an admin"
  }
  print(line)
}
```

```output
bas is admin, also ["dev"]
kim is not an admin
```

## Rules for names in patterns

Names that a pattern binds exist only in that arm's guard and result. They shadow outer names without changing them:

```ok file=pattern-scope.ok
let first = "outer"
let got = match [9] {
  [first] => first
}
print(got, first)
```

```output
9 outer
```

A name may appear only once in a pattern, and every alternative of a `|` must bind the same names, so the result can rely on them. Both mistakes are syntax errors, which ok reports before the script runs:

```ok nocheck
let label = match pair {
  [x, 1] | [y, 2] => "x is {x}"
  _ => "other"
}
```

Here the second alternative binds `y` instead of `x`. When the alternatives need different names, write them as separate arms. The complete grammar of patterns is in [section 6.6 of the spec](../SPEC.md#66-match).
