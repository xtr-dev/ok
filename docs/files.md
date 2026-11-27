# Files and paths

Work with paths that print the same on every OS, find files with glob, copy, move and remove them, use temporary files, and pack zip and tar archives.

ok has a `path` type and a small set of file functions that behave the same on Linux, macOS and Windows. They replace the parts of a shell script that differ between systems, such as `/` against `\`, the rules of `cp -r` and the flags of `find`. The exact rules are in [section 12.1](../SPEC.md#121-files) and [section 12.2](../SPEC.md#122-paths) of the spec.

## Making a path and reading its parts

`path("src/main.rs")` makes a path. A path is stored normalised and always prints with `/`, so the output of a script is identical on every OS. When ok hands a path to the operating system, in a file function or as a command argument, it converts the path to the native form.

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

Normalisation works on the text of the path only. Repeated separators collapse, `.` disappears, and `..` cancels the component before it. ok does not look at the disk and does not resolve symlinks. On Windows, `\` is also accepted as a separator in input. On other systems it is an ordinary character.

The file functions accept strings too, so `read("config.toml")` works. A path and a string are still different values, and `path("docs") == "docs"` is `false`. Compare `str(p)` with a string, or compare two paths.

A path has these members:

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

`/` joins a path with a string or with another path. The left side must be a path. `"a" / "b"` is an error, because it could be a mistyped division.

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

When the right side is absolute, it replaces the left side, as the last line shows. An option that accepts either a relative or an absolute path therefore needs no special case.

`p.relative_to(base)` removes `base` from the front of `p`, and is an error when `p` is not inside `base`. Use it to print short paths below a root:

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

A relative path is resolved against the working directory, `cwd`. In a script, the working directory is the directory you ran `ok` from. In a project task it is the project root, so `read("Cargo.toml")` in a task finds the project's file from any directory of the project. Four values hold the directories that a script can start from:

| Value | Is |
|---|---|
| `cwd` | ok's working directory (the project root in project mode) |
| `script_dir` | the directory of the file the running code is in |
| `invocation_dir` | where the user ran `ok` |
| `home` | the user's home directory |

Use `script_dir / "templates"` for files that ship next to a script, and `cwd` or plain relative paths for the files the script works on. [Tasks](tasks.md) has more on project mode.

## Checking whether a file exists and reading its details

`exists`, `is_file` and `is_dir` each return a bool, and `stat` returns a record of details:

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

The record that `stat` returns has these fields:

| Field | Holds |
|---|---|
| `size` | the size in bytes |
| `modified` | the time of the last change |
| `created` | the time of creation, or `null` where the file system does not record it |
| `is_file`, `is_dir`, `is_symlink` | what kind of entry it is |
| `mode` | the Unix permission bits, or `null` on Windows |

`modified` is a `time`, so `now() - info.modified` is a duration that you can compare with `7d`.

`hash(path("file"))` gives the SHA-256 of a file as lowercase hex, without loading the whole file into memory. `hash` needs a `path` or bytes. A string is an error, because it could mean either the file name or the text itself.

## Finding files with glob

`glob(pattern)` returns the matching paths as a sorted list. The list is sorted by the `/` form of the paths, so the order is the same on every OS. A pattern can contain:

| Pattern | Matches |
|---|---|
| `*` | any characters within one path component |
| `?` | one character |
| `[abc]`, `[a-z]` | one character from the class |
| `**` | zero or more whole components |
| `{a,b}` | either alternative |

This script writes four files below `site/` and searches them in five ways:

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

`glob` follows these rules:

- A name that starts with `.` is skipped at every level. To include such names, pass `hidden: true` or give the pattern a component that starts with `.`, such as `**/.env*`.
- `**/*` matches directories as well as files. Add `| where is_file` when you want only files.
- `root:` sets the directory to search. The pattern is matched below `root`, and the results are joined to it, so you can pass them to the file functions unchanged.
- `exclude:` patterns are matched the same way, and an excluded directory is not entered at all. Excluding `**/node_modules` or `**/target` saves the time of walking those trees.
- Matching is case-sensitive on every OS, also on file systems that are not.
- A pattern that matches nothing gives `[]`. It is not an error, and `glob` does not return the pattern itself as a shell does.
- Write a pattern with `{a,b}` in single quotes. In a double-quoted string, `{` starts an interpolation.

The `largest-files` example in the repository combines several of these rules. It calls `glob("**/*", root: root, hidden: true, exclude: ["**/.git", "**/node_modules", "**/target"])` and then uses `where is_file`, `stat` and `sort`.

## Listing a directory

`list_dir(p)` gives the entries of one directory, joined to `p` and sorted by name. Unlike `glob`, it includes hidden entries and does not go into subdirectories.

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

`mkdir`, `copy`, `move`, `remove` and `touch` change the file system. This script uses each of them:

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

`write` and `mkdir` create missing parent directories. `mkdir` returns the path and is not an error when the directory exists.

`copy(src, dst)` copies a file or a whole directory tree and overwrites existing files. `dst` is the exact destination, never a directory to copy into. `copy("a.txt", "dist")` does not create `dist/a.txt`. For that, write `copy("a.txt", "dist/a.txt")`. With `cp`, the result depends on whether `dist` already exists. With `copy`, a file copied onto an existing directory is an error. A directory copied onto an existing directory merges the two: files with the same name are replaced, and the other files stay.

`move(src, dst)` renames. When the destination is on another file system, it copies and then removes the source.

`remove(p)` removes a file or a directory tree. It returns `false` when there was nothing to remove, so cleaning up twice is not an error. `remove` refuses the file-system root, your home directory, the project root, and the working directory or any directory that contains it. `remove(".")` and `remove("..")` are therefore errors that delete nothing.

`touch(p)` creates an empty file, or updates the modification time of an existing one. `chmod(p, 0o755)` sets Unix permission bits and does nothing on Windows.

### Previewing changes with --dry-run and --trace

`ok --dry-run` shows what a script would change without changing anything. It prints every call of a function that changes files and skips the call. Those functions are `write`, `copy`, `move`, `remove`, `mkdir`, `touch`, `chmod` and the archive functions. In this example, `build/old.txt` exists before the script runs:

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

The script keeps running after a skipped call, so its reads see the files as they were. That is why the last line prints `false`. A call is printed also when it would have nothing to do, such as a `remove` of something that does not exist.

`ok --trace clean.ok` runs everything and prints each change with a `+` in place of the `~`.

## Temporary files

`tempdir()` creates a new directory and `tempfile()` a new empty file. ok removes both when the script exits, also when the script fails or you press Ctrl-C, so you do not need to clean up yourself. Other users of the machine cannot look inside them.

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

The location of a temporary directory or file depends on the system and is different in every run. When the output of a script must be repeatable, print a path below a temporary directory with `relative_to`.

## Packing and unpacking zip and tar archives

`zip(src, dst)` and `tar(src, dst)` pack a file, a directory or a list of paths. `unzip(archive, dst)` and `untar(archive, dst)` unpack an archive into a directory. `tar` compresses with gzip when `dst` ends in `.gz` or `.tgz`, and `untar` detects compression from the content.

Members are stored with `/` paths relative to the parent of the source. Packing the directory `report` gives members named `report/...`, and packing `report/summary.txt` on its own gives a member named `summary.txt`:

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

### What unpacking refuses

Unpacking an archive that you downloaded cannot write outside the destination directory. `unzip` and `untar` refuse a member with an absolute path, and a member whose path leaves the destination (the "zip slip" attack). They never write through a symlink that is already in the destination, and they do not create symlinks that point outside it.

Member names must also be portable, so that an archive unpacks the same on every OS. A name with `:` or `\`, a name that ends in a dot or a space, and a Windows device name such as `NUL` or `con.txt` are refused on every OS.

Extracted files get your usual permissions, because the umask applies. An executable member stays executable, but setuid and similar bits are dropped.

An archive whose files would hold more than 4 GiB, or that has more than a million members, is refused partway through unpacking. To change a limit, pass `max_size:` (in bytes) or `max_entries:`. Pass `null` for no limit:

```ok
unzip("out/report.zip", "big", max_size: 20_000_000_000)
```

`tar` stores a symbolic link as a link. `zip` stores what the link points to: the content of a file, or a directory with its entries. A link to a directory that contains the link is an error in `zip`.

A single data file compressed with gzip, such as `events.jsonl.gz`, is not an archive. `read` and `write` handle it directly, as [Compressed files](data.md#compressed-files) describes.

## What is the same on every OS

These rules make file work give the same result on Linux, macOS and Windows:

- Paths print with `/`, and `glob` and `list_dir` sort the same way everywhere.
- `write` uses `\n` line endings on every OS, and `read` accepts both `\n` and `\r\n`.
- File names are compared case-sensitively, also on macOS and Windows, whose file systems usually are not. A glob for `*.TXT` does not find `notes.txt` on any OS.
- File names are text. A name that is not valid UTF-8, which Linux allows, is an error in `list_dir`, and in `glob` when the pattern selects it. ok does not replace the invalid bytes, because the result would be the name of a different file.
- Where an OS lacks a feature, the behaviour is defined: `chmod` succeeds and does nothing on Windows, and `stat().mode` is `null` there.
- Nothing depends on a Unix shell or on tools such as `cp`, `find` or `tar` being installed.

To give a path to another program, pass the path value itself to `run`, which converts it to the native form. `str(p)` gives the `/` form. See [Running commands](commands.md) and [section 19](../SPEC.md#19-portability-rules) of the spec.
