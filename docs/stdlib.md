# Standard library

Every builtin function of ok, grouped by area, with its signature, what it returns and the errors it raises.

The functions on this page are available in every script without an import. Each function can be called in three ways:

- as a function: `trim(s)`;
- in method form, where the first argument comes before the dot: `s.trim()`;
- as a pipeline stage, where the value on the left becomes the first argument: `xs | sort .name` is `sort(xs, .name)` (see [pipelines](pipelines.md)).

Each entry starts with the signature, then says what the function returns and does, and ends with its errors. Most sections start with one sample that uses the functions of that section.

The signatures use ok's parameter syntax. `name: type` is a parameter and its type, `= v` gives a default, a `?` after a type means that the value may be `null`, and `--name` is a named argument, which is passed as `name: value`.

Errors are given by kind, such as a `type` error or an `io` error. The kind is the `kind` field of the error record that `try` and `?` produce (see [errors](errors.md)).

The normative definitions are in [section 12 of the spec](../SPEC.md#12-the-standard-library).

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

`print(...xs)`

Returns `null`. Writes the display form of each argument to stdout, separated by spaces and followed by a newline. `print()` writes an empty line. If stdout has been closed, because the output is piped into a program that stopped reading, the script ends quietly with exit code 0.

### eprint

`eprint(...xs)`

Returns `null`. Writes like `print`, but to stderr. Progress messages and warnings written with `eprint` stay out of captured output.

### fail

`fail(msg)`

Never returns. Raises a `fail` error with the display form of `msg` as its message. When `msg` is an error record caught with `try`, `fail` raises that error again with its original kind and location.

### exit

`exit(code: int = 0)`

Never returns. Ends the script with exit code `code`, after removing temporary files and directories. A code outside 0 to 255 is an `arg` error.

### prompt

`prompt(msg: string, --default: string? = null)`

Returns a `string`: the line that the user types on the terminal after the question `msg`. Without a terminal it returns `default`. Without a terminal and without a default, it is a `value` error.

### confirm

`confirm(msg: string, --default: bool = false)`

Returns a `bool`: the answer to the yes/no question `msg`, asked on the terminal. `default` is the answer when the user presses Enter without typing. With `ok --yes` it returns `true` without asking. Without a terminal and without `--yes` it is a `value` error, even when a default is given.

This sample is not run here, because it needs a terminal:

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

`str(x)`

Returns a `string`: the display form of `x`, which is the text that `print` writes. A string is returned as it is, and `null` becomes `"null"`.

### int

`int(x)`

Returns an `int` converted from a string, a float or a bool. Whitespace around the number in a string is ignored. A float is truncated toward zero. A bool becomes `1` or `0`. A malformed string and a float too large for an int are `value` errors. Any other type is a `type` error.

### float

`float(x)`

Returns a `float` converted from a string, an int or a bool. A malformed string is a `value` error. Any other type is a `type` error.

### type

`type(x)`

Returns a `string`: one of `null`, `bool`, `int`, `float`, `string`, `bytes`, `list`, `record`, `path`, `duration`, `time` or `fn`.

### len

`len(x)`

Returns an `int`: the number of characters of a string, bytes of a bytes value, elements of a list or fields of a record. The characters of a string are Unicode scalar values, not bytes. Any other type is a `type` error.

### bytes

`bytes(s: string)`

Returns `bytes`: the UTF-8 encoding of `s`. A bytes value is returned as it is.

### text

`text(b: bytes)`

Returns a `string`: `b` decoded as UTF-8. Invalid UTF-8 is a `value` error that says at which byte the decoding fails.

### fmt

`fmt(x, spec: string)`

Returns a `string`: `x` formatted by the format spec `spec`. The same spec is written after the colon in `"{x:spec}"`. A spec has the form `[[fill]align][0][width][,][.precision][type]`, where align is `<`, `>` or `^` and type is one of `d f e x X o b %`. Strings align left by default and numbers right. The width is at most 10000 and the precision at most 100. An invalid spec is a `format` error. The full definition is in [section 12.6 of the spec](../SPEC.md#126-format-specs).

### floor

`floor(x)`

Returns an `int`: the number `x` rounded down. An int is returned as it is. A result too large for an int is a `value` error. A value that is not a number is a `type` error.

### ceil

`ceil(x)`

Returns an `int`: the number `x` rounded up. An int is returned as it is. The errors are those of `floor`.

### round

`round(x)`

Returns an `int`: the number `x` rounded to the nearest whole number. Halves are rounded away from zero, so `round(2.5)` is `3` and `round(-2.5)` is `-3`. An int is returned as it is. The errors are those of `floor`.

### range

`range(n)` or `range(start, end, step = 1)`

Returns a lazy list of ints from `start` up to but not including `end`. `range(n)` starts at `0` and ends before `n`. A negative `step` counts down. The list is lazy, so `range(1_000_000_000) | take 3` produces only three numbers. A step of `0` is an `arg` error.

## Lists

The list functions take the list as their first argument, so the list can be the input of a pipeline stage. A `pred` or `key` argument is a function, usually an implicit closure such as `.age > 30` (see [functions](functions.md)).

An entry that says "Streams" describes a function that handles lazy input, such as `read("big.log") | lines`, one element at a time, without reading all of it into memory.

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

`where(xs: list, pred: fn)`

Returns a list of the elements for which `pred` returns `true`. Streams. A `pred` that returns anything but a bool is a `type` error: for a number field, `.count` is an error and `.count > 0` is a bool.

### map

`map(xs: list, f: fn)`

Returns a list of `f(x)` for each element `x`. Streams.

### each

`each(xs: list, f: fn)`

Returns `null`. Calls `f` on each element for its effect, such as printing. Lazy input is consumed one element at a time.

### sort

`sort(xs: list, key: fn? = null, --desc: bool = false)`

Returns the list sorted from smallest to largest, or from largest to smallest with `desc: true`. Without `key` the elements themselves are compared, and with `key` the values of `key(x)`. The sort is stable. Values that cannot be compared, such as a string and an int, are a `type` error.

### reverse

`reverse(xs: list)`

Returns the list in reverse order.

### uniq

`uniq(xs: list, key: fn? = null)`

Returns the list with only the first element of each distinct value, or of each distinct `key(x)`. Streams. The input does not need to be sorted.

### group

`group(xs: list, key: fn)`

Returns a record. Its keys are `str(key(x))`, in order of first appearance, and each value is the list of the elements with that key.

### flat

`flat(xs: list)`

Returns the list with nested lists flattened by one level: `flat([[1, [2]], 3])` is `[1, [2], 3]`. Streams.

### enumerate

`enumerate(xs: list)`

Returns a list of `{index, value}` records, where `index` counts from 0. Streams.

### take

`take(xs: list, n: int)`

Returns the first `n` elements, or the whole list if it is shorter. Streams, and stops reading the input after `n` elements. A negative `n` is an `arg` error.

### skip

`skip(xs: list, n: int)`

Returns the list without its first `n` elements. Streams. A negative `n` is an `arg` error.

### count

`count(xs: list, pred: fn? = null)`

Returns an `int`: the number of elements, or with `pred` the number of elements for which `pred` returns `true`.

### sum

`sum(xs: list, key: fn? = null)`

Returns a number: the sum of the elements, or with `key` the sum of `key(x)`. An empty list sums to `0`. Durations sum to a duration. Strings are a `type` error. CSV values are strings and need a conversion first, as in `sum(rows, r => int(r.amount))`.

### min

`min(xs: list, key: fn? = null)`

Returns the smallest element, or `null` for an empty list. With `key` it returns the element with the smallest `key(x)`.

### max

`max(xs: list, key: fn? = null)`

Returns the largest element, or `null` for an empty list. With `key` it returns the element with the largest `key(x)`.

### first

`first(xs: list, pred: fn? = null)`

Returns the first element, or with `pred` the first element for which `pred` returns `true`. Returns `null` if there is no such element. It stops reading the input when it has found the element.

### last

`last(xs: list, pred: fn? = null)`

Returns the last element, or with `pred` the last element for which `pred` returns `true`. Returns `null` if there is no such element.

### any

`any(xs: list, pred: fn)`

Returns a `bool`: `true` if `pred` returns `true` for at least one element, and `false` for an empty list. It stops reading the input at the first match.

### all

`all(xs: list, pred: fn)`

Returns a `bool`: `true` if `pred` returns `true` for every element, and `true` for an empty list. It stops reading the input at the first element for which `pred` returns `false`.

### join

`join(xs: list, sep: string = "")`

Returns a `string`: the elements converted with `str` and joined by `sep`. A list, record or bytes value among the elements is a `type` error.

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

`keys(r: record)`

Returns a list of the field names, in the order of the fields.

### values

`values(r: record)`

Returns a list of the field values, in the order of the fields.

### entries

`entries(r: record)`

Returns a list of `{key, value}` records, one for each field.

### from_entries

`from_entries(xs: list)`

Returns a record built from a list of `{key, value}` records. It is the inverse of `entries`. An element that is not such a record is a `type` error.

### get

`get(r: record, key: string, default = null)`

Returns the field `key` of `r`, or `default` if `r` has no such field. Unlike `r.name`, it takes a field name that is only known when the script runs, and a missing field is not an error. An `r` that is not a record is a `type` error.

### has

`has(r: record, key: string)`

Returns a `bool`: `true` if `r` has a field named `key`. An `r` that is not a record is a `type` error.

## Strings

The string functions work on Unicode text. A value that is not a string, where a function needs a string, is a `type` error.

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

`trim(s: string)`

Returns `s` without the whitespace at its start and its end.

### trim_start

`trim_start(s: string)`

Returns `s` without the whitespace at its start.

### trim_end

`trim_end(s: string)`

Returns `s` without the whitespace at its end.

### split

`split(s: string, sep: string? = null, --limit: int? = null)`

Returns a list of strings: the parts of `s`. Without `sep` it splits on runs of whitespace and drops empty parts. With `sep` it splits at every occurrence, so `"a,,b".split(",")` is `["a", "", "b"]`. `limit` is the largest number of parts, and the last part then holds the rest of `s`. An empty `sep` is an `arg` error. [`chars`](#chars) splits a string into its characters.

### lines

`lines(x)`

Returns a list of strings: `x` split on `\n` and `\r\n`, without a final empty line. For a list of strings it splits each string and flattens the result. Streams, so `read("big.log") | lines | count` reads the file in bounded memory.

### chars

`chars(s: string)`

Returns a list of the characters of `s`, each as a string of one character.

### replace

`replace(s: string, old: string, new: string)`

Returns `s` with every occurrence of `old` replaced by `new`. `old` is matched literally. [`sub`](#sub) replaces the matches of a regular expression.

### contains

`contains(x, item)`

Returns a `bool`: `true` if the string `x` contains the substring `item`, the list `x` contains the element `item`, or the record `x` has the key `item`. The operator `item in x` gives the same result. An `item` that is not a string, when `x` is a string, is a `type` error.

### starts_with

`starts_with(s: string, prefix: string)`

Returns a `bool`: `true` if `s` starts with `prefix`.

### ends_with

`ends_with(s: string, suffix: string)`

Returns a `bool`: `true` if `s` ends with `suffix`.

### upper

`upper(s: string)`

Returns `s` in upper case.

### lower

`lower(s: string)`

Returns `s` in lower case.

### repeat

`repeat(s: string, n: int)`

Returns `s` repeated `n` times. A negative `n` and a result larger than 1 GiB are `arg` errors.

### pad

`pad(x, width: int, --fill: string = " ", --align: string = "left")`

Returns a `string`: `str(x)` padded with `fill` to `width` characters. `align` is `"left"`, `"right"` or `"center"` and says where the text goes. Text that is already `width` characters or longer is returned unchanged. Another value for `align` and a result larger than 1 GiB are `arg` errors.

## Regular expressions

Regular expressions use the syntax of the Rust `regex` crate, which is like RE2: matching takes linear time, and there are no backreferences. A pattern in a single-quoted string keeps its backslashes, so `'\d'` is the pattern `\d`. An invalid pattern is a `value` error.

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

`s.match(re: string)`

Returns a record for the first match of `re` in `s`, or `null` if there is none. The record is `{text, start, end, groups, named}`. `start` and `end` are positions in `s`. `groups` is the list of capture groups, with `null` for a group that did not take part in the match. `named` is a record of the named groups. `match` is a keyword, so this function exists in method form only.

### matches

`matches(s: string, re: string)`

Returns a `bool`: `true` if `re` matches anywhere in `s`. A pattern between `^` and `$` must match the whole string.

### find

`find(s: string, re: string)`

Returns a list of match records, one for each non-overlapping match of `re` in `s`. The records have the form that [`match`](#match) returns.

### sub

`sub(s: string, re: string, replacement)`

Returns `s` with every match of `re` replaced. In a string `replacement`, `$1` and `${name}` stand for groups and `$$` for a literal `$`. A function `replacement` is called with each match record and returns the replacement text.

## Files

File functions accept a path or a string. A relative path is resolved against the working directory, which is the project root when a task runs. The guide page on [files](files.md) shows these functions in longer scripts.

The functions that change files are `write`, `copy`, `move`, `remove`, `mkdir`, `touch`, `chmod` and the [archive functions](#archives). `ok --trace` prints each of their calls, and `ok --dry-run` prints the calls without making the changes.

A file name that is not valid UTF-8 cannot be a string or a path. `list_dir` of a directory that holds such a name, and a `glob` that would return one, are `io` errors.

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

`copy(src: path, dst: path)`

Returns `null`. Copies a file, or a directory with everything in it. `dst` is the exact destination and never a directory to copy into: `copy("a.txt", "out")` creates a file named `out`. Parent directories are created and existing files are overwritten. Copying a directory onto an existing directory merges the two: files with the same name are replaced, and the other files in `dst` stay.

A symbolic link given as `src` is followed. Symbolic links inside a copied directory are copied as links, as `cp -R` does, so a link that points outside the directory does not bring outside files into the copy. `copy` never writes through a link in an existing destination: a link where a file goes is replaced, and a link where a directory goes is an `io` error.

The other `io` errors are a missing `src`, a file copied onto an existing directory, a directory copied onto an existing file, a path copied onto itself and a directory copied into itself.

### move

`move(src: path, dst: path)`

Returns `null`. Renames a file, a directory or a link. When `src` and `dst` are on different file systems, it copies `src` and then removes it, and links are recreated as links. Moving a directory into itself is an `io` error.

### remove

`remove(p: path)`

Returns a `bool`: `true` if it removed something and `false` if nothing existed at `p`. Removes a file or a whole directory tree. A symbolic link is removed itself, not the file it points to.

It is an `io` error, raised before anything is deleted, to remove any of these:

- the root of the file system;
- your home directory, or a directory that contains it, such as `/home`;
- the project root;
- the working directory, or a directory that contains it, as in `remove(".")` or `remove("..")`.

### mkdir

`mkdir(p: path)`

Returns `p`. Creates the directory and any missing parent directories. A directory that already exists is not an error.

### exists

`exists(p: path)`

Returns a `bool`: `true` if a file or directory exists at `p`.

### is_file

`is_file(p: path)`

Returns a `bool`: `true` if `p` is an existing file.

### is_dir

`is_dir(p: path)`

Returns a `bool`: `true` if `p` is an existing directory.

### glob

`glob(pattern: string, --root: path = cwd, --hidden: bool = false, --exclude: list[string] = [])`

Returns a sorted list of the paths that match `pattern`, or `[]` if nothing matches. In a pattern:

- `*` matches any characters within one path component;
- `?` matches one character;
- `[a-z]` matches one character of a class;
- `**` matches any number of directories;
- `{a,b}` matches either alternative. In a double-quoted string `{` starts an interpolation, so such a pattern is written in single quotes: `'*.{md,txt}'`.

Patterns use `/` on every OS and are case-sensitive on every OS. Names that start with `.` are skipped, unless a component of the pattern starts with a dot (`.*`) or `hidden: true` is given. The pattern is matched against paths relative to `root`, and the results are joined to `root`. A directory that matches an `exclude` pattern is not searched, so `exclude: ["node_modules"]` saves the time of reading that tree. An invalid pattern is a `value` error.

### list_dir

`list_dir(p: path = cwd)`

Returns the entries of the directory `p` as a list of paths joined to `p`, sorted by name. Unlike `glob`, it includes names that start with `.`. A missing directory is an `io` error.

### stat

`stat(p: path)`

Returns a record `{size, modified, created, is_file, is_dir, is_symlink, mode}`. `size` is in bytes. `modified` is a time. `created` is a time, or `null` where the file system does not record it. `mode` holds the Unix permission bits, or `null` on Windows. A missing file is an `io` error.

### touch

`touch(p: path)`

Returns `null`. Creates an empty file at `p`, or sets the modification time of an existing file to the current time.

### tempdir

`tempdir()`

Returns a `path` to a new, empty directory. The directory is removed when the script exits, also when the script ends with an error or is stopped with Ctrl-C. Its name cannot be predicted, and on Unix only its owner can open it (mode 0700), so other users of the machine cannot read or replace the files in it.

### tempfile

`tempfile(--ext: string = "")`

Returns a `path` to a new, empty file, which is removed when the script exits, like the directory of `tempdir()`. On Unix only its owner can read or write it (mode 0600). `ext` gives the file an extension, from which `write` and `read` choose the format: `tempfile(ext: "json")`.

### chmod

`chmod(p: path, mode: int)`

Returns `null`. Sets the Unix permission bits of `p`, usually written in octal: `chmod("deploy.sh", 0o755)`. Windows has no permission bits, so there `chmod` does nothing and succeeds.

## Paths

A path is a type of its own and prints with `/` on every OS. The `/` operator joins paths: `path("dist") / "app.zip"`. The path functions also accept a string where they need a path.

A path has four properties, read without parentheses:

| Property | Type | Value for `path("build/app.tar.gz")` |
|---|---|---|
| `p.parent` | `path` | `build`. The parent of a root is the root itself. |
| `p.name` | `string` | `"app.tar.gz"` |
| `p.stem` | `string` | `"app.tar"`: the name without its last extension |
| `p.ext` | `string` | `"gz"`: the last extension without the dot, or `""` if there is none |

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

`path(s: string)`

Returns a `path`. The path is normalised lexically: `.` parts are dropped, and a name followed by `..` is removed together with the `..`. A path is returned as it is. Any other type is a `type` error.

### abs

`p.abs()`

Returns a `path`: `p` joined to the working directory and normalised, which makes it absolute. A path that is already absolute is only normalised. `abs` takes a path and does not compute the absolute value of a number.

### relative_to

`p.relative_to(base: path)`

Returns a `path`: the part of `p` below `base`. The paths are compared lexically, so the files do not need to exist. A `p` that is not inside `base` is a `value` error.

### with_ext

`p.with_ext(ext: string)`

Returns a `path`: `p` with its last extension replaced by `ext`, which is given without a dot. An empty `ext` removes the extension.

### parts

`p.parts()`

Returns a list of the components of `p` as strings. The first part of an absolute path is its root, which is `"/"`, or `"C:/"` on Windows. Joining the parts in order with `/` gives the path again.

### is_abs

`p.is_abs()`

Returns a `bool`: `true` if `p` is an absolute path.

## Data formats

`read` and `write` choose a format from the file name: `.json`, `.jsonl` or `.ndjson`, `.csv`, `.tsv`, `.toml`, `.yaml` or `.yml`, and `.env`. Any other name means `text`. A `format` argument names the format instead of the file name. It is one of `json`, `jsonl`, `csv`, `tsv`, `lines`, `text`, `toml`, `yaml`, `env` or `bytes`, and for `parse` also `yaml_all`. An unknown format is an `arg` error.

A file whose name ends in `.gz` is decompressed when it is read and compressed when it is written.

The [data](data.md) page explains each format, and the exact rules are in [section 13 of the spec](../SPEC.md#13-data-formats).

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

`read(p: path, format: string? = null, --delimiter: string = ",", --header: bool = true, --gzip: bool? = null, --max_size: int? = 1_073_741_824, --max_line: int? = 67_108_864)`

Returns the parsed contents of the file `p`. What it returns depends on the format:

| Format | Result |
|---|---|
| `json`, `toml`, `yaml`, `env` | the value the document holds |
| `csv`, `tsv` | a lazy list of records of strings, or of lists of strings with `header: false` |
| `jsonl`, `lines` | a lazy list |
| `text` | a lazy string |
| `bytes` | a bytes value |

The named arguments are:

- `delimiter` and `header` apply to CSV and TSV.
- `gzip: true` decompresses a file whatever its name, and `gzip: false` reads a `.gz` file as it is.
- `max_size` is the largest size in bytes of a document that is read as a whole (JSON, TOML, YAML, .env and bytes). The default is 1 GiB.
- `max_line` is the largest size in bytes of one line or one CSV record. The default is 64 MiB.
- `null` for `max_size` or `max_line` removes that limit.

A missing file is an `io` error. Malformed content is a `format` error that gives the line number. A text file that is not valid UTF-8 is an error that suggests `read(p, "bytes")`. A document larger than `max_size` and a line longer than `max_line` are `io` errors.

### write

`write(value, p: path, format: string? = null, --append: bool = false, --compact: bool = false, --delimiter: string = ",", --header: bool = true, --gzip: bool? = null)`

Returns `null`. Writes `value` to the file `p` in the chosen format, with `\n` line endings. Parent directories are created. The named arguments are:

- `append: true` adds to the end of the file. For CSV, the header is then written only to an empty file.
- `compact: true` writes JSON on one line without spaces.
- `delimiter` and `header` apply to CSV and TSV. The header is the keys of the first record. With `header: false` the rows are lists.
- `gzip: true` compresses the file whatever its name, and `gzip: false` writes a `.gz` file without compression.

A failed write leaves an existing file as it was. The new contents go to a temporary file next to `p`, which replaces `p` only when it is complete. For the same reason `read(p) | ... | write(p)` can rewrite a file in place.

A value that the format cannot hold, such as a string written to a `.csv` file, is a `type` error. The value is the first argument: `write("out.txt", "x")` is refused with a hint, because the arguments look swapped.

### parse

`parse(text: string, format: string, --delimiter: string = ",", --header: bool = true)`

Returns the value that `text` holds in `format`, as `read` does for a file. The format `yaml_all` returns the list of the documents of a YAML text with several documents. The format `yaml` refuses such a text with a `format` error that suggests `yaml_all`. `parse` never decompresses.

### to

`to(value, format: string, --compact: bool = false, --delimiter: string = ",", --header: bool = true)`

Returns a `string`: `value` serialised in `format`, as `write` puts it in a file. For the format `bytes` it returns bytes. `to` never compresses.

### stdin

`stdin(format: string? = null, --delimiter: string = ",", --header: bool = true, --max_size: int? = 1_073_741_824, --max_line: int? = 67_108_864)`

Returns ok's own standard input, read lazily and parsed as `format`, which is `text` by default. The limits are those of `read`. `stdin` never decompresses. With `stdin`, a script can take the output of another program as its input:

```ok
let rows = stdin("csv")
print(rows | count)
```

## Commands

`run` starts programs. It is part of the language and not a function, and the [commands](commands.md) page describes it.

### which

`which(cmd: string)`

Returns a `path` to the program that `run` would start for the name `cmd`, or `null` if the program is not installed. The search is the one `run` makes, which on Windows includes the `PATH` extensions such as `.exe`.

This script stops with a message when `docker` is not installed:

```ok
if which("docker") == null {
  fail("this script needs docker; install it first")
}
```

## Dates and durations

A `time` is a point in time. A `duration` is a length of time, written as a literal such as `90s`, `1h30m` or `250ms`. A time has the fields `year`, `month`, `day`, `hour`, `minute`, `second` and `unix`. Times print in the local time zone. With `utc: true`, `format` gives the same text in every time zone.

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

`now()`

Returns a `time`: the current time.

### time

`time(s: string)`

Returns a `time` parsed from `s`. `s` is in RFC 3339 form, such as `"2026-11-24T10:30:00Z"` or `"2026-11-24T10:30:00+02:00"`, or a date `YYYY-MM-DD`, which means midnight in the local time zone. Other text is a `value` error.

### format

`t.format(pattern: string, --utc: bool = false)`

Returns a `string`: the time `t` formatted with a strftime pattern, such as `%Y-%m-%d %H:%M:%S`. Common codes are `%Y %m %d %H %M %S %z %:z %a %b %j` and `%%`. The time is shown in the local time zone, or in UTC with `utc: true`.

### sleep

`sleep(d: duration)`

Returns `null` after waiting for `d`. A plain number is a `type` error: `sleep(2s)` is valid and `sleep(2)` is not.

### duration

`duration(s: string)`

Returns a `duration` parsed from a string in the literal syntax, such as `"1h30m"` or `"250ms"`. It converts durations that come from the environment or a configuration file. Other text is a `value` error.

## Environment and system

The names in this section are global values, not functions. `env` and `os` are records, and `env` also has methods.

| Name | Type | Description |
|---|---|---|
| `env.NAME` | `string` | The environment variable `NAME`. A `key` error if the variable is not set, with a hint to use `env.get`. |
| `env.get(name: string, default = null)` | value | The variable `name`, or `default` if it is not set. |
| `env.has(name: string)` | `bool` | `true` if the variable is set. |
| `env.set(name: string, value: string)` | `null` | Sets a variable for `env.NAME` and for every command started afterwards. |
| `env.remove(name: string)` | `null` | Removes a variable, also for commands started afterwards. |
| `env.all()` | `record` | All variables. |
| `os.name` | `string` | `"linux"`, `"macos"`, `"windows"`, or another OS name in lower case. |
| `os.arch` | `string` | The processor architecture, such as `"x86_64"` or `"aarch64"`. |
| `os.exe` | `string` | `".exe"` on Windows, `""` elsewhere. |
| `args` | `list[string]` | The script's arguments, when the script has no `main` function. |
| `cwd` | `path` | The working directory, which is the project root when a task runs. |
| `invocation_dir` | `path` | The directory where you ran `ok`. |
| `script_dir` | `path` | The directory of the file that the running code is in. |
| `home` | `path` | Your home directory. |

Environment variable names are case-insensitive on Windows and case-sensitive elsewhere. A variable named like one of the methods is read with `env.get`, as in `env.get("get")`. For `env.set` and `env.remove`, a name that is empty or contains `=` is a `value` error.

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

`hash(x: path | bytes, algo: string = "sha256")`

Returns a `string`: the hash of `x` in lowercase hex. `algo` is `sha256`, `sha512`, `sha1` or `md5`. A path is hashed as a file, which is read in pieces, so the size of the file does not matter. A string is a `type` error, because it could mean a file name or the text itself: `hash(path(s))` hashes the file and `hash(bytes(s))` the text. An unknown algorithm is an `arg` error.

## Network

### fetch

`fetch(url: string, --method: string = "GET", --headers: record = {}, --body = null, --json = null, --timeout: duration = 30s, --to: path? = null, --check: bool = true, --bytes: bool = false, --max_size: int? = 1_073_741_824)`

Returns a record `{status, ok, headers, body, url}` for an HTTP or HTTPS request. TLS uses the certificates of the operating system. `fetch` follows up to 10 redirects and honours `HTTPS_PROXY`, `HTTP_PROXY` and `NO_PROXY`.

The named arguments are:

- `method` is `GET`, `HEAD`, `POST`, `PUT`, `PATCH` or `DELETE`.
- `headers` is a record of request headers.
- `body` is the request body, a string or bytes.
- `json` is a value that is serialised as the request body, with `content-type: application/json`. Passing both `body` and `json` is an `arg` error.
- `timeout` covers the whole call, redirects included.
- `to` streams the response body into a file. The request then asks for no content encoding, so the file is saved as the server stores it.
- `check: true` makes a status of 400 or more a `net` error that shows the status and the start of the body. With `check: false` such a response is returned like any other.
- `bytes: true` returns the body as bytes. Without it, a body that is not UTF-8 is an error.
- `max_size` limits the response body in bytes. The default is 1 GiB, and no limit for a download with `to` unless a limit is passed. `null` removes the limit. A larger body is a `net` error.

The fields of the result are:

- `status` is the status code, and `ok` is `true` for a status from 200 to 399.
- `headers` is a record with lowercase names. A header that the response sends several times has its values joined with `, `, except `set-cookie`, which is always a list.
- `body` is a string, bytes with `bytes: true`, or `null` with `to`.
- `url` is the final URL after redirects.

Redirects go only to `http://` and `https://` URLs and never from `https` to plain `http`. A redirect to another host, scheme or port drops the `headers` that were passed, so a token such as `authorization` or `x-api-key` is sent only to the place it was meant for.

A connection failure and a timeout are `net` errors. `ok --dry-run` skips a request that is not `GET` or `HEAD`, and any download with `to`.

This sample is not run here, because it needs the network:

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

Archive members are stored with `/` paths relative to the parent directory of each source, so an archive of the directory `site` has members named `site/...`. A missing archive is an `io` error.

Extraction writes only inside the destination. `unzip` and `untar` stop with an `io` error at:

- a member with an absolute path, or a path that leaves the destination with `..` ("zip slip");
- a member name that is not portable: one with `:`, `\` or a control character, one that ends in a dot or a space, or a Windows device name such as `NUL`;
- a link that points outside the destination.

They never write through a link that is already in the destination. Extracted files get the permissions of any new file, so the umask applies, and a member stored as executable stays executable.

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

`zip(src: path | list[path], dst: path)`

Returns `null`. Creates the zip archive `dst` from a file, a directory or a list of files and directories. A symbolic link is stored as what it points to: the content of a file, or a directory with its entries. A link to a directory that contains the link is an `io` error.

### unzip

`unzip(archive: path, dst: path, --max_size: int? = 4_294_967_296, --max_entries: int? = 1_000_000)`

Returns `null`. Extracts a zip archive into the directory `dst`. A symbolic link in the archive is an `io` error. `max_size` is the largest total size in bytes of the extracted files (4 GiB by default) and `max_entries` the largest number of members. Going over either limit is an `io` error that stops the extraction. `null` removes a limit.

### tar

`tar(src: path | list[path], dst: path)`

Returns `null`. Creates the tar archive `dst` from a file, a directory or a list of files and directories. The archive is gzip-compressed when `dst` ends in `.gz` or `.tgz`.

### untar

`untar(archive: path, dst: path, --max_size: int? = 4_294_967_296, --max_entries: int? = 1_000_000)`

Returns `null`. Extracts a tar archive into the directory `dst`. Gzip compression is detected from the content of the archive, not from its name. Symbolic links are created after every other member, and only when they point inside `dst`. A hard link becomes a copy of the file it names. Devices and pipes are skipped. `max_size` and `max_entries` work as for `unzip`.
