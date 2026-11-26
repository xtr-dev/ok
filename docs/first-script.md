# Your first script

Build a small report script step by step: read a CSV file, filter and group it with a pipeline, print a report, write JSON and add a command line.

## What you will build

You have an export of orders from a web shop, and you want to know which products bring in the most money. By the end of this page you will have a script, `report.ok`, that you run like this:

```sh
ok report.ok orders.csv --top 3 --json summary.json
```

It prints revenue per product, writes the same numbers to a JSON file, has `--help`, and stops with a clear message when the input is wrong. Every step below is a complete script you can run, with the output it prints. Press Run on a step to try it in your browser, or copy the files into a folder and run them with the installed `ok`.

## Read and shape the data

This is the export. It is a normal CSV file with a header row:

```csv file=orders.csv
id,customer,product,quantity,price,status
1001,Anna,Notebook,3,4.50,paid
1002,Bram,Pen,10,1.20,paid
1003,Chloe,Backpack,1,39.95,refunded
1004,Anna,Pen,5,1.20,paid
1005,Dev,Notebook,2,4.50,pending
1006,Bram,Backpack,1,39.95,paid
1007,Chloe,Notebook,4,4.50,paid
```

### Step 1: read the file

`read` looks at the file extension and parses the file for you. A `.csv` file becomes a list of records, one per row, keyed by the header:

```ok file=report.ok
let orders = read("orders.csv")
print("{len(orders)} orders")
print(orders[0])
```

```output
7 orders
{id: "1001", customer: "Anna", product: "Notebook", quantity: "3", price: "4.50", status: "paid"}
```

`"{len(orders)} orders"` is string interpolation: any expression in `{...}` inside a double-quoted string is replaced by its value.

Look at the first record: every value is a string, including `"3"` and `"4.50"`. ok never guesses whether a CSV field is a number, a date or a boolean, because guessing goes wrong on values like ZIP codes and version numbers. You convert the fields you need yourself, which is the next step.

### Step 2: filter and compute

A pipeline passes a value through a series of functions with `|`. Each stage receives the result of the one before it:

```ok file=report.ok
let paid = read("orders.csv")
  | where .status == "paid"
  | map(o => {...o, total: int(o.quantity) * float(o.price)})

for o in paid {
  print(o.id, o.product, o.total)
}
```

```output
1001 Notebook 13.5
1002 Pen 12.0
1004 Pen 6.0
1006 Backpack 39.95
1007 Notebook 18.0
```

Read it from top to bottom:

- `where .status == "paid"` keeps the rows whose `status` is `"paid"`. `.status` is shorthand for "the `status` field of the current row", so you do not have to write a function `o => o.status == "paid"`.
- `map(o => {...})` turns each row into a new value. `o => ...` is a small function, and `{...o, total: ...}` is a record with all the fields of `o` plus a new `total` field.
- `int(...)` and `float(...)` convert the strings to numbers. If a field is not a number, they stop the script with an error; you will see that at the end of this page.

`print` with several arguments prints them separated by spaces.

### Step 3: group and sort

Now add up the revenue per product. `group` collects the rows into a record keyed by product, `entries` turns that record into a list of `{key, value}` pairs, and `sort` orders them:

```ok file=report.ok
let paid = read("orders.csv")
  | where .status == "paid"
  | map(o => {...o, total: int(o.quantity) * float(o.price)})

let by_product = paid
  | group .product
  | entries
  | map(e => {product: e.key, orders: len(e.value), revenue: e.value | sum .total})
  | sort(.revenue, desc: true)

for p in by_product {
  print(p)
}
```

```output
{product: "Backpack", orders: 1, revenue: 39.95}
{product: "Notebook", orders: 2, revenue: 31.5}
{product: "Pen", orders: 2, revenue: 18.0}
```

Two ways of writing a stage appear here. `group .product` has a space and exactly one argument, which reads well for short stages. `sort(.revenue, desc: true)` uses parentheses because it has two arguments; `desc: true` is a named argument. Both mean "call the function with the value from the left as its first argument". [Pipelines](pipelines.md) has the details.

### Step 4: print a report

Records are useful while you work, but people want a table. A format spec after a colon inside `{...}` controls width and alignment: `<10` pads to 10 characters on the left, `>8.2f` right-aligns a number in 8 characters with 2 decimals:

```ok file=report.ok
let paid = read("orders.csv")
  | where .status == "paid"
  | map(o => {...o, total: int(o.quantity) * float(o.price)})

let by_product = paid
  | group .product
  | entries
  | map(e => {product: e.key, orders: len(e.value), revenue: e.value | sum .total})
  | sort(.revenue, desc: true)

print("product    orders  revenue")
for p in by_product {
  print("{p.product:<10} {p.orders:>6} {p.revenue:>8.2f}")
}
print("{"total":<10} {len(paid):>6} {paid | sum .total:>8.2f}")
```

```output
product    orders  revenue
Backpack        1    39.95
Notebook        2    31.50
Pen             2    18.00
total           5    89.45
```

The last line shows that an interpolation can hold any expression, even a pipeline, and a string literal: `{"total":<10}`.

### Step 5: write JSON

Other programs want the numbers as data, not as a table. `write` picks the format from the extension, just like `read`, so writing JSON is one line:

```ok file=report.ok
let paid = read("orders.csv")
  | where .status == "paid"
  | map(o => {...o, total: int(o.quantity) * float(o.price)})

let by_product = paid
  | group .product
  | entries
  | map(e => {product: e.key, orders: len(e.value), revenue: e.value | sum .total})
  | sort(.revenue, desc: true)

write({orders: len(paid), products: by_product}, "summary.json")
print(read("summary.json"))
```

```output
{orders: 5, products: [{product: "Backpack", orders: 1, revenue: 39.95}, {product: "Notebook", orders: 2, revenue: 31.5}, {product: "Pen", orders: 2, revenue: 18.0}]}
```

The file itself is pretty-printed with two-space indentation and ends with a newline, and the keys keep the order you wrote them in, so the output is stable from one run to the next and diffs well in version control. `write` also creates missing parent directories. [Data formats](data.md) covers the other formats: TOML, YAML, JSON Lines, TSV, `.env` and compressed files.

## Make it a command-line tool

The script works, but the file name, the status and the number of products are fixed. In bash you would parse `$1` and `$2` by hand; in Python you would reach for `argparse`. In ok you declare a function `main`, and its parameters are the command line:

```csv file=orders.csv
id,customer,product,quantity,price,status
1001,Anna,Notebook,3,4.50,paid
1002,Bram,Pen,10,1.20,paid
1003,Chloe,Backpack,1,39.95,refunded
1004,Anna,Pen,5,1.20,paid
1005,Dev,Notebook,2,4.50,pending
1006,Bram,Backpack,1,39.95,paid
1007,Chloe,Notebook,4,4.50,paid
```

```ok file=report.ok
# Summarise an order export: revenue per product.
fn main(
  input: path = "orders.csv",                          # the CSV export to read
  --status: "paid" | "pending" | "refunded" = "paid",  # which orders to count
  --top: int = 10,                                     # how many products to list
  --json: path?,                                       # also write the summary as JSON here
) {
  let orders = read(input)
    | where .status == status
    | map(o => {...o, total: int(o.quantity) * float(o.price)})

  let by_product = orders
    | group .product
    | entries
    | map(e => {product: e.key, orders: len(e.value), revenue: e.value | sum .total})
    | sort(.revenue, desc: true)
    | take top

  print("{status} orders by product")
  for p in by_product {
    print("  {p.product:<10} {p.orders:>3} {p.revenue:>8.2f}")
  }
  if json != null {
    write({status, orders: len(orders), products: by_product}, json)
    print("wrote {json}")
  }
}
```

```output
paid orders by product
  Backpack     1    39.95
  Notebook     2    31.50
  Pen          2    18.00
```

The signature says everything about the command line:

- `input: path = "orders.csv"` is a positional argument with a default. As a `path`, a relative name is taken relative to the directory you run ok from.
- `--status: "paid" | "pending" | "refunded" = "paid"` is a flag that accepts only those three words.
- `--top: int = 10` is a flag whose value must be a whole number.
- `--json: path?` is an optional flag: the `?` means it is `null` when you leave it out.
- The comment above `main` describes the script, and the comment at the end of each parameter line describes that parameter.

The arguments are checked before any of your code runs. Run it with some flags:

```output run="ok report.ok --top 2 --json out/summary.json"
paid orders by product
  Backpack     1    39.95
  Notebook     2    31.50
wrote out/summary.json
```

```output run="ok report.ok --status refunded"
refunded orders by product
  Backpack     1    39.95
```

And ask it for help:

```output run="ok report.ok --help"
Summarise an order export: revenue per product.

Usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]

Arguments:
  [input]                           the CSV export to read [default: orders.csv]

Options:
  --status <paid|pending|refunded>  which orders to count [default: paid]
  --top <int>                       how many products to list [default: 10]
  --json <path>                     also write the summary as JSON here
  -h, --help                        print this help
```

Mistakes on the command line get one line of explanation, a suggestion when ok can guess what you meant, and the usage line. They exit with code 2, the usual code for wrong usage:

```output run="ok report.ok --tpo 2" exit=2
error: unknown flag --tpo
  = help: did you mean --top?
usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]
run with --help for details
```

```output run="ok report.ok --status payed" exit=2
error: --status expects one of "paid", "pending", "refunded", got "payed"
  = help: did you mean "paid"?
usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]
run with --help for details
```

```output run="ok report.ok --top two" exit=2
error: --top expects an integer, got "two"
usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]
run with --help for details
```

[Command-line scripts](script-cli.md) explains the rest: short flags such as `-v`, repeated flags, rest arguments and `--`.

## When the input is wrong

Real exports are messy. Suppose someone typed a word into the quantity column:

```csv file=orders.csv
id,customer,product,quantity,price,status
1001,Anna,Notebook,3,4.50,paid
1002,Bram,Pen,ten,1.20,paid
```

```ok file=report.ok
let orders = read("orders.csv")
  | map(o => {...o, total: int(o.quantity) * float(o.price)})
print(len(orders))
```

```output exit=1
error[value]: cannot convert "ten" to an int
 --> report.ok:2:28
  |
2 |   | map(o => {...o, total: int(o.quantity) * float(o.price)})
  |                            ^^^^^^^^^^^^^^^
```

ok does not carry on with a missing or zero value. It stops, says what went wrong, and points at the expression that failed. Every error looks like this: a kind (`value`), a message, the file, line and column, and the line of code with the problem underlined. The script exits with code 1.

The message names the bad value, but not the row it came from. When you know which mistakes your users make, you can do better. `int(o.quantity)?` gives `null` instead of an error, `??` supplies what to do with a `null`, and `fail` stops the script with your own message:

```ok file=report.ok
let orders = read("orders.csv")
  | map(o => {...o, total: quantity(o) * float(o.price)})
print(len(orders))

fn quantity(order) {
  int(order.quantity)? ?? fail("order {order.id}: quantity {order.quantity} is not a number")
}
```

```output exit=1
error[fail]: order 1002: quantity ten is not a number
 --> report.ok:6:27
  |
6 |   int(order.quantity)? ?? fail("order {order.id}: quantity {order.quantity} is not a number")
  |                           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
  = in `quantity`, called at report.ok:2:28
```

The last line is a short call trace: the error happened inside `quantity`, which was called on line 2. Functions can be declared after the code that uses them. A function returns the value of its last expression, so `quantity` needs no `return`.

Problems in the file itself are reported with the line of the data. A row with too few fields:

```csv file=orders.csv
id,customer,product,quantity,price,status
1001,Anna,Notebook,3,4.50,paid
1002,Bram,Pen
```

```ok file=report.ok
let orders = read("orders.csv")
print(len(orders))
```

```output exit=1
error[format]: line 3 of orders.csv has 3 fields, but the header has 6
 --> report.ok:1:14
  |
1 | let orders = read("orders.csv")
  |              ^^^^^^^^^^^^^^^^^^
```

A file that does not exist:

```ok file=report.ok
let orders = read("ordres.csv")
```

```output exit=1
error[io]: cannot read ordres.csv: no such file
 --> report.ok:1:14
  |
1 | let orders = read("ordres.csv")
  |              ^^^^^^^^^^^^^^^^^^
```

[Errors](errors.md) shows how to catch errors with `try` and `catch` when a script should recover instead of stopping.

## Where to go next

You have used most of what everyday scripts need: `read` and `write`, pipelines with `where`, `map`, `group` and `sort`, string interpolation with format specs, and `fn main` for the command line. From here:

- [Coming from bash and make](from-bash.md) if you want to replace existing shell scripts or Makefiles.
- [Running commands](commands.md) to start programs such as `git` or `cargo` from a script.
- [Tasks](tasks.md) to put a project's commands in `ok.ok` instead of a Makefile.
- The [log-summary](../examples/log-summary/) and [csv-to-json](../examples/csv-to-json/) examples, which do similar work and sit next to the bash and Python scripts they replace.
