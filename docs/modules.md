# Modules

Split code across files with `import`: what a library file may contain, what it exports, how import paths work, and the errors you can meet.

## A first library

When a script grows, or two scripts need the same helper, move the shared functions into their own file and `import` it. Here is a small library and a script that uses it:

```ok file=lib/release.ok
# Helpers for release scripts.
let prefix = "v"

fn tag_name(version: string) {
  "{prefix}{version}"
}

fn bump(version: string, part: string) {
  let [major, minor, patch] = version.split(".").map(int)
  match part {
    "major" => "{major + 1}.0.0"
    "minor" => "{major}.{minor + 1}.0"
    _ => "{major}.{minor}.{patch + 1}"
  }
}
```

```ok file=release.ok
import "./lib/release.ok"

let next = bump("1.4.2", "minor")
print(next, tag_name(next))
```

```output
1.5.0 v1.5.0
```

`import` without anything after the path brings every exported name of the file into scope, so `bump` and `tag_name` can be called directly.

## Importing under a name

With `as`, the names stay inside a namespace instead. You call them as `name.function(...)`:

```ok file=lib/release.ok
let prefix = "v"

fn tag_name(version: string) {
  "{prefix}{version}"
}
```

```ok file=tag.ok
import "./lib/release.ok" as release

print(release.tag_name("2.0.0"))
print(release.prefix)
```

```output
v2.0.0
v
```

A namespace is easier to read in a long script, because every call says where the function comes from, and it cannot clash with your own names. Use the plain form for a small set of helpers you use everywhere, and `as` for everything else.

A namespace is a record of the exported names. A misspelled name gets a suggestion, like a misspelled field:

```ok file=typo.ok
import "./lib/release.ok" as release

print(release.tagname("2.0.0"))
```

```output exit=1
error[name]: no function `tagname` to call on a record
 --> typo.ok:3:15
  |
3 | print(release.tagname("2.0.0"))
  |               ^^^^^^^
  |
  = help: did you mean `tag_name`?
```

## What a module exports

Every top-level `fn` and `let` of a library is exported, except names that start with `_`. Use a leading underscore for helpers that are not part of the library's interface:

```ok file=lib/names.ok
let _separator = "-"

fn _clean(word: string) {
  lower(trim(word))
}

fn slug(title: string) {
  title.split(" ").map(_clean).where(w => w != "") | join(_separator)
}
```

```ok file=slug.ok
import "./lib/names.ok" as names

print(names.slug("  Hello  Big World "))
print(names)
```

```output
hello-big-world
{slug: <fn slug>}
```

Functions in a library see the library's own names, private ones included, and the names it imports. They do not see the names of the script that imports them: a library is self-contained, so it behaves the same whoever imports it.

Names a library imports are not exported again. If `lib/math.ok` imports `lib/helpers.ok`, a script that imports `lib/math.ok` does not get the helpers; it imports `lib/helpers.ok` itself if it needs them.

## Libraries only declare

An imported file is a **library**, and a library may contain only `fn` declarations, constant `let` declarations and `import`. Loading a library never runs any code. This makes imports safe and predictable: the order of your imports cannot matter, and importing a file cannot print, write files or start programs behind your back.

A constant is built from literals, lists and records, operators, string interpolation, other constants, `path(...)` and `os.name`, `os.arch` and `os.exe`. Anything that does work belongs inside a function:

```ok file=lib/config.ok
let settings = read("settings.toml")
```

```ok file=app.ok
import "./lib/config.ok"
```

```output exit=1
error[syntax]: a library's top-level `let` must be a constant
 --> lib/config.ok:1:16
  |
1 | let settings = read("settings.toml")
  |                ^^^^^^^^^^^^^^^^^^^^^
  |
  = help: libraries never run code when imported; compute the value inside a function instead
```

The fix is a function the script calls when it needs the value:

```ok
fn settings() {
  read("settings.toml")
}
```

Statements such as `print(...)` or `run` at the top level of a library are errors for the same reason. A library may still declare `fn main`; it is only run when the file itself is run as a script.

## Import paths

The path after `import` is a string literal, relative to the directory of the file that contains the `import`, not to your working directory. That way a library that imports its neighbours keeps working wherever it is used from. `"./lib/release.ok"` and `"lib/release.ok"` mean the same thing; the `./` just makes it obvious that the path is relative. `..` goes up a directory, and paths use `/` on every OS.

Some paths are refused, because they would make a script depend on the machine it runs on or on the network:

- Absolute paths, such as `"/opt/lib.ok"`, are an error that suggests a relative path.
- URLs are an error: ok has no remote imports, package manager or search path.
- The path must be written out as a string. `import some_variable` is a syntax error, so every import can be found without running anything.

A file that does not exist is reported at the `import` line:

```ok file=missing.ok
import "./lib/nothing.ok"
```

```output exit=1
error[io]: cannot import "./lib/nothing.ok": no such file
 --> missing.ok:1:8
  |
1 | import "./lib/nothing.ok"
  |        ^^^^^^^^^^^^^^^^^^
```

## Clashes and cycles

A plain `import` puts names straight into your scope, so they must not collide with your own declarations or with another import. A collision is an error when the script loads, with the fix in the help line:

```ok file=lib/a.ok
fn greet() {
  "hello from a"
}
```

```ok file=lib/b.ok
fn greet() {
  "hello from b"
}
```

```ok file=both.ok
import "./lib/a.ok"
import "./lib/b.ok"

print(greet())
```

```output exit=1
error[name]: `greet` from lib/b.ok clashes with a name already declared here
 --> both.ok:2:8
  |
2 | import "./lib/b.ok"
  |        ^^^^^^^^^^^^
  |
  = help: import it under a name instead: import "./lib/b.ok" as lib
```

ok never lets one name silently win over another. Import one or both files with `as` and the names no longer meet.

Each file is loaded once per run, however many files import it. Two libraries that import each other, directly or through others, are an import cycle, and the error shows the whole cycle:

```ok file=lib/one.ok
import "./two.ok"

fn one() {
  1
}
```

```ok file=lib/two.ok
import "./one.ok"

fn two() {
  2
}
```

```ok file=cycle.ok
import "./lib/one.ok"
```

```output exit=1
error[name]: import cycle: lib/one.ok -> lib/two.ok -> lib/one.ok
 --> lib/two.ok:1:8
  |
1 | import "./one.ok"
  |        ^^^^^^^^^^
```

Move the shared part into a third file that both import.

## Modules in a project

Project files (`ok.ok` and the files in `ok/`) use the same `import`, with two additions:

- Every import must stay inside the project root, after following symlinks. A path that escapes with `..` or a symlink is an error, so the code you trust for a project is the code in that project. See [Trust](trust.md).
- Importing another project's `ok.ok` with `as`, such as `import "frontend/ok.ok" as fe`, mounts that project's tasks under a namespace. See [Tasks](tasks.md).

## Where to go next

- [Functions](functions.md) covers what goes inside a library: parameters, defaults and named arguments.
- [Tasks](tasks.md) shows how project files are organised with namespaces and imports.
- The spec has the fine print on [modules](../SPEC.md#11-modules) and [which declarations each kind of file allows](../SPEC.md#41-files).
