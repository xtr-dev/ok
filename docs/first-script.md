# Your first script

In this tutorial you build a script, `report.ok`, that reads a web shop's order export and reports the revenue per product. At the end you run the script like this:

```sh
ok report.ok orders.csv --top 3 --json summary.json
```

The script prints a table of revenue per product, writes the same numbers to a JSON file, has `--help`, and stops with an error when the input is wrong.

Each step is a complete script, followed by the output that it prints. To run a step, press Run on it in your browser. If you [installed ok](index.md#install-ok), you can instead save the files in an empty folder and run `ok report.ok` there.

## Build the report

The order export is a CSV file with a header row. Save it as `orders.csv`:

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

`read` chooses a parser from the file extension. It turns a `.csv` file into a list of records, one per row, keyed by the header. Save this script as `report.ok`:

```ok file=report.ok
let orders = read("orders.csv")
print("{len(orders)} orders")
print(orders[0])
```

```output
7 orders
{id: "1001", customer: "Anna", product: "Notebook", quantity: "3", price: "4.50", status: "paid"}
```

`"{len(orders)} orders"` is string interpolation. Inside a double-quoted string, ok replaces any expression in `{...}` by its value.

Every value in the first record is a string, including `"3"` and `"4.50"`. ok never guesses whether a CSV field is a number, a date or a boolean, because a guess goes wrong on values such as ZIP codes and version numbers. In step 2 you convert the fields that you need.

### Step 2: filter and compute

A pipeline passes a value through a series of functions with `|`. Each stage receives the result of the stage before it. Replace the script with a pipeline that keeps the paid orders and computes a total for each:

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

The pipeline reads from top to bottom:

- `where .status == "paid"` keeps the rows whose `status` is `"paid"`. `.status` is short for "the `status` field of the current row", so you do not have to write the function `o => o.status == "paid"`.
- `map(o => {...})` turns each row into a new value. `o => ...` is a function, and `{...o, total: ...}` is a record with all the fields of `o` and a new field, `total`.
- `int(...)` and `float(...)` convert the strings to numbers. If a field is not a number, they stop the script with an error, which [When the input is wrong](#when-the-input-is-wrong) shows.

`print` with several arguments prints them separated by spaces.

### Step 3: group and sort

Add a second pipeline that adds up the revenue per product. `group` collects the rows into a record keyed by product, `entries` turns that record into a list of `{key, value}` pairs, and `sort` orders the list:

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

This pipeline writes its stages in two ways. `group .product` has a space and exactly one argument. `sort(.revenue, desc: true)` uses parentheses because it has two arguments, and `desc: true` is a named argument. Both forms call the function with the value from the left as its first argument. [Pipelines](pipelines.md) has the details.

### Step 4: print a report

Replace the loop that prints records with one that prints a table. A format spec after a colon inside `{...}` sets the width and the alignment. `<10` left-aligns a value in 10 characters, and `>8.2f` right-aligns a number in 8 characters with 2 decimals:

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

An interpolation can hold any expression. The last line of the script has a string literal in one, `{"total":<10}`, and a pipeline in another.

### Step 5: write JSON

Another program can use the numbers if they are in a JSON file. `write` chooses the format from the file extension, as `read` does. This version of the script writes `summary.json` and reads it back to show what the file holds:

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

The file `summary.json` is indented with two spaces and ends with a newline, and its keys keep the order in which you wrote them. Two runs on the same input therefore write the same bytes, which keeps diffs in version control small. `write` also creates missing parent directories. [Data formats](data.md) covers the other formats, which are TOML, YAML, JSON Lines, TSV, `.env` and compressed files.

### Step 6: add a command line

So far the file name, the status and the number of products are fixed in the script. To make them arguments, put the code in a function `main`. The parameters of `main` are the script's command line. This version combines the table of step 4 and the JSON file of step 5:

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

Without arguments, the script uses the defaults. Each parameter of `main` is one part of the command line:

- `input: path = "orders.csv"` is a positional argument with a default. Because it is a `path`, a relative name is relative to the directory that you run ok from.
- `--status: "paid" | "pending" | "refunded" = "paid"` is a flag that accepts only those three words.
- `--top: int = 10` is a flag whose value must be a whole number.
- `--json: path?` is an optional flag. The `?` means that it is `null` when you leave it out.
- The comment above `main` describes the script, and the comment at the end of each parameter line describes that parameter.

### Step 7: run the script with arguments

Run the script with `--top` and `--json`. It lists two products and writes the JSON file:

```output run="ok report.ok --top 2 --json out/summary.json"
paid orders by product
  Backpack     1    39.95
  Notebook     2    31.50
wrote out/summary.json
```

With `--status`, it counts the refunded orders:

```output run="ok report.ok --status refunded"
refunded orders by product
  Backpack     1    39.95
```

With `--help`, it prints the comments from the signature:

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

ok checks the arguments before any of your code runs. For a wrong argument it prints one line that explains the mistake, a suggestion when a known flag or value is close to what you typed, and the usage line. The script exits with code 2, the usual code for wrong usage. A misspelled flag:

```output run="ok report.ok --tpo 2" exit=2
error: unknown flag --tpo
  = help: did you mean --top?
usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]
run with --help for details
```

A value that is not one of the choices:

```output run="ok report.ok --status payed" exit=2
error: --status expects one of "paid", "pending", "refunded", got "payed"
  = help: did you mean "paid"?
usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]
run with --help for details
```

A value of the wrong type:

```output run="ok report.ok --top two" exit=2
error: --top expects an integer, got "two"
usage: ok report.ok [input] [--status <paid|pending|refunded>] [--top <int>] [--json <path>]
run with --help for details
```

[Command-line scripts](script-cli.md) explains what this step leaves out, such as short flags like `-v`, repeated flags, rest arguments and `--`.

## When the input is wrong

When a script meets a value or a file that it cannot use, ok stops with an error that points at the code that failed. The samples in this section are short scripts, not the full `report.ok`, so that each error is easy to find.

### A value that is not a number

Suppose that someone typed a word in the quantity column:

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

ok stops at the first value that it cannot convert. It does not continue with a missing value or a zero. Every error has the same parts: a kind (here `value`), a message, the file with the line and column, and the line of code with the failing expression underlined. The script exits with code 1.

### Your own error message

The message above names the bad value, but not the row that it came from. To name the row, write your own message. `int(o.quantity)?` gives `null` where `int(o.quantity)` gives an error, `??` gives the value to use in place of a `null`, and `fail` stops the script with your message:

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

The last line of the error is a call trace. The error happened inside `quantity`, which was called on line 2.

The sample also shows two rules for functions. You can declare a function after the code that uses it. A function returns the value of its last expression, so `quantity` needs no `return`.

### A broken or missing file

When the file itself is wrong, the error gives the line of the data. A row with too few fields:

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

[Errors](errors.md) shows how to catch an error with `try` and `catch` when a script should recover and continue.

## Where to go next

- [Coming from bash and make](from-bash.md), if you want to replace existing shell scripts or Makefiles.
- [Running commands](commands.md), to start programs such as `git` or `cargo` from a script.
- [Tasks](tasks.md), to put a project's commands in `.ok/ok.ok` in place of a Makefile.
- The [log-summary](../examples/log-summary/) and [csv-to-json](../examples/csv-to-json/) examples, which do similar work and sit next to the bash and Python scripts that they replace.
