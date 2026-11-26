# Pipelines

Chain list and text functions with `|`, write short predicates like `.age > 30`, and stream big files in bounded memory.

Most scripts take some data, filter it, reshape it and print or write the result. In ok you write that as a pipeline: a value on the left, then a chain of stages separated by `|`. The fine print is in [section 8 of the spec](../SPEC.md#8-pipelines).

## What `|` does

`a | f(x)` means `f(a, x)`: the value on the left becomes the first argument of the call on the right. That is all there is to it. Every list, text and file function in ok takes its main input first, so they all work as stages.

```ok file=pipe.ok
let words = ["pear", "apple", "fig"]
print(words | sort | join(", "))
print(join(sort(words), ", "))
```

```output
apple, fig, pear
apple, fig, pear
```

Both lines do the same thing. The pipeline reads left to right in the order things happen, while the nested calls read inside out.

Your own functions work as stages too, as long as they take the piped value as their first parameter:

```ok file=own.ok
fn add_tax(price, rate) { price * (1 + rate) }

print(100 | add_tax(0.21))
print([10, 20] | map(p => (p | add_tax(0.1))))
```

```output
121.0
[11.0, 22.0]
```

The parentheses around `p | add_tax(0.1)` matter. The body of an arrow closure stops before `|`, so without them `p => p | add_tax(0.1)` would pipe the closure itself into `add_tax`. Either parenthesise the body or call the function directly: `map(p => add_tax(p, 0.1))`.

## The shapes of a stage

The right side of `|` can take a few shapes:

- A call with parentheses: `| sort(.size, desc: true)`. The piped value is added in front of the arguments.
- A bare name: `| sort`, `| count`, `| print`. The function is called with the piped value only.
- A name and one argument, separated by a space: `| take 5`, `| join ", "`, `| sort .name`, `| where not .done`. This is the same as `| take(5)` and reads more like a sentence. It takes exactly one argument; for more, use parentheses.
- A `run` command: `| run sort -u`. The value becomes the command's standard input. See [Running commands](commands.md).
- Any other expression that gives a function, such as `| .items` or `| (x => x * 2)`.

```ok file=shapes.ok
let data = {items: [5, 3, 8, 1]}
print(data | .items | sort | take 2)
print(data.items | map(x => x * 10) | sum)
print(data.items | sort(desc: true) | join " > ")
```

```output
[1, 3]
170
8 > 5 > 3 > 1
```

A stage must be something to call. If you want to compare or calculate with the result of a pipeline, put the pipeline in parentheses:

```ok file=compare.ok
let xs = [1, 2, 3, 4]
if (xs | count) > 3 {
  print("more than three")
}
```

```output
more than three
```

Without the parentheses, `xs | count > 3` is an error that tells you to add them.

## Short predicates with `.field`

Most stages take a function: which items to keep, what to sort by, how to transform each item. You can always write a closure, `u => u.age > 30`, but usually you can leave out the parameter. A `.` followed by a name, where a value is expected, refers to a field (or property, or method) of the current item:

```ok file=users.ok
let users = [
  {name: "ana", age: 34, active: true},
  {name: "bo", age: 27, active: false},
  {name: "cy", age: 41, active: true},
]
print(users | where .active | map .name)
print(users | where .age > 30 and .active | count)
print(users | sort .age | map(.name.upper()))
print(users | map(.age * 2))
```

```output
["ana", "cy"]
2
["BO", "ANA", "CY"]
[68, 54, 82]
```

The whole argument becomes the function: `where .age > 30 and .active` means `where(u => u.age > 30 and u.active)`. You can also store one in a variable, since `let older = .age > 30` is a function.

Two things to know about this shorthand:

- It covers one argument, list element or record value. Inside a record literal, each value is its own function, so `map({id: .id})` builds a record holding a function, which `map` then rejects. Write the closure: `map(u => {id: u.id})`.
- A `.` alone is not a closure. For the item itself, write `x => x`. And `.name` cannot appear inside string interpolation; use `map(u => "{u.name}")`.

[Implicit closures](../SPEC.md#69-implicit-closures-field) in the spec has the exact rules, and [Functions](functions.md) covers closures in general.

## Filtering and transforming

`where` keeps the items for which the predicate is `true`. The predicate must return a bool: ok has no truthiness, so `where .name` on strings is an error rather than a silent guess. `map` replaces each item with the result of the function, and `each` calls a function for its effect, such as printing.

```ok file=orders.ok
let orders = [
  {id: 1, customer: "ana", total: 30},
  {id: 2, customer: "bo", total: 12},
  {id: 3, customer: "ana", total: 8},
]
orders
  | where .total >= 10
  | map(o => {...o, vip: o.total > 20})
  | each(o => print("#{o.id} {o.customer} vip={o.vip}"))
```

```output
#1 ana vip=true
#2 bo vip=false
```

Long pipelines go on several lines, each stage starting with `|`. The `{...o, vip: ...}` record copies every field of `o` and adds one; it is the usual way to add or change a field in `map`.

## Sorting, grouping and summarising

`sort` sorts by value, or by a key function, and is stable. `desc: true` reverses the order. `reverse`, `take` and `skip` do what their names say.

`group` puts the items in a record of lists keyed by the result of the key function, in order of first appearance. Combine it with `entries` to turn the groups back into a list you can map over.

```ok file=report.ok
let orders = [
  {id: 1, customer: "ana", total: 30},
  {id: 2, customer: "bo", total: 12},
  {id: 3, customer: "ana", total: 8},
  {id: 4, customer: "cy", total: 25},
]
print(orders | sort(.total, desc: true) | take 2 | map .id)
print(orders | group .customer | keys)

orders
  | group .customer
  | entries
  | map(e => {customer: e.key, spent: sum(e.value, .total)})
  | sort(.spent, desc: true)
  | each(r => print("{r.customer:<5} {r.spent:>4}"))
```

```output
[1, 4]
["ana", "bo", "cy"]
ana     38
cy      25
bo      12
```

The functions that reduce a list to one value:

| Function | Gives |
|---|---|
| `count`, `count(pred)` | how many items (that match) |
| `sum`, `sum(key)` | the total, `0` for an empty list |
| `min(key)`, `max(key)` | the smallest or largest item (not the key), `null` for an empty list |
| `first`, `first(pred)`, `last` | one item, or `null` if there is none |
| `any(pred)`, `all(pred)` | a bool |
| `join(sep)` | one string |

```ok file=summary.ok
let orders = [
  {id: 1, customer: "ana", total: 30},
  {id: 2, customer: "bo", total: 12},
  {id: 3, customer: "ana", total: 8},
]
print(orders | count .customer == "ana")
print(orders | sum .total)
print(orders | max .total | .customer)
print(orders | first(.total < 10) | .id)
print(orders | any .total > 25, orders | all .total > 25)
print(orders | uniq .customer | map .customer)
print([[1, 2], [3], []] | flat)
print(["a", "b"] | enumerate)
```

```output
2
50
ana
3
true false
["ana", "bo"]
[1, 2, 3]
[{index: 0, value: "a"}, {index: 1, value: "b"}]
```

`uniq` keeps the first item of each distinct value or key and does not need sorted input. The full list of list and record functions is in the [standard library reference](stdlib.md).

## Methods or pipes

Every function can also be called as a method, with the first argument before the dot. `xs.where(.ok).map(.id)` is the same as `xs | where .ok | map .id`. Use whichever reads better: methods suit short chains on one value, such as `name.trim().lower()`, and pipes suit longer data flows.

```ok file=methods.ok
let tags = [" Web", "ops ", "web"]
print(tags.map(.trim().lower()).uniq())
print(tags | map(.trim().lower()) | uniq)
```

```output
["web", "ops"]
["web", "ops"]
```

## Lazy data and big files

Some values are lazy: their data is produced only as a pipeline asks for it. These are:

- `read` of a line or row format: text, lines, CSV, TSV and JSON Lines (also when gzip-compressed). See [Data formats](data.md).
- `lines` applied to a lazy string, `range`, and `stdin()`.
- The output of a command in a pipeline.

The stages `where`, `map`, `take`, `skip`, `uniq`, `flat`, `enumerate` and `lines` pass laziness on: given a lazy list, they return a lazy list. The consumers `each`, `count`, `sum`, `min`, `max`, `first`, `last`, `any`, `all`, `join` and `write` pull one item at a time, so a pipeline made of these runs in bounded memory however big the file is. `first`, `any`, `all` and `take` stop as soon as they know the answer.

```ok file=big.ok
write(range(100_000) | map(n => "line {n}"), "big.log")
print(read("big.log") | lines | where .ends_with("99") | count)
print(read("big.log") | lines | first)
print(range(1_000_000_000_000) | where(n => n % 7 == 0) | skip 1 | take 3)
```

```output
1000
line 0
[7, 14, 21]
```

The script never holds the whole file in memory, not even while writing it, and the range of a trillion numbers produces only the 22 it needs.

Stopping early is visible when the data has a problem further on. In this file, line 4 has one field too many. Reading the first two rows works, because ok never gets to line 4; counting the rows has to read everything and fails:

```csv file=orders.csv
id,name
1,ana
2,bo
3,cy,extra
4,di
```

```ok file=early.ok
print(read("orders.csv") | take 2 | map .name)
print(read("orders.csv") | count)
```

```output exit=1
["ana", "bo"]
error[format]: line 4 of orders.csv has 3 fields, but the header has 2
 --> early.ok:2:7
  |
2 | print(read("orders.csv") | count)
  |       ^^^^^^^^^^^^^^^^^^^^^^^^^^
```

As this shows, an error in lazy data is raised where the data is consumed, and the message gives the line number in the file.

A `for` loop also pulls one item per iteration, so `break` stops the reading:

```ok file=loop.ok
write(["ok start", "ERROR disk full", "ok retry", "ERROR disk full again"], "app.log")
for i, line in read("app.log") | lines | where .starts_with("ERROR") {
  print(i, line)
  break
}
```

```output
0 ERROR disk full
```

## When lazy values materialise

A lazy value is read completely into an ordinary list or string (materialised) as soon as you:

- bind it to a name with `let` or `var`,
- put it in a list or record,
- pass it to a function that does not stream: any function you wrote, and the builtins not listed above, such as `sort`, `group`, `reverse` and `len`,
- index it, print it, interpolate it or compare it.

```ok
read("big.log") | lines | where .contains("ERROR") | count   # bounded memory
let all = read("big.log") | lines                             # the whole file in memory
```

Usually this is what you want: a few thousand rows fit in memory easily, and once a value is bound to a name you can use it several times. For files of gigabytes, keep the work in one pipeline that ends in a consumer such as `count`, `each` or `write`, and put the streaming stages (`where`, `take`) before the ones that materialise (`sort`, `group`).

[Section 13.2](../SPEC.md#132-lazy-values) of the spec has the details.
