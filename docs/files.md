# Files and paths

Work with paths that print the same on every OS, find files with glob, copy, move and remove them, use temporary files, and pack zip and tar archives.

File work is where shell scripts break most often across systems: `/` against `\`, `cp -r` semantics, `find` flags, `rm -rf` on the wrong variable. ok has a `path` type and a small set of file functions that behave the same on Linux, macOS and Windows. The fine print is in [section 12.1](../SPEC.md#121-files) and [section 12.2](../SPEC.md#122-paths) of the spec.

## Path values

`path("src/main.rs")` makes a path. Paths are stored normalised and always print with `/`, so a script's output is identical on every OS. When a path is handed to the operating system (a file function, a command argument) it is converted to the native form.

```ok file=paths.ok
print(path("a//b/./c/../d/"))
let p = path("reports/2026/summary.csv")
print(p.name, p.stem, p.ext)
print(p.parent, p.with_ext("json"))
print(p.parts())
print([p], "{p}")
```

```output
a/b/d
summary.csv summary csv
reports/2026 reports/2026/summary.json
["reports", "2026", "summary.csv"]
[path("reports/2026/summary.csv")] reports/2026/summary.csv
```

Normalisation is purely textual: repeated separators collapse, `.` disappears, and `..` cancels the component before it. Nothing on disk is consulted and symlinks are not resolved. On Windows, `\` is also accepted as a separator in input; on other systems it is an ordinary character.

The file functions accept strings too, so `read("config.toml")` is fine. A path and a string are different values, though: `path("docs") == "docs"` is `false`. Compare `str(p)` with a string, or compare two paths.

| Member | Gives | For `path("build/app.tar.gz")` |
|---|---|---|
| `p.name` | last component | `"app.tar.gz"` |
| `p.stem` | name without the last extension | `"app.tar"` |
| `p.ext` | last extension, without the dot | `"gz"` |
| `p.parent` | the directory part | `build` |
| `p.with_ext(e)` | the path with another extension | `p.with_ext("zip")` is `build/app.tar.zip` |
| `p.parts()` | the components, starting with the root `"/"` for an absolute path | `["build", "app.tar.gz"]` |
| `p.is_abs()` | whether it is absolute | `false` |
| `p.abs()` | joined to the working directory | depends on the machine |
| `p.relative_to(base)` | the part below `base` | `p.relative_to("build")` is `app.tar.gz` |

## Joining paths

`/` joins a path with a string or another path. Start from a path value; joining two strings is an error, because `"a" / "b"` could just as well be a typo for division.

```ok file=join.ok
let out = path("build")
print(out / "app" / "main.js")
print(out / "../dist/app.js")
print(out / path("/etc/app.conf"))
```

```output
build/app/main.js
dist/app.js
/etc/app.conf
```

When the right side is absolute it replaces the left side, as the last line shows. This is handy for options that accept either a relative or an absolute path. `p.relative_to(base)` goes the other way and is an error when `p` is not inside `base`; it is how you print short paths below a root:

```ok file=relative.ok
let root = path("site")
for f in [root / "index.html", root / "css/main.css"] {
  print(f.relative_to(root))
}
```

```output
index.html
css/main.css
```

## Where relative paths point

Relative paths are resolved against the working directory, `cwd`. In a script that is the directory you ran `ok` from; in a project task it is the project root, so `read("Cargo.toml")` in a task finds the project's file wherever you started it. Four values give you the directories you might need:

| Value | Is |
|---|---|
| `cwd` | ok's working directory (the project root in project mode) |
| `script_dir` | the directory of the file the running code is in |
| `invocation_dir` | where the user ran `ok` |
| `home` | the user's home directory |

Use `script_dir / "templates"` for files that ship next to a script, and `cwd` or plain relative paths for the files the script works on. [Tasks](tasks.md) has more on project mode.

## Checking files

```ok file=check.ok
write("hello\n", "notes/today.txt")
print(exists("notes/today.txt"), exists("notes/tomorrow.txt"))
print(is_file("notes/today.txt"), is_dir("notes"))
let info = stat("notes/today.txt")
print(info.size, info.is_file, type(info.modified))
```

```output
true false
true true
6 true time
```

`stat` gives `size`, `modified`, `created` (or `null` where the file system does not record it), `is_file`, `is_dir`, `is_symlink` and `mode` (the Unix permission bits, `null` on Windows). `modified` is a `time`, so `now() - info.modified` is a duration you can compare with `7d`.

`hash(path("file"))` gives a file's SHA-256 as lowercase hex, read in a streaming way. It needs a `path` (or bytes), not a string, because a string could mean either the file name or the text itself.

## Finding files with glob

`glob(pattern)` returns the matching paths as a sorted list. Sorting is by the `/` form, so the order is the same on every OS.

| Pattern | Matches |
|---|---|
| `*` | any characters within one path component |
| `?` | one character |
| `[abc]`, `[a-z]` | one character from the class |
| `**` | zero or more whole components |
| `{a,b}` | either alternative |

```ok file=glob.ok
write("<h1>hi</h1>\n", "site/index.html")
write("body \{}\n", "site/css/main.css")
write({TOKEN: "x"}, "site/.env")
write("notes\n", "site/drafts/todo.md")

print(glob("site/*.html"))
print(glob('site/**/*.{html,css}'))
print(glob("**/*", root: "site", exclude: ["drafts"]))
print(glob("**/*", root: "site", hidden: true) | where is_file | map .name)
print(glob("*.pdf"))
```

```output
[path("site/index.html")]
[path("site/css/main.css"), path("site/index.html")]
[path("site/css"), path("site/css/main.css"), path("site/index.html")]
[".env", "main.css", "todo.md", "index.html"]
[]
```

Points to know:

- Names that start with `.` are skipped at every level, unless the pattern itself has a component starting with `.` (such as `**/.env*`) or you pass `hidden: true`.
- `**/*` matches directories as well as files. Add `| where is_file` when you only want files.
- `root:` sets the directory to search. The pattern is matched below it, and the results are joined to it, so they are ready to use as they are.
- `exclude:` patterns are matched the same way, and an excluded directory is not entered at all. Excluding `**/node_modules` or `**/target` keeps a search of a big tree fast.
- Matching is case-sensitive on every OS, even on file systems that are not.
- A pattern that matches nothing gives `[]`, not an error and not the pattern itself.
- Write patterns with `{a,b}` in single quotes. In a double-quoted string `{` starts an interpolation.

The `largest-files` example in the repository combines these: `glob("**/*", root: root, hidden: true, exclude: ["**/.git", "**/node_modules", "**/target"])`, then `where is_file`, `stat` and `sort`.

## Listing a directory

`list_dir(p)` gives the entries of one directory, joined to `p` and sorted by name. Unlike `glob` it includes hidden entries and does not descend.

```ok file=list.ok
write("a", "photos/b.jpg")
write("b", "photos/a.jpg")
mkdir("photos/raw")
print(list_dir("photos"))
print(list_dir("photos") | where is_file | map .name)
```

```output
[path("photos/a.jpg"), path("photos/b.jpg"), path("photos/raw")]
["a.jpg", "b.jpg"]
```

## Creating, copying, moving and removing

```ok file=manage.ok
write("v1\n", "src/app.txt")
write("readme\n", "src/docs/README.md")
print(mkdir("dist/assets"))

copy("src/app.txt", "dist/app.txt")
copy("src", "backup/src")
print(glob("backup/**/*") | where is_file)

move("dist/app.txt", "dist/assets/app.txt")
print(exists("dist/app.txt"), exists("dist/assets/app.txt"))

print(remove("backup"), remove("backup"))
touch("dist/.keep")
print(list_dir("dist") | map .name)
```

```output
dist/assets
[path("backup/src/app.txt"), path("backup/src/docs/README.md")]
false true
true false
[".keep", "assets"]
```

- `write` and `mkdir` create missing parent directories. `mkdir` returns the path and is not an error when the directory exists.
- `copy(src, dst)` copies a file or a whole directory tree, overwriting files. `dst` is always the exact destination, never a directory to copy into: `copy("a.txt", "dist")` does not create `dist/a.txt`; write `copy("a.txt", "dist/a.txt")`. This avoids the `cp` ambiguity where the result depends on whether `dist` already exists: copying a file onto an existing directory is an error, and copying a directory onto an existing directory merges the two, replacing files with the same name and keeping the rest.
- `move(src, dst)` renames, and falls back to copying and removing when the destination is on another file system.
- `remove(p)` removes a file or a directory tree and returns `false` when there was nothing to remove, so cleaning up twice is not an error. It refuses the file-system root, your home directory, the project root, and the working directory or any directory that contains it, so `remove(".")` and `remove("..")` are errors that delete nothing.
- `touch(p)` creates an empty file or updates its modification time; `chmod(p, 0o755)` sets Unix permission bits and does nothing on Windows.

### Previewing with --dry-run and --trace

All the functions that change files (`write`, `copy`, `move`, `remove`, `mkdir`, `touch`, `chmod` and the archive functions) are printed by `--trace` and skipped by `--dry-run`. A dry run shows what a script would change without changing anything:

```text file=build/old.txt
left over from the last build
```

```ok file=clean.ok
remove("build")
write("done\n", "build/stamp.txt")
print(exists("build/stamp.txt"))
```

```output run="ok --dry-run clean.ok"
~ remove build
~ write build/stamp.txt
false
```

The script keeps running, so its reads see the files as they were. A `remove` of something that does not exist changes nothing and is not printed. `ok --trace clean.ok` runs everything and prints each change with a `+`.

## Temporary files

`tempdir()` creates a new directory and `tempfile()` a new empty file. Both are removed when the script exits, also when it fails, so you do not need to clean up yourself.

```ok file=temp.ok
let work = tempdir()
write("draft", work / "notes.txt")
print(list_dir(work) | map .name)

let t = tempfile(ext: "json")
write({ok: true}, t)
print(t.ext, read(t))
```

```output
["notes.txt"]
json {ok: true}
```

Their location depends on the system, so print paths below them with `relative_to` rather than printing them whole.

## Archives

`zip(src, dst)` and `tar(src, dst)` pack a file, a directory or a list of paths; `unzip(archive, dst)` and `untar(archive, dst)` unpack into a directory. `tar` compresses with gzip when `dst` ends in `.gz` or `.tgz`, and `untar` detects compression by the content.

Members are stored with `/` paths relative to the source's parent, so packing the directory `report` gives members `report/...`, and packing `report/summary.txt` on its own gives `summary.txt`:

```ok file=archive.ok
write("one\n", "report/summary.txt")
write("two\n", "report/data/notes.md")

zip("report", "out/report.zip")
tar("report", "out/report.tar.gz")
tar(["report/summary.txt"], "out/single.tar")

unzip("out/report.zip", "from-zip")
untar("out/report.tar.gz", "from-tar")
untar("out/single.tar", "single")
print(glob("from-zip/**/*") | where is_file)
print(glob("from-tar/**/*") | where is_file)
print(list_dir("single"), read("single/summary.txt"))
```

```output
[path("from-zip/report/data/notes.md"), path("from-zip/report/summary.txt")]
[path("from-tar/report/data/notes.md"), path("from-tar/report/summary.txt")]
[path("single/summary.txt")] one
```

Extraction refuses members with an absolute path or one that escapes the destination (the "zip slip" attack), never writes through a symlink already in the destination, and does not create symlinks that point outside it, so unpacking a downloaded archive cannot write elsewhere on your disk. Member names must also be portable: a name with `:` or `\`, one ending in a dot or space, or a Windows device name such as `NUL` or `con.txt` is refused on every OS, so an archive unpacks the same everywhere. Extracted files get your usual permissions (the umask applies); an executable member stays executable, but setuid and similar bits are dropped.

An archive whose files would hold more than 4 GiB, or that has more than a million members, is refused partway through; pass `max_size:` (in bytes) or `max_entries:` to change a limit, or `null` to lift it:

```ok
unzip("out/report.zip", "big", max_size: 20_000_000_000)
```

For gzip-compressed data files, such as `events.jsonl.gz`, you do not need archives at all: see [Compressed files](data.md#compressed-files).

## Portability rules

ok keeps file work identical across operating systems:

- Paths print with `/`, and `glob` and `list_dir` sort the same way everywhere.
- `write` uses `\n` line endings on every OS, and `read` accepts both `\n` and `\r\n`.
- File names are compared case-sensitively, also on macOS and Windows, whose file systems usually are not. A glob for `*.TXT` does not find `notes.txt` on any OS.
- Where an OS lacks a feature, the behaviour is defined: `chmod` succeeds and does nothing on Windows, and `stat().mode` is `null` there.
- Nothing depends on a Unix shell or on tools such as `cp`, `find` or `tar` being installed.

If you need a native path for a program that is not ok, `str(p)` gives the `/` form, and passing the path value itself to `run` converts it to the native form. See [Running commands](commands.md) and [section 19](../SPEC.md#19-portability-rules) of the spec.
