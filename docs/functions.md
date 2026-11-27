# Functions

Declare functions with typed parameters, defaults and flags, write closures and implicit closures, and call functions in pipelines.

Functions in ok are values. You can store a function in a variable, put it in a record and pass it to another function. Most often you pass a function to a list function such as `where`, `map` or `sort`, and ok has two short forms for that: arrow closures (`x => x * 2`) and implicit closures (`.name`). The exact rules are in [section 5.10](../SPEC.md#510-functions), [section 6.8](../SPEC.md#68-closures), [section 6.9](../SPEC.md#69-implicit-closures-field) and [section 7.2](../SPEC.md#72-function-parameters) of the spec.

## Declaring a function

`fn name(parameters) { body }` declares a function. It returns the value of its last expression, so short functions need no `return`:

```ok file=declare.ok
# Format a size in bytes for people to read.
fn human_size(bytes) {
  if bytes < 1024 { "{bytes} B" } else { "{bytes / 1024:.1f} KiB" }
}

print(human_size(512))
print(human_size(1536))
```

```output
512 B
1.5 KiB
```

The comment lines directly above a `fn` are its doc comment. A doc comment describes the function to readers. For `main` and for tasks it also becomes the `--help` text (see [Command-line scripts](script-cli.md)).

You can call a function before the line that declares it. A function declared at the top level is visible in the whole file, and a function declared inside a block is visible in the whole block. A script can therefore start with its main steps and declare the helpers below them.

```ok file=hoisting.ok
print(greet("Ana"))

fn greet(name) {
  "Hello, {name}"
}
```

```output
Hello, Ana
```

## Parameters and arguments

A plain parameter is positional: callers pass arguments in order. Passing too many or too few is an error at the call:

```ok file=arity.ok
fn area(width, height) {
  width * height
}
print(area(3, 4))
print(area(3))
```

```output exit=1
12
error[arg]: missing argument `height` for `area`
 --> arity.ok:5:7
  |
5 | print(area(3))
  |       ^^^^^^^
```

### Defaults

`name = value` gives a parameter a default, which makes it optional. The default is evaluated at each call, and it may use earlier parameters:

```ok file=defaults.ok
fn banner(title, width = len(title) + 4) {
  let line = "-".repeat(width)
  let middle = title.pad(width, align: "center")
  "{line}\n{middle}\n{line}"
}
print(banner("ok"))
print(banner("ok", 10))
```

```output
------
  ok
------
----------
    ok
----------
```

### Named flags

A parameter written with `--` is a flag. Callers must pass a flag by name, as `name: value`, after the positional arguments. Use flags for options: the call `glob("*", hidden: true)` says what `true` means, and `glob("*", true)` would not.

```ok file=flags.ok
fn greet(name: string, --greeting: string = "Hello", --loud: bool) {
  let text = "{greeting}, {name}"
  if loud { "{text}!" } else { text }
}
print(greet("Ana"))
print(greet("Bo", greeting: "Hi", loud: true))
```

```output
Hello, Ana
Hi, Bo!
```

The `: string` and `: bool` after the names are types, described in [Parameter types](#parameter-types). A `bool` flag without a default is `false`, a `T?` flag is `null` and a `list[T]` flag is `[]`, so these flags are optional without a written default. A flag with dashes, `--dry-run`, is the name `dry_run` inside the body and at the call: `deploy("prod", dry_run: true)`.

Positional parameters cannot be passed by name, and a misspelt name is caught with a suggestion:

```ok file=named.ok
fn greet(name, --loud: bool) {
  name
}
greet("Ana", lound: true)
```

```output exit=1
error[arg]: `greet` has no parameter `lound`
 --> named.ok:4:14
  |
4 | greet("Ana", lound: true)
  |              ^^^^^
  |
  = help: did you mean `loud`?
```

For `main` and for tasks, each flag becomes a command-line option `--name value`. See [Command-line scripts](script-cli.md) and [Tasks](tasks.md).

### Rest parameters and spreading

`...name` as the last positional parameter collects any remaining positional arguments into a list. At the call, `...xs` does the opposite and spreads a list into separate arguments:

```ok file=rest.ok
fn total(first: int, ...rest: int) {
  var sum = first
  for x in rest {
    sum += x
  }
  sum
}
let more = [4, 5, 6]
print(total(1), total(1, 2, 3), total(...more))
```

```output
1 6 15
```

## Parameter types

A parameter can declare a type after a colon. ok checks the type at every call. An argument of the wrong type is an error that points at the call and names the parameter:

```ok file=types.ok
fn backup(file: path, --keep: int = 3) {
  "keep {keep} copies of {file.name}"
}
print(backup("data/app.db"))
print(backup("data/app.db", keep: "all"))
```

```output exit=1
keep 3 copies of app.db
error[type]: argument `keep` of `backup` must be an int, got a string
 --> types.ok:5:7
  |
5 | print(backup("data/app.db", keep: "all"))
  |       ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = note: `backup` is declared at types.ok:1:4
```

The first call also shows a conversion: a string passed to a `path` parameter becomes a path, so `file.name` works. Likewise an `int` passed to a `float` parameter becomes a float.

The types are `any`, `null`, `bool`, `int`, `float`, `string`, `bytes`, `path`, `duration`, `time`, `record`, `fn`, `list` and `list[T]`. Add `?` to also accept `null`: `int?`. A choice type lists the strings a parameter accepts:

```ok file=choice.ok
fn deploy(env: "staging" | "production") {
  "deploying to {env}"
}
print(deploy("staging"))
print(deploy("prod"))
```

```output exit=1
deploying to staging
error[type]: argument `env` of `deploy` must be one of "staging", "production", got "prod"
 --> choice.ok:5:7
  |
5 | print(deploy("prod"))
  |       ^^^^^^^^^^^^^^
  |
  = note: `deploy` is declared at choice.ok:1:4
```

Types are optional. A parameter without one accepts anything, like `any`. There are no type annotations on `let` or on return values. The full table of conversions is in [section 5.11 of the spec](../SPEC.md#511-type-annotations).

## Returning a value

A function's value is the last expression of its body. `return value` leaves the function early, from anywhere inside it, including from inside loops. A function that ends without a value, or with a bare `return`, gives `null`.

```ok file=return.ok
fn index_of(xs, target) {
  for i, x in xs {
    if x == target {
      return i
    }
  }
  null
}
print(index_of(["a", "b", "c"], "b"), index_of(["a"], "z"))
```

```output
1 null
```

`return` at the top level of a script is an error. To end a script early, use `exit(code)`.

## Recursion

A function may call itself:

```ok file=recursion.ok
fn fib(n) {
  if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}
print(fib(15))
```

```output
610
```

Calls nest at most 2000 deep. Deeper recursion is an error that points at the repeating call.

To walk a deep structure such as a directory tree, use a function that already does it, such as `glob("**/*.txt")`.

## Closures

A closure is a function written as an expression, without a name. `fn(parameters) { body }` is a closure with a block as its body. The arrow form is shorter and has a single expression as its body: `x => x * 2`, `(a, b) => a + b` for several parameters, or `() => ...` for none.

```ok file=closures.ok
let add = (a, b) => a + b
let double = x => x * 2
let shout = fn(s) {
  let up = s.upper()
  "{up}!"
}
print(add(2, 3), double(21), shout("hey"))
print([3, 1, 2] | map(x => x * 10))
```

```output
5 42 HEY!
[30, 10, 20]
```

A closure can use the variables around it. It captures them by reference, so it sees later changes to a `var`, and it can change that `var` itself:

```ok file=counter.ok
fn counter() {
  var count = 0
  fn next() {
    count += 1
    count
  }
  next
}
let next = counter()
next()
next()
print(next())
```

```output
3
```

Here `counter` returns its inner function `next`, which keeps `count` alive between calls.

An arrow's body extends over operators but stops at `|`. So in `xs | map x => x * 2 | sum`, the arrow is `x => x * 2` and `sum` is the next stage. To use a pipeline inside an arrow, put the body in parentheses.

## Implicit closures with .field

An implicit closure is a closure without a written parameter: `.name` means `u => u.name`. A `.` followed by a name, where a value is expected, refers to a field, property or method of an implicit argument, and the surrounding expression becomes a function of one parameter. Most closures in scripts look up a field or call a method on their argument, as in `u => u.name` and `line => line.contains("ERROR")`, so the implicit form fits most of them:

```ok file=implicit.ok
let users = [
  {name: "ana", age: 34, active: true},
  {name: "bo", age: 15, active: true},
  {name: "cy", age: 52, active: false},
]
print(users | where .active | map(.name))
print(users | where .age >= 18 and .active | map(.name))
print(users | sort .age | map(.name.upper()))
let by_name = .name
print(by_name(users[0]))
```

```output
["ana", "bo"]
["ana"]
["BO", "ANA", "CY"]
ana
```

The spec calls the implicit argument `it`, so `.age >= 18 and .active` means `it => it.age >= 18 and it.active`. `it` only explains the rule. It is not a name you can write in a script. When you need the argument itself and not one of its fields, write an arrow such as `x => x > 2`.

An implicit closure covers the whole expression up to the nearest argument boundary: a call argument, a pipeline-stage argument, a list element, a record value, the right side of `let`, `var` or an assignment, or an arrow or `fn` body. That is why `.age >= 18 and .active` is one closure and not two. Inside the arguments of a call within an implicit closure, a new `.x` starts a new closure:

```ok file=nested-implicit.ok
let teams = [
  {name: "web", members: [{name: "ana", active: true}, {name: "bo", active: false}]},
  {name: "db", members: [{name: "cy", active: true}]},
]
print(teams | map(.members | where .active | map(.name)))
```

```output
[["ana"], ["cy"]]
```

The outer `.members` belongs to the closure passed to the first `map`. `.active` and `.name` start their own closures for `where` and for the inner `map`.

## Method calls

`x.name(args)` is another way to write `name(x, args)`. It works for built-in functions and for your own:

```ok file=methods.ok
fn shout(s) {
  "{s.upper()}!"
}
print("hi".shout(), shout("hi"))
print("a,b,c".split(",").reverse())
```

```output
HI! HI!
["c", "b", "a"]
```

If `x` is a record with a field `name` that holds a function, that function is called instead. This is how you call a function that is stored in a record or imported under a namespace (see [Modules](modules.md#importing-under-a-name)). `x.name` without parentheses is always a field or property, never a call.

## Calls in pipelines

A pipeline passes the value on its left as the first argument of the call on its right: `a | f(x)` means `f(a, x)`, and `a | f` means `f(a)`. Every list function takes the list first. Your own functions work in pipelines too when they take their main input as the first parameter:

```ok file=pipeline-calls.ok
fn scale(xs, factor) {
  xs | map(x => x * factor)
}
print([1, 2, 3] | scale(10) | sum)
print([1, 2, 3] | scale 10)
```

```output
60
[10, 20, 30]
```

The second line uses a juxtaposed call: inside a pipeline, a name followed by one argument, `scale 10`, means `scale(10)`. Laziness and the other forms of a stage are covered in [Pipelines](pipelines.md).
