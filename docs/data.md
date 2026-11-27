# Data formats

Read and write JSON, JSON Lines, CSV, TSV, TOML, YAML, .env and text files with `read` and `write`, convert between the formats, and handle .gz files.

`read(path)` chooses the format from the file name, parses the file and returns ordinary ok values: records, lists, strings and numbers. `write(value, path)` does the reverse. Every format reads into the same values, so converting a CSV file to JSON takes one `read` and one `write`. The exact rules are in [section 13 of the spec](../SPEC.md#13-data-formats).

## Reading and writing a file

This script reads `package.json`, prints two of its fields, and writes a copy with a new version number:

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

`write` creates missing parent directories (`out/` here) and always writes `\n` line endings. It writes JSON pretty-printed, with a two-space indent and a final newline.

The value comes first and the path second, so `write` also works as a pipeline stage: `data | write("out.json")`. When the two arguments look swapped, as in `write("out.txt", "result")`, `write` stops with an error that says the value comes first.

## How ok picks the format

The extension of the file name decides the format. To use another format, name it as the second argument of `read` or the third argument of `write`.

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

A format that you name wins over the extension. `read("data.out", "json")` parses `data.out` as JSON, and `read("users.csv", "text")` gives the raw text of a CSV file. A format name that ok does not know is an error that lists the formats.

A file with an unknown extension is text. Writing a record to `settings.cfg` is therefore an error, because text output needs a string or a list of strings. Name the format to fix it: `write(cfg, "settings.cfg", "json")`.

The lazy formats are read as you consume them, so a pipeline over a big CSV or log file runs in bounded memory. See [Lazy data and big files](pipelines.md#lazy-data-and-big-files).

## CSV and TSV

The first row of a CSV file is the header, and every other row becomes a record keyed by it. Every value is a string. ok does not guess that `34` is a number or that `true` is a bool, because a wrong guess is hard to notice: a postcode loses its leading zero, or a version number becomes a float. Convert the fields you need with `int()`, `float()` or `time()`.

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

`read` accepts a UTF-8 byte order mark, Windows line endings, and quoted fields with `""` escapes and newlines inside the quotes. It skips empty lines. A row with the wrong number of fields is an error that gives its line number.

When you write a list of records, the header is the keys of the first record. A later record without one of those keys gets an empty field, and `null` becomes an empty field too. A field is quoted only when it needs quotes:

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

A record with a key that the first record lacks is an error. So is a field value that is a list or a record.

### Other separators and files without a header

`delimiter:` sets the separator for reading and for writing. `header: false` treats every row, the first included, as a list of strings. Also use `header: false` for a file whose header names a column twice. Such a header is an error otherwise, because one of the two columns would be lost.

This script reads a file separated by semicolons, with and without its header, and writes a file without a header:

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

TSV works the same way, with tabs as the separator and no quoting. A value that contains a tab or a newline cannot be written as TSV and is an error.

### Appending rows

`append: true` adds to an existing file. For CSV and TSV, the header is written only when the file is empty, so a log that every run adds to keeps a single header:

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

To convert a file, read it, reshape the values if you need to, and write them under a name with another extension. This is the `csv-to-json` example from the repository, slightly shortened:

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

For another format, change the extension of the output file: `--out active.yaml`, `--out active.jsonl` or `--out active.tsv`. [Command-line scripts](script-cli.md) explains how the parameters of `fn main` become command-line arguments.

## JSON and JSON Lines

JSON objects become records with their keys in document order. A number without a fraction or exponent that fits in a signed 64-bit int becomes an `int`, and every other number becomes a `float`. An integer above 9223372036854775807 therefore loses precision, as it does in JavaScript.

When you write JSON, a path becomes its `/` display form, a duration its literal form such as `"1m30s"`, and a time a UTC RFC 3339 string. Functions and bytes cannot be written as JSON.

JSON Lines (`.jsonl` or `.ndjson`) holds one JSON value per line. Logs and exports that grow over time often use it, and ok reads it lazily, one line at a time. The last line of this script uses `to`, which converts a value to text without writing a file (see [Converting in memory](#converting-in-memory-parse-and-to)):

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

`compact: true`, for `write` and `to`, writes JSON on one line without spaces. JSON Lines is always written that way.

## YAML

ok reads YAML 1.2 with the core schema. Only `true` and `false` are bools, so `yes`, `no` and `on` stay strings. `null`, `~` and an empty value are `null`. Anchors, aliases and merge keys (`<<: *defaults`) are resolved, as CI and compose files need.

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

A file with several documents separated by `---` is an error when read as `yaml`. To get the list of documents, read the file as text and parse it: `read(p, "text") | parse("yaml_all")`.

An empty YAML file reads as `null`, so `read(p) ?? {}` gives an empty record for an override file that has nothing in it.

## TOML

A TOML file reads into a record, and writing TOML needs a record.

A date-time with an offset becomes a `time` value. Local dates and times, which have no offset, stay strings because they do not name one instant. A `time` covers the years 1677 to 2262, so a date-time such as `9999-12-31T00:00:00Z` is an error. Quote it in the file to read it as a string.

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

A time prints in your local time zone. Format it with `utc: true` when the output must be the same on every machine.

## .env files

A `.env` file, or one named like `.env.local` or `.env.production`, reads into a record of strings. Blank lines and `#` comments are skipped, and a leading `export ` is allowed. Text in single quotes is literal, and text in double quotes may use the escapes `\n`, `\t`, `\"` and `\\`. Variables are not expanded, so `${HOME}` stays as written.

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

To give the values to the commands that the script runs, set each one with `env.set`: `for key, value in local { env.set(key, value) }`.

## Text and lines

The `text` format gives the whole file as one string. The `lines` format, like the `lines` function applied to text, gives a list of lines without their line endings. `write` writes a string as it is, and a list of strings as one line per string.

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

Reading a file as text when it is not valid UTF-8 is an error that suggests `read(p, "bytes")`.

## Converting in memory: parse and to

`parse(text, format)` and `to(value, format)` do what `read` and `write` do, without a file. Use them for text that comes from somewhere else, such as the output of a command or the body of an HTTP response:

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

Write JSON text in a single-quoted string, where `{` does not start an interpolation.

To read the JSON that a program prints, pipe the command into `parse`, as in `let meta = run cargo metadata --format-version 1 | parse("json")`. [Running commands](commands.md) explains `run`.

## Merging configuration

Spread combines two records: in `{...base, ...override}`, a key in `override` wins. This merge is shallow, so a nested record in `override` replaces the whole nested record in `base`. Configuration in layers usually needs a deep merge, which combines nested records key by key. ok has no built-in function for a deep merge. The `merge` function in this script does it:

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

The first line is the shallow merge, which lost `port`. The second line is the deep merge, which kept it. The `config-render` example in the repository does this for one environment and writes the result as JSON.

## Compressed files

When a file name ends in `.gz`, in any letter case, `read` decompresses the file and `write` compresses it with gzip. The format comes from the rest of the name, so `events.jsonl.gz` is JSON Lines. It stays lazy and is decompressed as it is read:

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

The other rules for `.gz` files:

- A `.gz` name without an inner extension, such as `data.gz`, is text.
- A format that you name replaces only the format, and the file is still decompressed. `gzip: false` reads or writes the bytes as stored, which is why the last line of the output above is `not text`. `gzip: true` compresses a file whose name does not end in `.gz`.
- `append: true` adds a new gzip member, and `read` joins the members into one stream. A CSV header is still written once.
- The written bytes are the same on every OS. The gzip header holds no file name and no time, and the compression level is fixed.
- `parse`, `to` and `stdin()` do not decompress.

The spec defines the details under [Compressed files](../SPEC.md#1311-compressed-files).

## Reading standard input

`stdin(format)` reads ok's own standard input, lazily, in any of the formats on this page. The default format is `text`. With `stdin`, a script can process what another program pipes into it:

```ok file=count-errors.ok
let n = stdin() | lines | where .contains("ERROR") | count
print("{n} errors")
```

In a shell, pipe the output of a program into the script, or into a one-line program given with `-e`:

```sh
some-server --verbose 2>&1 | ok count-errors.ok
curl -s https://api.example.com/items | ok -e 'print(stdin("json") | count)'
```

`stdin("csv")` and `stdin("jsonl")` take the same options as `read`, such as `delimiter:` and `header:`.

## Size limits

`read` and `stdin` stop with an `io` error when their input is larger than a limit, so that a file from an unknown source cannot fill your memory. A small `.gz` file, for example, can unpack to gigabytes.

- A whole document (JSON, TOML, YAML, .env or bytes) may hold up to 1 GiB.
- One line of text, lines or JSON Lines, and one CSV record, may hold up to 64 MiB. These formats stream, so a file of many ordinary lines has no overall limit.

To set another limit, pass `max_size:` or `max_line:` in bytes. Pass `null` for no limit.

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

## Options of each function

This table lists the named options that each function on this page accepts. The [standard library](stdlib.md) page describes each function in full.

| Function | Options |
|---|---|
| `read(path, format)` | `delimiter`, `header`, `gzip`, `max_size`, `max_line` |
| `write(value, path, format)` | `append`, `compact`, `delimiter`, `header`, `gzip` |
| `parse(text, format)` | `delimiter`, `header` |
| `to(value, format)` | `compact`, `delimiter`, `header` |
| `stdin(format)` | `delimiter`, `header`, `max_size`, `max_line` |
