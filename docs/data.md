# Data formats

Read and write JSON, JSON Lines, CSV, TSV, TOML, YAML, .env and text with one pair of functions, convert between them, and handle .gz files.

ok has no separate parser per format. `read(path)` looks at the file name, parses the file and gives you ordinary ok values: records, lists, strings and numbers. `write(value, path)` does the reverse. Converting CSV to JSON is therefore a `read` and a `write`. The fine print is in [section 13 of the spec](../SPEC.md#13-data-formats).

## Reading and writing a file

```json file=package.json
{
  "name": "shop",
  "version": "1.4.0",
  "scripts": {"build": "vite build", "test": "vitest"},
  "private": true
}
```

```ok file=pkg.ok
let pkg = read("package.json")
print(pkg.name, pkg.version)
print(pkg.scripts | keys)

write({...pkg, version: "1.5.0"}, "out/package.json")
print(read("out/package.json").version)
```

```output
shop 1.4.0
["build", "test"]
1.5.0
```

`write` creates missing parent directories (`out/` here), always writes `\n` line endings, and writes JSON pretty-printed with a two-space indent and a final newline. The value comes first and the path second, as in every pipeline stage, so `data | write("out.json")` works too.

## How the format is picked

The extension decides, unless you name a format as the second argument of `read` or the third of `write`:

| Format | Extensions | `read` gives |
|---|---|---|
| `json` | `.json` | any value |
| `jsonl` | `.jsonl`, `.ndjson` | a lazy list, one value per line |
| `csv` | `.csv` | a lazy list of records of strings |
| `tsv` | `.tsv` | a lazy list of records of strings |
| `toml` | `.toml` | a record |
| `yaml` | `.yaml`, `.yml` | any value |
| `env` | `.env`, and names starting with `.env.` | a record of strings |
| `text` | `.txt`, `.log`, `.md`, and anything unknown | a lazy string |
| `lines` | only when asked for | a lazy list of strings |
| `bytes` | only when asked for | bytes |

An explicit format wins over the name: `read("data.out", "json")`, or `read("users.csv", "text")` for the raw text of a CSV file. A format name ok does not know is an error that lists the ones it does.

The lazy formats are read as you consume them, so a pipeline over a big CSV or log file runs in bounded memory. See [Lazy data and big files](pipelines.md#lazy-data-and-big-files).

Any file with an unknown extension is text. Writing a record to `settings.cfg` is therefore an error (text output needs a string), and the fix is to name the format: `write(cfg, "settings.cfg", "json")`.

## CSV and TSV

The first row of a CSV file is the header, and every other row becomes a record keyed by it. All values are strings: ok never guesses that `34` is a number or `true` a bool, because a guess that goes wrong (a postcode losing its leading zero, a version number becoming a float) is hard to notice. Convert the fields you need with `int()`, `float()` or `time()`.

```csv file=users.csv
name,email,age,active
Ana,ana@example.com,34,true
Bo,bo@example.com,27,false
Cy,"cy@example.com",41,true
```

```ok file=users.ok
let users = read("users.csv")
print(users[0])
print(users | where .active == "true" | map .name)
print(users | sum(u => int(u.age)))
```

```output
{name: "Ana", email: "ana@example.com", age: "34", active: "true"}
["Ana", "Cy"]
102
```

Reading accepts a UTF-8 byte order mark, Windows line endings, quoted fields with `""` escapes and newlines inside quotes, and skips empty lines. A row with the wrong number of fields is an error that gives its line number.

When you write a list of records, the header is the keys of the first record. A later record without one of those keys gets an empty field, `null` becomes an empty field, and fields are quoted only when they need it:

```ok file=write-csv.ok
let rows = [
  {name: "ana", note: "says \"hi\", then leaves"},
  {name: "bo", note: null},
]
write(rows, "notes.csv")
print(read("notes.csv", "text"))
```

```output
name,note
ana,"says ""hi"", then leaves"
bo,
```

A record with a key that the first record lacks is an error, as is a list or record as a field value.

### Options

`delimiter:` reads and writes other separators, and `header: false` treats every row, the first included, as a list of strings. Use it also for a file whose header names a column twice, which is an error otherwise, since one of the two columns would be lost:

```text file=scores.txt
team;points
red;12
blue;9
```

```ok file=options.ok
print(read("scores.txt", "csv", delimiter: ";"))
print(read("scores.txt", "csv", delimiter: ";", header: false) | take 2)
write([["x", "y"], [1, 2]], "plain.csv", header: false)
print(read("plain.csv", "text"))
```

```output
[{team: "red", points: "12"}, {team: "blue", points: "9"}]
[["team", "points"], ["red", "12"]]
x,y
1,2
```

TSV works the same way with tabs and no quoting; a value that contains a tab or a newline cannot be written as TSV and is an error.

### Appending rows

`append: true` adds to an existing file. For CSV and TSV the header is written only when the file is empty, so a log you add to in each run keeps a single header:

```ok file=append.ok
write([{run: "1", result: "pass"}], "runs.csv", append: true)
write([{run: "2", result: "fail"}], "runs.csv", append: true)
print(read("runs.csv", "text"))
```

```output
run,result
1,pass
2,fail
```

## Converting CSV to JSON

Because every format reads into the same values, a conversion is a read, some reshaping, and a write. This is the `csv-to-json` example from the repository, slightly shortened:

```csv file=users.csv
name,email,age,active
Ana,ana@example.com,34,true
Bo,bo@example.com,27,false
Cy,cy@example.com,41,true
```

```ok file=to-json.ok
# Convert a CSV export to JSON, keeping only active users.
fn main(input: path, --out: path = "out.json") {
  # CSV values are always strings; convert the ones that are numbers.
  let users = read(input)
    | where .active == "true"
    | sort .name
    | map(u => {name: u.name, age: int(u.age)})

  write(users, out)
  print("wrote {len(users)} users to {out}")
  print(read(out, "text"))
}
```

```output run="ok to-json.ok users.csv --out active.json"
wrote 2 users to active.json
[
  {
    "name": "Ana",
    "age": 34
  },
  {
    "name": "Cy",
    "age": 41
  }
]
```

Writing the same list to `active.yaml`, `active.jsonl` or `active.tsv` gives the other formats. [Script arguments](script-cli.md) explains how `fn main` turns into the command-line options.

## JSON and JSON Lines

JSON objects become records with their keys in document order, and numbers without a fraction or exponent that fit in a signed 64-bit int become `int`, the rest `float` (so an integer above 9223372036854775807 loses precision, as in JavaScript). When writing, paths become their `/` display form, durations their literal form such as `"1m30s"`, and times UTC RFC 3339 strings. Functions and bytes cannot be written as JSON.

JSON Lines (`.jsonl` or `.ndjson`) holds one JSON value per line. It is the usual format for logs and exports that grow over time, and ok reads it lazily, one line at a time:

```json file=events.jsonl
{"type": "login", "user": "ana"}
{"type": "upload", "user": "ana", "size": 1200}
{"type": "login", "user": "bo"}
```

```ok file=events.ok
let events = read("events.jsonl")
print(events | count .type == "login")
print(events | where .type == "upload" | map .size)

print(to({name: "ok", tags: ["a", "b"]}, "json", compact: true))
```

```output
2
[1200]
{"name":"ok","tags":["a","b"]}
```

`compact: true`, for `write` and `to`, writes JSON on one line without spaces instead of pretty-printing it. JSON Lines is always written that way.

## YAML

ok reads YAML 1.2 with the core schema. Only `true` and `false` are booleans, so `yes`, `no` and `on` stay strings, and `null`, `~` and an empty value are `null`. Anchors, aliases and merge keys (`<<: *defaults`) are resolved, which covers CI and compose files.

```yaml file=ci.yaml
defaults: &defaults
  image: rust:1.100
  retries: 2
jobs:
  test:
    <<: *defaults
    script: cargo test
  lint:
    <<: *defaults
    retries: 0
    script: cargo clippy
notify: yes
```

```ok file=ci.ok
let ci = read("ci.yaml")
for name, job in ci.jobs {
  print(name, job.image, job.retries)
}
print(ci.notify, type(ci.notify))
```

```output
test rust:1.100 2
lint rust:1.100 0
yes string
```

A file with several documents separated by `---` is an error when read as `yaml`; `parse(text, "yaml_all")` gives the list of documents. An empty YAML file reads as `null`, which makes `read(p) ?? {}` a handy default for an optional override file.

## TOML

TOML files read into records. Date-times with an offset become `time` values, while local dates and times (without an offset) stay strings, because they do not name one instant. A `time` covers 1677 to 2262, so a date-time like `9999-12-31T00:00:00Z` is an error; quote it in the file to read it as a string. Writing TOML needs a record.

```toml file=release.toml
title = "spring"

[deploy]
started = 2026-11-25T10:00:00Z
freeze = 2026-12-20
```

```ok file=release.ok
let release = read("release.toml")
let started = release.deploy.started
print(type(started), started.format("%Y-%m-%d %H:%M", utc: true))
print(release.deploy.freeze, type(release.deploy.freeze))
print(to({title: "autumn", server: {port: 80}}, "toml"))
```

```output
time 2026-11-25 10:00
2026-12-20 string
title = "autumn"

[server]
port = 80
```

Times print in your local time zone, so format them with `utc: true` when the output must be the same on every machine.

## .env files

A `.env` file (or `.env.local`, `.env.production` and so on) reads into a record of strings. Blank lines and `#` comments are skipped, a leading `export ` is allowed, single quotes are literal and double quotes understand `\n`, `\t`, `\"` and `\\`. There is no variable expansion: `${HOME}` stays as written.

```text file=.env
# local settings
export DB_HOST=localhost
DB_PASSWORD='s3cret #1'
GREETING="hello\nworld"
```

```ok file=dotenv.ok
let local = read(".env")
print(local.DB_HOST, local.DB_PASSWORD)
print(local.GREETING)
print(to({PORT: 8080, MOTTO: "has spaces"}, "env"))
```

```output
localhost s3cret #1
hello
world
PORT=8080
MOTTO="has spaces"
```

To make the values available to the commands you run, pass them to `env.set`, for example `for key, value in local { env.set(key, value) }`.

## Text and lines

`text` gives the whole file as one string, and `lines` (or the `lines` function on text) gives a list of lines without their line endings. Writing a string writes it as it is; writing a list of strings writes one per line.

```ok file=text.ok
write(["first", "second"], "notes.txt")
write(["third"], "notes.txt", append: true)
print(read("notes.txt") | lines)
print(read("notes.txt", "lines") | count)
```

```output
["first", "second", "third"]
3
```

`read` of a text file that is not valid UTF-8 is an error that suggests `read(p, "bytes")`.

## Converting in memory: parse and to

`parse(text, format)` and `to(value, format)` do what `read` and `write` do, without a file. Use them for command output, HTTP bodies and strings you build yourself:

```ok file=parse.ok
print(parse('{"id": 7, "tags": ["x"]}', "json"))
print(parse("x,y\n1,2\n", "csv"))
print(parse("a: 1\n---\nb: 2\n", "yaml_all"))
print(to([{a: 1}, {a: 2}], "jsonl"))
```

```output
{id: 7, tags: ["x"]}
[{x: "1", y: "2"}]
[{a: 1}, {b: 2}]
{"a":1}
{"a":2}
```

JSON is easiest to write in a single-quoted string, where `{` does not start an interpolation. A common use is reading a tool's JSON output by piping it into `parse`, as in `let meta = run cargo metadata --format-version 1 | parse("json")`; see [Running commands](commands.md).

## Merging configuration

Records combine with spread: in `{...base, ...override}` later keys win. That is a shallow merge: a nested record in `override` replaces the whole nested record in `base`. For layered configuration you usually want a deep merge, which takes a few lines:

```yaml file=base.yaml
server:
  host: localhost
  port: 8080
log_level: info
```

```yaml file=production.yaml
server:
  host: app.example.com
log_level: warn
```

```ok file=config.ok
# Deep-merge two records; values from `b` win, nested records are merged.
fn merge(a: record, b: record) {
  var out = a
  for key, value in b {
    out[key] = if type(value) == "record" and type(a[key]?) == "record" {
      merge(a[key], value)
    } else {
      value
    }
  }
  out
}

let base = read("base.yaml")
let prod = read("production.yaml") ?? {}
print({...base, ...prod})
print(merge(base, prod))
```

```output
{server: {host: "app.example.com"}, log_level: "warn"}
{server: {host: "app.example.com", port: 8080}, log_level: "warn"}
```

The shallow merge lost `port`; the deep merge kept it. The `config-render` example in the repository does this for one environment and writes the result as JSON.

## Compressed files

A name that ends in `.gz` (in any letter case) means gzip: `read` decompresses and `write` compresses. The format comes from the rest of the name, so `events.jsonl.gz` is JSON Lines and stays lazy, decompressed as it is read:

```ok file=gzip.ok
write(range(1, 1001) | map(n => {n, even: n % 2 == 0}), "numbers.jsonl.gz")
print(read("numbers.jsonl.gz") | where .even | count)
print(read("numbers.jsonl.gz") | first)

write({name: "shop", port: 80}, "settings.json.gz")
print(read("settings.json.gz"))
let raw = read("settings.json.gz", "bytes", gzip: false)
print(text(raw)? ?? "not text")
```

```output
500
{n: 1, even: false}
{name: "shop", port: 80}
not text
```

Some details:

- A `.gz` name without an inner extension, such as `data.gz`, is text.
- An explicit format replaces only the format; the file is still decompressed. `gzip: false` reads or writes the bytes as stored, and `gzip: true` compresses a file whose name does not end in `.gz`.
- `append: true` adds a new gzip member, and `read` joins the members into one stream. CSV headers are still written once.
- The written bytes are the same on every OS: no file name or time in the header, and a fixed compression level.
- `parse`, `to` and `stdin()` do not decompress.

See [Compressed files](../SPEC.md#1311-compressed-files) in the spec.

## Reading standard input

`stdin(format)` reads ok's own standard input, lazily, in any of the formats above (default `text`). It lets a script sit in a shell pipeline:

```ok file=count-errors.ok
let n = stdin() | lines | where .contains("ERROR") | count
print("{n} errors")
```

```sh
some-server --verbose 2>&1 | ok count-errors.ok
curl -s https://api.example.com/items | ok -e 'print(stdin("json") | count)'
```

`stdin("csv")` and `stdin("jsonl")` take the same options as `read`, such as `delimiter:` and `header:`.

## Options at a glance

| Function | Options |
|---|---|
| `read(path, format)` | `delimiter`, `header`, `gzip`, `max_size`, `max_line` |
| `write(value, path, format)` | `append`, `compact`, `delimiter`, `header`, `gzip` |
| `parse(text, format)` | `delimiter`, `header` |
| `to(value, format)` | `compact`, `delimiter`, `header` |
| `stdin(format)` | `delimiter`, `header`, `max_size`, `max_line` |

## Size limits

A data file may come from anywhere, and a small `.gz` file can unpack to gigabytes. So `read` and `stdin` stop with an `io` error rather than fill your memory: a whole document (JSON, TOML, YAML, .env, bytes) may hold up to 1 GiB, and one line of text, lines or JSON Lines, or one CSV record, up to 64 MiB. The lazy formats stream, so a file of many ordinary lines has no overall limit. Pass `max_size:` or `max_line:` (in bytes) for other limits, or `null` for none:

```ok file=limits.ok
write("one\na line that is far too long\n", "notes.txt")
let r = try { read("notes.txt", max_line: 10) | lines | count } catch e { e.message }
print(r)
print(read("notes.txt", max_line: 100) | lines | count)
```

```output
cannot read notes.txt: line 2 is longer than 10 bytes
2
```

`write` refuses arguments that look swapped, such as `write("out.txt", "result")`, with a hint that the value comes first. The reference for each function is in the [standard library](stdlib.md).
