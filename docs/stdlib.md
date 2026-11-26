# Standard library

Every builtin function of ok, grouped by area: its signature, what it returns, its named arguments and the errors it raises.

All of these are available in every script without an import. Each one also works in method form, so `s.trim()` is `trim(s)`, and as a pipeline stage, where the value on the left becomes the first argument: `xs | sort .name` is `sort(xs, .name)` (see [pipelines](pipelines.md)). The signatures use ok's parameter syntax: `name: type`, `= v` for a default, `?` for a type that may be `null`, and `--name` for a named argument, which you pass as `name: value`. The normative definitions are in [the spec](../SPEC.md#12-the-standard-library).

Errors are shown by kind, such as a `type` error or an `io` error. You can catch them with `try` or `?` (see [errors](errors.md)); the kind is the `kind` field of the error record.

- [Output and control](#output-and-control): `print`, `eprint`, `fail`, `exit`, `prompt`, `confirm`
- [Values and numbers](#values-and-numbers): `str`, `int`, `float`, `type`, `len`, `bytes`, `text`, `fmt`, `floor`, `ceil`, `round`, `range`
- [Lists](#lists): `where`, `map`, `each`, `sort`, `reverse`, `uniq`, `group`, `flat`, `enumerate`, `take`, `skip`, `count`, `sum`, `min`, `max`, `first`, `last`, `any`, `all`, `join`
- [Records](#records): `keys`, `values`, `entries`, `from_entries`, `get`, `has`
- [Strings](#strings): `trim`, `trim_start`, `trim_end`, `split`, `lines`, `chars`, `replace`, `contains`, `starts_with`, `ends_with`, `upper`, `lower`, `repeat`, `pad`
- [Regular expressions](#regular-expressions): `match`, `matches`, `find`, `sub`
- [Files](#files): `copy`, `move`, `remove`, `mkdir`, `exists`, `is_file`, `is_dir`, `glob`, `list_dir`, `stat`, `touch`, `tempdir`, `tempfile`, `chmod`
- [Paths](#paths): `path`, `abs`, `relative_to`, `with_ext`, `parts`, `is_abs`
- [Data formats](#data-formats): `read`, `write`, `parse`, `to`, `stdin`
- [Commands](#commands): `which`
- [Dates and durations](#dates-and-durations): `now`, `time`, `format`, `sleep`, `duration`
- [Environment and system](#environment-and-system): `env`, `os` and the other global values
- [Hashing](#hashing): `hash`
- [Network](#network): `fetch`
- [Archives](#archives): `zip`, `unzip`, `tar`, `untar`

## Output and control

```ok file=hello.ok
let name = "world"
print("hello,", name)
print([1, 2], {a: 1}, path("src/main.rs"))
eprint("this line goes to stderr")
exit(3)
```

```output exit=3
hello, world
[1, 2] {a: 1} src/main.rs
this line goes to stderr
```

### print

`print(...xs)` returns `null`. Writes the display form of each argument, separated by spaces, then a newline, to stdout. `print()` writes an empty line. If stdout has been closed (output piped into a program that stopped reading), the script ends quietly with exit code 0.

### eprint

`eprint(...xs)` returns `null`. Like `print`, but to stderr. Use it for progress messages and warnings, so they do not end up in captured output.

### fail

`fail(msg)` never returns. Raises a `fail` error with the display form of `msg` as its message. Given an error record caught with `try`, it raises that error again with its original kind and location.

### exit

`exit(code: int = 0)` never returns. Ends the script with `code` after cleaning up (temporary files and directories are removed). A code outside 0 to 255 is an `arg` error.

### prompt

`prompt(msg: string, --default: string? = null)` returns a `string`. Asks for a line on the terminal and returns what was typed. Without a terminal it returns `default`, or raises a `value` error if there is none.

### confirm

`confirm(msg: string, --default: bool = false)` returns a `bool`. Asks a yes/no question on the terminal; `default` is the answer when the user just presses Enter. With `ok --yes` it returns `true` without asking. Without a terminal and without `--yes` it raises a `value` error, even when a default is given.

```ok
let name = prompt("Your name?", default: "anonymous")
if confirm("Delete the build directory?") {
  remove("build")
}
```

## Values and numbers

```ok file=values.ok
print(str(1.0), int("42"), int(-2.7), float("1.5"), int(true))
print(type(1), type(1.5), type("x"), type([]), type(null), type(2s))
print(len("héllo"), len(bytes("héllo")), len([1, 2]), len({a: 1}))
print(floor(2.7), ceil(2.1), round(2.5), round(-2.5))
print(fmt(3.14159, ".2f"), fmt(1234567, ","), fmt(0.256, ".1%"), fmt(255, "x"))
print(range(3), range(1, 4), range(10, 0, -3))
```

```output
1.0 42 -2 1.5 1
int float string list null duration
5 6 2 1
2 3 3 -3
3.14 1,234,567 25.6% ff
[0, 1, 2] [1, 2, 3] [10, 7, 4, 1]
```

### str

`str(x)` returns a `string`: the display form of `x`, the same text `print` writes. Strings are returned as they are; `null` becomes `"null"`.

### int

`int(x)` returns an `int`. Converts a string (surrounding whitespace is ignored), a float (truncated toward zero) or a bool (`1` or `0`). A malformed string is a `value` error, a float too large for an int is a `value` error, and any other type is a `type` error.

### float

`float(x)` returns a `float`. Converts a string, an int or a bool. A malformed string is a `value` error.

### type

`type(x)` returns a `string`: one of `null`, `bool`, `int`, `float`, `string`, `bytes`, `list`, `record`, `path`, `duration`, `time` or `fn`.

### len

`len(x)` returns an `int`: the number of characters of a string (Unicode scalar values, not bytes), bytes of a bytes value, elements of a list or fields of a record. Anything else is a `type` error.

### bytes

`bytes(s: string)` returns `bytes`: the UTF-8 encoding of `s`. Bytes are returned as they are.

### text

`text(b: bytes)` returns a `string`: `b` decoded as UTF-8. Invalid UTF-8 is a `value` error that says at which byte it fails.

### fmt

`fmt(x, spec: string)` returns a `string`: `x` formatted by a format spec, the same one you write in `"{x:spec}"`. The spec is `[[fill]align][0][width][,][.precision][type]`, with align `<`, `>` or `^` and type one of `d f e x X o b %`. Strings align left by default, numbers right. An invalid spec is a `format` error. See [§12.6 of the spec](../SPEC.md#126-format-specs).

### floor

`floor(x)` returns an `int`: `x` rounded down. An int is returned as it is.

### ceil

`ceil(x)` returns an `int`: `x` rounded up.

### round

`round(x)` returns an `int`: `x` rounded to the nearest whole number, with halves rounded away from zero (`round(2.5)` is `3`, `round(-2.5)` is `-3`). For `floor`, `ceil` and `round`, a result too large for an int is a `value` error and a non-number is a `type` error.

### range

`range(n)` or `range(start, end, step = 1)` returns a lazy list of ints from `start` (default `0`) up to but not including `end`. A negative `step` counts down. Because it is lazy, `range(1_000_000_000) | take 3` only produces three numbers. A step of `0` is an `arg` error.

## Lists

The list functions take the list first, so they read well in pipelines. A `pred` or `key` argument is a function, usually an implicit closure such as `.age > 30` (see [functions](functions.md)). The functions marked *streams* work element by element on lazy input, such as `read("big.log") | lines`, without reading it all into memory.

```ok file=lists.ok
let people = [
  {name: "ana", age: 34, team: "web"},
  {name: "bo", age: 27, team: "data"},
  {name: "cy", age: 41, team: "web"},
]
print(people | where .age > 30 | map(.name))
print(people | sort(.age, desc: true) | map(.name))
print(people | group .team | keys)
print(people | sum .age, people | max .age | .name, people | count .team == "web")
print(people | first(.age < 30) | .name, any(people, .age > 40), all(people, .age > 30))
print(uniq([3, 1, 3, 2]), flat([[1, 2], [3]]), take([1, 2, 3], 2), skip([1, 2, 3], 2))
print(people | map(.name) | join(", "))
```

```output
["ana", "cy"]
["cy", "ana", "bo"]
["web", "data"]
102 cy 2
bo true false
[3, 1, 2] [1, 2, 3] [1, 2] [3]
ana, bo, cy
```

### where

`where(xs: list, pred: fn)` returns a list of the elements for which `pred` returns `true`. Streams. `pred` must return a bool; anything else is a `type` error, so write `.count > 0` rather than `.count`.

### map

`map(xs: list, f: fn)` returns a list of `f(x)` for each element. Streams.

### each

`each(xs: list, f: fn)` returns `null`. Calls `f` on each element for its effect, such as printing. Consumes lazy input element by element.

### sort

`sort(xs: list, key: fn? = null, --desc: bool = false)` returns a sorted list. The sort is stable. Without `key` it sorts the values themselves; with `key` it sorts by `key(x)`. `desc: true` sorts from largest to smallest. Values that cannot be compared, such as a string with an int, are a `type` error.

### reverse

`reverse(xs: list)` returns the list in reverse order.

### uniq

`uniq(xs: list, key: fn? = null)` returns the list with only the first of each distinct value (or each distinct `key(x)`). The input does not need to be sorted. Streams.

### group

`group(xs: list, key: fn)` returns a record. Its keys are `str(key(x))`, in order of first appearance, and each value is the list of elements with that key.

### flat

`flat(xs: list)` returns the list with nested lists flattened by one level: `flat([[1, [2]], 3])` is `[1, [2], 3]`. Streams.

### enumerate

`enumerate(xs: list)` returns a list of `{index, value}` records, with `index` counting from 0. Streams.

### take

`take(xs: list, n: int)` returns the first `n` elements (fewer if the list is shorter). Streams and stops reading after `n`. A negative `n` is an `arg` error.

### skip

`skip(xs: list, n: int)` returns the list without its first `n` elements. Streams.

### count

`count(xs: list, pred: fn? = null)` returns an `int`: the number of elements, or the number for which `pred` is `true`.

### sum

`sum(xs: list, key: fn? = null)` returns a number: the sum of the elements (or of `key(x)`). An empty list sums to `0`, and durations sum to a duration. Strings are a `type` error; CSV values are strings, so convert them first, as in `sum(rows, r => int(r.amount))`.

### min

`min(xs: list, key: fn? = null)` returns the smallest element, or `null` for an empty list. With `key`, it compares `key(x)` and returns the element itself.

### max

`max(xs: list, key: fn? = null)` returns the largest element, or `null` for an empty list. With `key`, it returns the element with the largest `key(x)`.

### first

`first(xs: list, pred: fn? = null)` returns the first element, or the first for which `pred` is `true`, or `null` if there is none. Stops reading as soon as it has found one.

### last

`last(xs: list, pred: fn? = null)` returns the last element, or the last for which `pred` is `true`, or `null`.

### any

`any(xs: list, pred: fn)` returns `true` if `pred` is `true` for at least one element. `false` for an empty list. Stops at the first match.

### all

`all(xs: list, pred: fn)` returns `true` if `pred` is `true` for every element. `true` for an empty list. Stops at the first miss.

### join

`join(xs: list, sep: string = "")` returns a `string`: the elements converted with `str` and joined by `sep`. Lists, records and bytes inside `xs` are a `type` error.

## Records

```ok file=records.ok
let config = {name: "app", port: 8080}
print(keys(config), values(config))
print(entries(config))
print(config | entries | map(e => {key: e.key, value: str(e.value)}) | from_entries)
print(get(config, "port"), get(config, "host"), get(config, "host", "localhost"))
print(has(config, "name"), config.has("debug"))
```

```output
["name", "port"] ["app", 8080]
[{key: "name", value: "app"}, {key: "port", value: 8080}]
{name: "app", port: "8080"}
8080 null localhost
true false
```

### keys

`keys(r: record)` returns a list of the field names, in order.

### values

`values(r: record)` returns a list of the field values, in order.

### entries

`entries(r: record)` returns a list of `{key, value}` records, one per field.

### from_entries

`from_entries(xs: list)` returns a record built from a list of `{key, value}` records: the inverse of `entries`. An element that is not such a record is a `type` error.

### get

`get(r: record, key: string, default = null)` returns the field `key` of `r`, or `default` if there is no such field. Use it for field names that are only known at run time, or that may be missing.

### has

`has(r: record, key: string)` returns `true` if `r` has a field named `key`. For `get` and `has`, a value that is not a record is a `type` error.

## Strings

All string functions work on Unicode text. A non-string where a string is needed is a `type` error.

```ok file=strings.ok
print("  padded  ".trim() + "|", "a b  c".split(), "k=v=w".split("=", limit: 2))
print("x-y-z".replace("-", "+"), "Hello".upper(), "héllo".chars())
print("hello".contains("ell"), [1, 2].contains(2), "app.log".ends_with(".log"))
print("ab".repeat(3), pad("7", 3, fill: "0", align: "right"))
print("[" + pad("x", 5, align: "center") + "]")
print(lines("one\ntwo\r\nthree\n"))
```

```output
padded| ["a", "b", "c"] ["k", "v=w"]
x+y+z HELLO ["h", "é", "l", "l", "o"]
true true true
ababab 007
[  x  ]
["one", "two", "three"]
```

### trim

`trim(s: string)` returns `s` without whitespace at the start and the end.

### trim_start

`trim_start(s: string)` returns `s` without whitespace at the start.

### trim_end

`trim_end(s: string)` returns `s` without whitespace at the end.

### split

`split(s: string, sep: string? = null, --limit: int? = null)` returns a list of strings. Without `sep` it splits on runs of whitespace and drops empty parts; with `sep` it splits on every occurrence, so `"a,,b".split(",")` is `["a", "", "b"]`. `limit` caps the number of parts: the last part holds the rest. An empty `sep` is an `arg` error; use `chars` instead.

### lines

`lines(x)` returns a list of strings: `x` split on `\n` and `\r\n`, without a final empty line. On a list of strings it splits each one and flattens the result. Streams, so `read("big.log") | lines | count` reads the file in bounded memory.

### chars

`chars(s: string)` returns a list of the characters of `s`, each a one-character string.

### replace

`replace(s: string, old: string, new: string)` returns `s` with every occurrence of `old` replaced by `new`. The match is literal; for patterns use [`sub`](#sub).

### contains

`contains(x, item)` returns a `bool`: whether a string contains a substring, a list contains an element, or a record has a key. A string can only contain a string; anything else is a `type` error. The operator `item in x` does the same.

### starts_with

`starts_with(s: string, prefix: string)` returns `true` if `s` starts with `prefix`.

### ends_with

`ends_with(s: string, suffix: string)` returns `true` if `s` ends with `suffix`.

### upper

`upper(s: string)` returns `s` in upper case.

### lower

`lower(s: string)` returns `s` in lower case.

### repeat

`repeat(s: string, n: int)` returns `s` repeated `n` times. A negative `n` is an `arg` error.

### pad

`pad(x, width: int, --fill: string = " ", --align: string = "left")` returns a `string`: `str(x)` padded with `fill` to `width` characters. `align` is `"left"`, `"right"` or `"center"`. A string that is already wider is returned unchanged.

## Regular expressions

Regular expressions use the syntax of the Rust `regex` crate: it is like RE2, matches in linear time and has no backreferences. Write them in single-quoted strings, so that `\d` is not an escape. An invalid pattern is a `value` error.

```ok file=regex.ok
let m = "version 1.24.3 released".match('(\d+)\.(\d+)\.(?P<patch>\d+)')
print(m.text, m.groups, m.named.patch, m.start, m.end)
print("no digits".match('\d'), "abc123".matches('^[a-z]+\d+$'))
print("a1b22c333".find('\d+') | map(.text))
print("2026-11-24".sub('(\d+)-(\d+)-(\d+)', "$3/$2/$1"))
print("a1b2".sub('\d', d => "<{int(d.text) * 10}>"))
```

```output
1.24.3 ["1", "24", "3"] 3 8 14
null true
["1", "22", "333"]
24/11/2026
a<10>b<20>
```

### match

`s.match(re: string)` returns a `record?`: the first match as `{text, start, end, groups, named}`, or `null` if there is none. `start` and `end` are positions in `s`; `groups` is the list of capture groups (`null` for a group that did not take part) and `named` a record of the named groups. Because `match` is a keyword, you can only call it in method form.

### matches

`matches(s: string, re: string)` returns `true` if `re` matches anywhere in `s`. Use `^` and `$` to match the whole string.

### find

`find(s: string, re: string)` returns a list of match records, one per non-overlapping match, in the same form as `match`.

### sub

`sub(s: string, re: string, replacement)` returns `s` with every match of `re` replaced. A string replacement may use `$1` or `${name}` for groups and `$$` for a literal `$`. A function replacement is called with each match record and returns the replacement text.

## Files

Relative paths are resolved against the working directory (the project root when you run a task). File functions accept a path or a string. The functions that change files (`write`, `copy`, `move`, `remove`, `mkdir`, `touch`, `chmod` and the archive functions) are printed by `ok --trace` and skipped by `ok --dry-run`. The guide page on [files](files.md) shows them at work.

```ok file=files.ok
write("hello\n", "src/a.txt")
write("x", "src/b.md")
write("y", "src/.hidden")
copy("src", "backup")
move("backup/a.txt", "backup/c.txt")
print(list_dir("backup"))
print(glob("**/*.txt"), glob("src/*", hidden: true))
print(exists("src"), is_file("src"), is_dir("src"))
print(stat("src/a.txt").size, mkdir("build/out"))
print(remove("backup"), remove("backup"))
```

```output
[path("backup/.hidden"), path("backup/b.md"), path("backup/c.txt")]
[path("backup/c.txt"), path("src/a.txt")] [path("src/.hidden"), path("src/a.txt"), path("src/b.md")]
true false true
6 build/out
true false
```

### copy

`copy(src: path, dst: path)` returns `null`. Copies a file, or a directory with everything in it. `dst` is the exact destination, never a directory to copy into: `copy("a.txt", "out")` creates a file named `out`. Existing files are overwritten and parent directories are created. Copying a directory onto an existing directory merges them: files with the same name are replaced, the other files in `dst` stay. A missing `src`, a file copied onto an existing directory (or a directory onto a file), and a path copied onto itself or a directory into itself are `io` errors.

### move

`move(src: path, dst: path)` returns `null`. Renames a file or directory, copying and removing when `src` and `dst` are on different file systems.

### remove

`remove(p: path)` returns a `bool`: `true` if it removed something, `false` if nothing existed. Removes a file or a whole directory tree. A symlink is removed itself, not what it points to. It refuses, before deleting anything, to remove the root of the file system, your home directory, the project root, the working directory and any directory that contains the working directory, such as `remove(".")` or `remove("..")` (an `io` error).

### mkdir

`mkdir(p: path)` returns `p`. Creates the directory and any missing parents; it is not an error if it already exists.

### exists

`exists(p: path)` returns `true` if a file or directory exists at `p`.

### is_file

`is_file(p: path)` returns `true` if `p` is an existing file.

### is_dir

`is_dir(p: path)` returns `true` if `p` is an existing directory.

### glob

`glob(pattern: string, --root: path = cwd, --hidden: bool = false, --exclude: list[string] = [])` returns a sorted list of paths. `*` matches within one path component, `?` one character, `[a-z]` a character class, `**` any number of directories, and `{a,b}` either alternative. Patterns use `/` on every OS and are case-sensitive everywhere. Names starting with `.` are skipped unless the pattern spells out the dot (`.*`) or `hidden: true` is given. The results are joined to `root`; directories that match an `exclude` pattern are not descended into, so `exclude: ["node_modules"]` keeps `**` fast. A pattern that matches nothing returns `[]`; an invalid pattern is a `value` error.

### list_dir

`list_dir(p: path = cwd)` returns the entries of a directory as paths joined to `p`, sorted by name. Unlike `glob`, it includes names that start with `.`. A missing directory is an `io` error.

### stat

`stat(p: path)` returns a record `{size, modified, created, is_file, is_dir, is_symlink, mode}`. `modified` is a time, `created` a time or `null` where the file system does not record it, and `mode` the Unix permission bits, or `null` on Windows. A missing file is an `io` error.

### touch

`touch(p: path)` returns `null`. Creates an empty file, or updates the modification time of an existing one.

### tempdir

`tempdir()` returns a `path` to a new, empty directory. It is removed when the script exits, also when it ends with an error.

### tempfile

`tempfile(--ext: string = "")` returns a `path` to a new, empty file, removed when the script exits. `ext` gives it an extension, so that `write` and `read` pick the right format: `tempfile(ext: "json")`.

### chmod

`chmod(p: path, mode: int)` returns `null`. Sets the Unix permission bits, written in octal: `chmod("deploy.sh", 0o755)`. On Windows there are no permission bits, and `chmod` does nothing and succeeds.

## Paths

A path is its own type, printed with `/` on every OS. Join paths with `/`, as in `path("dist") / "app.zip"`. Besides the functions below, a path has the properties `parent`, `name`, `stem` and `ext`. The path functions also accept a string where they need a path.

```ok file=paths.ok
let p = path("build/out/../app.tar.gz")
print(p, p.parent, p.name, p.stem, p.ext)
print(p.with_ext("zip"), p.parts(), p.is_abs())
print(path("src/lib/a.ok").relative_to("src"), path("x").abs().is_abs())
print(path("dist") / "app" / "main.js")
```

```output
build/app.tar.gz build app.tar.gz app.tar gz
build/app.tar.zip ["build", "app.tar.gz"] false
lib/a.ok true
dist/app/main.js
```

### path

`path(s: string)` returns a `path`. The path is normalised lexically: `.` parts are dropped and `a/..` is removed. A path is returned as it is; anything else is a `type` error.

### abs

`p.abs()` returns `p` joined to the working directory and normalised, so it is absolute. A path that is already absolute is only normalised. `abs` works on paths only: it is not the absolute value of a number.

### relative_to

`p.relative_to(base: path)` returns the part of `p` below `base`, compared lexically (the files do not need to exist). A `p` that is not inside `base` is a `value` error.

### with_ext

`p.with_ext(ext: string)` returns `p` with its last extension replaced by `ext` (given without a dot). An empty `ext` removes the extension.

### parts

`p.parts()` returns a list of the components of `p` as strings.

### is_abs

`p.is_abs()` returns `true` if `p` is an absolute path.

## Data formats

`read` and `write` pick a format from the file name: `.json`, `.jsonl`/`.ndjson`, `.csv`, `.tsv`, `.toml`, `.yaml`/`.yml`, `.env`, and `text` for anything else. A second argument names the format instead: one of `json`, `jsonl`, `csv`, `tsv`, `lines`, `text`, `toml`, `yaml`, `env` or `bytes`, and for `parse` also `yaml_all`. An unknown format is an `arg` error. A file name ending in `.gz` is decompressed on read and compressed on write. The [data](data.md) page explains each format; the fine print is in [§13 of the spec](../SPEC.md#13-data-formats).

```ok file=data.ok
let rows = [{name: "ana", age: 34}, {name: "bo", age: 27}]
write(rows, "people.csv")
print(read("people.csv"))
write(rows, "people.json")
print(read("people.json") | map(.age) | sum)
print(to(rows, "json", compact: true).trim_end())
print(parse("a;b\n1;2\n", "csv", delimiter: ";"))
print(parse("port: 8080\ntags: [web, api]\n", "yaml"))
write("first\n", "log.txt")
write("second\n", "log.txt", append: true)
print(read("log.txt", "lines"))
```

```output
[{name: "ana", age: "34"}, {name: "bo", age: "27"}]
61
[{"name":"ana","age":34},{"name":"bo","age":27}]
[{a: "1", b: "2"}]
{port: 8080, tags: ["web", "api"]}
["first", "second"]
```

### read

`read(p: path, format: string? = null, --delimiter: string = ",", --header: bool = true, --gzip: bool? = null)` returns the parsed contents of a file. JSON, YAML and TOML give values; CSV and TSV give a lazy list of records of strings (lists of strings with `header: false`); `jsonl` and `lines` give lazy lists and `text` a lazy string. `delimiter` and `header` apply to CSV and TSV. `gzip` overrides the `.gz` rule in either direction. A missing file is an `io` error; malformed content is a `format` error with the line number; a text file that is not valid UTF-8 is an error that suggests `read(p, "bytes")`.

### write

`write(value, p: path, format: string? = null, --append: bool = false, --compact: bool = false, --delimiter: string = ",", --header: bool = true, --gzip: bool? = null)` returns `null`. Writes `value` to a file in the chosen format, creating parent directories, with `\n` line endings. `append: true` adds to the end of the file (for CSV, the header is only written to an empty file). `compact: true` writes JSON on one line without spaces. For CSV, the header is the keys of the first record; with `header: false` the rows are lists. A value the format cannot hold, such as a string written to `.csv`, is a `type` error. A failed write leaves an existing file as it was: the new contents go to a temporary file next to it, which replaces the file only when it is complete, so `read(p) | ... | write(p)` can also rewrite a file in place. The value comes first: `write("out.txt", "x")` is refused with a hint, because the arguments look swapped.

### parse

`parse(text: string, format: string, --delimiter: string = ",", --header: bool = true)` returns the value that `text` holds in `format`. The format `yaml_all` returns the list of documents of a YAML text with several documents; plain `yaml` refuses such text with a `format` error that suggests it.

### to

`to(value, format: string, --compact: bool = false, --delimiter: string = ",", --header: bool = true)` returns `value` serialised as a string in `format` (bytes for `bytes`). It is `write` without the file.

### stdin

`stdin(format: string? = null, --delimiter: string = ",", --header: bool = true)` returns ok's own standard input, read lazily and parsed as `format` (default `text`). Use it to write scripts that sit in a pipeline of other programs. `stdin`, `parse` and `to` never decompress.

```ok
let rows = stdin("csv")
print(rows | count)
```

## Commands

You start programs with `run`, which is part of the language rather than a function: see [commands](commands.md).

### which

`which(cmd: string)` returns a `path?`: where the program `cmd` would be found, by the same search `run` uses (on Windows this includes the `PATH` extensions such as `.exe`), or `null` if it is not installed.

```ok
if which("docker") == null {
  fail("this script needs docker; install it first")
}
```

## Dates and durations

A `time` is a point in time and a `duration` a length of time, written as literals like `90s`, `1h30m` or `250ms`. A time has the fields `year`, `month`, `day`, `hour`, `minute`, `second` and `unix`. Times print in the local time zone; pass `utc: true` to `format` for output that is the same everywhere.

```ok file=dates.ok
let t = time("2026-11-24T10:30:15Z")
print(t.format("%Y-%m-%d %H:%M", utc: true), t.format("%a %d %b", utc: true), t.unix)
print((t + 36h).format("%Y-%m-%d %H:%M", utc: true))
print(time("2026-11-25T00:00:00Z") - t, duration("1h30m") + 15m)
print(type(now()), now() > t)
sleep(10ms)
```

```output
2026-11-24 10:30 Tue 24 Nov 1795516215
2026-11-25 22:30
13h29m45s 1h45m
time true
```

### now

`now()` returns the current `time`.

### time

`time(s: string)` returns a `time` parsed from RFC 3339 (`"2026-11-24T10:30:00Z"`, `"2026-11-24T10:30:00+02:00"`) or from a date `YYYY-MM-DD`, which means midnight in the local time zone. Other text is a `value` error.

### format

`t.format(pattern: string, --utc: bool = false)` returns a `string`: the time `t` formatted with a strftime pattern, such as `%Y-%m-%d %H:%M:%S`. Common codes are `%Y %m %d %H %M %S %z %:z %a %b %j` and `%%`. Without `utc: true` the time is shown in the local time zone.

### sleep

`sleep(d: duration)` returns `null` after waiting for `d`. A plain number is a `type` error: write `sleep(2s)`, not `sleep(2)`.

### duration

`duration(s: string)` returns a `duration` parsed from the literal syntax, such as `"1h30m"` or `"250ms"`. Useful for values from the environment or a config file. Other text is a `value` error.

## Environment and system

These are values rather than functions; they are listed here because scripts use them in the same way. `env` and `os` are records you read fields from; `env` also has methods.

| Name | Description |
|---|---|
| `env.NAME` | The environment variable `NAME` as a string. A `key` error if it is not set, with a hint to use `env.get`. |
| `env.get(name: string, default = null)` | The variable, or `default` if it is not set. |
| `env.has(name: string)` | `true` if the variable is set. |
| `env.set(name: string, value: string)` | Sets a variable for `env.NAME` and every command started afterwards. |
| `env.remove(name: string)` | Removes a variable, also for commands started afterwards. |
| `env.all()` | A record of all variables. |
| `os.name` | `"linux"`, `"macos"`, `"windows"`, or another OS name in lower case. |
| `os.arch` | The processor architecture, such as `"x86_64"` or `"aarch64"`. |
| `os.exe` | `".exe"` on Windows, `""` elsewhere. |
| `args` | The script's arguments as a list of strings, when the script has no `main` function. |
| `cwd` | The working directory (the project root in project mode). |
| `invocation_dir` | The directory where you ran `ok`. |
| `script_dir` | The directory of the file that the running code is in. |
| `home` | Your home directory. |

On Windows, environment variable names are case-insensitive; elsewhere they are case-sensitive. To read a variable named like one of the methods, use `env.get("get")`.

```ok file=env.ok
env.set("GREETING", "hello")
print(env.GREETING, env.get("OK_SURELY_UNSET", "fallback"), env.has("GREETING"))
env.remove("GREETING")
print(env.get("GREETING"), os.exe == "" or os.exe == ".exe")
```

```output
hello fallback true
null true
```

## Hashing

```ok file=hash.ok
write("hello\n", "a.txt")
print(hash(path("a.txt")))
print(hash(bytes("abc"), "md5"))
```

```output
5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03
900150983cd24fb0d6963f7d28e17f72
```

### hash

`hash(x: path | bytes, algo: string = "sha256")` returns a `string` of lowercase hex. `algo` is `sha256`, `sha512`, `sha1` or `md5`. A path is hashed as a file, streaming, so large files are fine. A string is a `type` error, because it is ambiguous: write `hash(path(s))` for a file or `hash(bytes(s))` for the text. An unknown algorithm is an `arg` error.

## Network

### fetch

`fetch(url: string, --method: string = "GET", --headers: record = {}, --body = null, --json = null, --timeout: duration = 30s, --to: path? = null, --check: bool = true, --bytes: bool = false)` returns a record `{status, ok, headers, body, url}`. It makes an HTTP or HTTPS request (TLS with the operating system's certificates), follows up to 10 redirects and honours `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY`.

- `method` is `GET`, `HEAD`, `POST`, `PUT`, `PATCH` or `DELETE`.
- `body` is a string or bytes; `json` serialises a value as the body and sets `content-type: application/json`. Passing both is an `arg` error.
- `to` streams the response body into a file, and `body` is then `null`.
- `bytes: true` returns the body as bytes; otherwise a body that is not UTF-8 is an error.
- With `check: true`, a status of 400 or more is a `net` error that shows the status and the start of the body. Pass `check: false` to get the response record instead, and look at `ok` and `status`.
- `headers` in the result has lowercase names, and `url` is the final URL after redirects.

A connection failure or timeout is a `net` error. A request that is not `GET` or `HEAD`, and any download with `to`, is skipped by `ok --dry-run`.

```ok
let url = "https://api.github.com/repos/xtr-dev/ok/releases/latest"
let release = fetch(url, headers: {accept: "application/json"})
print(parse(release.body, "json").tag_name)
fetch("https://example.com/tool.tar.gz", to: "downloads/tool.tar.gz")
let r = fetch("https://example.com/missing", check: false)
if not r.ok {
  eprint("status", r.status)
}
```

## Archives

Archive members are stored with `/` paths relative to the parent of each source, so zipping the directory `site` gives members `site/...`. Extraction refuses members with an absolute path or one that escapes the destination ("zip slip"), and does not create symlinks that point outside it. Unix permission bits are kept where the OS has them. A missing archive is an `io` error.

```ok file=archives.ok
write("hello\n", "site/index.html")
write("body \{}\n", "site/css/main.css")
zip("site", "site.zip")
unzip("site.zip", "unpacked")
print(glob("unpacked/**/*"))
tar(["site/index.html", "site/css"], "site.tar.gz")
untar("site.tar.gz", "t")
print(glob("t/**/*"))
```

```output
[path("unpacked/site"), path("unpacked/site/css"), path("unpacked/site/css/main.css"), path("unpacked/site/index.html")]
[path("t/css"), path("t/css/main.css"), path("t/index.html")]
```

### zip

`zip(src: path | list[path], dst: path)` returns `null`. Creates the zip archive `dst` from a file, a directory or a list of them.

### unzip

`unzip(archive: path, dst: path)` returns `null`. Extracts a zip archive into the directory `dst`.

### tar

`tar(src: path | list[path], dst: path)` returns `null`. Creates a tar archive, gzip-compressed when `dst` ends in `.gz` or `.tgz`.

### untar

`untar(archive: path, dst: path)` returns `null`. Extracts a tar archive into `dst`; gzip compression is detected from the content, not the name.
