# Command-line scripts

Give a script a command line by declaring `fn main(...)`: parameters become arguments and flags, with types, defaults, short flags and `--help`.

## Arguments without main

A script without a `main` function gets its command-line arguments as the list `args`, exactly as they were given:

```ok file=echo-args.ok
print(args, len(args))
```

```output run="ok echo-args.ok one 'two words' --three"
["one", "two words", "--three"] 3
```

That is fine for a quick script. As soon as a script takes a flag or two, though, you end up parsing `args` by hand, checking types and writing a usage message. `main` does all of that for you.

## Declaring main

When a script has a `fn main`, ok reads its parameters and builds the command line from them. The arguments are checked against the signature before any of your code runs, and then `main` is called with them:

```ok file=greet.ok
# Greet someone, possibly loudly.
fn main(
  name: string,              # who to greet
  --times, -n: int = 1,      # how many times
  --shout: bool,             # use capital letters
) {
  let line = "Hello, {name}!"
  for _ in range(times) {
    print(if shout { upper(line) } else { line })
  }
}
```

```output run="ok greet.ok Ada"
Hello, Ada!
```

```output run="ok greet.ok Ada --times 2 --shout"
HELLO, ADA!
HELLO, ADA!
```

The signature is the whole interface. There is nothing to keep in sync: the names, types, defaults and help text in one place are what the user sees, and what your code receives.

Code at the top level of the script still runs first, before `main`. `main` does not need any parameters: a script with `fn main()` and no arguments simply runs it.

## Positional parameters

Parameters without `--` are positional, in the order you declare them:

- A parameter without a default is required. Leaving it out is an error.
- A parameter with a default is optional.
- A rest parameter, `...name: T`, takes all remaining positional arguments as a list, which may be empty.

```ok file=copy.ok
fn main(target: string, mode: string = "fast", ...files: string) {
  print("target={target} mode={mode} files={files}")
}
```

```output run="ok copy.ok build"
target=build mode=fast files=[]
```

```output run="ok copy.ok build slow a.txt b.txt"
target=build mode=slow files=["a.txt", "b.txt"]
```

## Flags

Parameters that start with `--` are flags. They are given by name, in any order, before or after the positional arguments.

- A flag with a value is written `--name value` or `--name=value`.
- A `bool` flag is a switch: `--verbose` makes it `true`, and leaving it out makes it `false`.
- A `bool` flag whose default is `true` is turned off with `--no-name`; its help lists it as `--[no-]name`.
- A `list[T]` flag is repeated: `--tag a --tag b` gives `["a", "b"]`. Not giving it at all gives `[]`.
- An optional flag, `--label: string?`, is `null` when it is not given.
- A flag without a default and not optional, such as `--token: string`, is required.

Inside the function, a dash in a flag name becomes an underscore: `--dry-run` is the variable `dry_run`.

```ok file=build.ok
fn main(
  --target: string = "debug",
  --tag: list[string],
  --label: string?,
  --dry-run: bool,
  --color: bool = true,
) {
  print(target, tag, label, dry_run, color)
}
```

```output run="ok build.ok --target=release --tag a --tag b --dry-run --no-color"
release ["a", "b"] null true false
```

`--` ends the flags: everything after it is a positional argument, even if it starts with a dash. That is how you pass a value like `-x`, or a file actually named `--help`.

ok's own options, such as `--trace` and `--dry-run`, go before the script name. Everything after the name belongs to the script, so a script can declare a `--dry-run` flag of its own. If you put one of ok's options after the name by mistake, and the script does not declare it, the error tells you where it goes.

## Types

A parameter's type says how its text is turned into a value. The value is checked before the script runs, so `main` never sees a malformed number.

| Type | Accepts |
|---|---|
| `string`, `any` or no type | any text |
| `int` | a whole number, with an optional sign |
| `float` | a decimal number |
| `bool` | only as a flag, by being present |
| `path` | any text, as a path relative to where you ran ok |
| `duration` | duration syntax, such as `90s` or `1h30m` |
| `T?` | as `T`; leaving it out gives `null` |
| `list[T]` | a rest parameter or a repeated flag |
| `"a" \| "b"` | exactly one of the listed strings |

```ok file=wait.ok
fn main(host: string, --port: int = 80, --timeout: duration = 30s) {
  print("waiting up to {timeout} for {host}:{port}")
}
```

```output run="ok wait.ok db.internal --port 5432 --timeout 2m"
waiting up to 2m for db.internal:5432
```

```output run="ok wait.ok db.internal --port fivefourthreetwo" exit=2
error: --port expects an integer, got "fivefourthreetwo"
usage: ok wait.ok <host> [--port <int>] [--timeout <duration>]
run with --help for details
```

## Choices

When a parameter takes one of a fixed set of values, write them as its type. ok checks the value, lists the choices in `--help`, and suggests the closest one when the user mistypes:

```ok file=deploy.ok
# Deploy the site.
fn main(
  env: "staging" | "production",        # where to deploy
  --role: "admin" | "user" = "user",    # who deploys
) {
  print("deploying to {env} as {role}")
}
```

```output run="ok deploy.ok production --role admin"
deploying to production as admin
```

```output run="ok deploy.ok prod" exit=2
error: <env> expects one of "staging", "production", got "prod"
  = help: did you mean "production"?
usage: ok deploy.ok <env> [--role <admin|user>]
run with --help for details
```

A choice type is a checked string: inside `main`, `env` is the string `"production"`. A default must be one of the choices, and an optional choice is written with `null` among them, as in `--region: "eu" | "us" | null`. Shell completion offers the choices too (see [Tasks](tasks.md)).

## Short flags

A flag can have a one-letter short form, written after the long name and a comma:

```ok file=convert.ok
fn main(
  input: string,                   # file to read
  --out, -o: path = "out.json",    # where to write
  --verbose, -v: bool,             # print each row
  --quiet, -q: bool,               # print nothing
) {
  print(input, out, verbose, quiet)
}
```

```output run="ok convert.ok data.csv -vq -o result.json"
data.csv result.json true true
```

Switches combine, so `-vq` is `-v -q`. A short flag that takes a value takes the next argument: `-o result.json`. It does not combine with other letters, and `-oresult.json` is an error that shows the right form:

```output run="ok convert.ok data.csv -oresult.json" exit=2
error: -o takes a value, given as the next argument
  = help: write -o VALUE (or --out VALUE)
usage: ok convert.ok <input> [--out <path>] [--verbose] [--quiet]
run with --help for details
```

Short letters are case-sensitive. Some are not allowed: `-h` is always help, and `-C` and `-e` are ok's own options, so they mean the same before and after the script name. A switch whose default is `true` has no short form, because it can only be turned off with `--no-name`. Negative numbers such as `-5` are values, not flags.

## Help from doc comments

`--help` (or `-h`) prints help built from the signature and its comments. The comment block right above `main` is the description, and the comment at the end of a parameter's line is that parameter's help:

```ok file=backup.ok
# Back up a directory into a dated archive.
#
# The archive goes to the current directory unless --dest says otherwise.
fn main(
  source: path,                                # directory to back up
  --dest: path = env.get("BACKUP_DIR", "."),   # where to put the archive
  --keep, -k: int = 7,                         # how many archives to keep
  --format: "zip" | "tar" = "tar",             # archive format
  --compress: bool = true,                     # compress the archive
) {
  print(source, dest, keep, format, compress)
}
```

```output run="ok backup.ok --help"
Back up a directory into a dated archive.

The archive goes to the current directory unless --dest says otherwise.

Usage: ok backup.ok <source> [--dest <path>] [--keep <int>] [--format <zip|tar>] [--no-compress]

Arguments:
  <source>                directory to back up

Options:
      --dest <path>       where to put the archive [default: env.get("BACKUP_DIR", ".")]
  -k, --keep <int>        how many archives to keep [default: 7]
      --format <zip|tar>  archive format [default: tar]
      --[no-]compress     compress the archive [default: true]
  -h, --help              print this help
```

A few details to notice:

- A literal default is shown as its value. Any other default, like the `env.get(...)` call, is shown as its source text, because printing help never runs your code.
- The usage line always uses the long flag names, which say what they do.
- A `bool` flag that defaults to `true` is listed as `--[no-]name`: `--compress` keeps the default and `--no-compress` turns it off, and the usage line shows `--no-compress`, the form that changes something. The help text is the comment you wrote for the parameter, so phrase it to describe the setting (here "compress the archive") rather than the act of turning it off.
- `--help` is only recognised before `--`; `ok backup.ok -- --help` passes `--help` as the source directory.

A script without `main` gets help too: the first comment block of the file (after a `#!` line) is its description, and the usage line is `ok script.ok [ARGS...]`.

## Argument errors and exit codes

A missing, unknown or malformed argument prints one error line, a hint when there is one, and the usage line, all to stderr, and exits with code 2 without running any of the script:

```ok file=greet.ok
print("this line never runs")

fn main(name: string, --shout: bool) {
  print(if shout { upper(name) } else { name })
}
```

```output run="ok greet.ok Ada --shuot" exit=2
error: unknown flag --shuot
  = help: did you mean --shout?
usage: ok greet.ok <name> [--shout]
run with --help for details
```

Exit code 2 is the usual signal for "you called it wrong", which lets a caller tell it apart from code 1, an error inside the script. Once the arguments are accepted, the script's exit code follows the rules on the [Errors](errors.md#exit-codes) page: 0 when `main` returns, 1 for an uncaught error, a failed command's own code, or whatever you pass to `exit()`. The value `main` returns is ignored.

## Making a script executable

On Linux and macOS, start the file with `#!/usr/bin/env ok` and make it executable. You can then run it by its path, and the arguments work the same way:

```ok file=hello
#!/usr/bin/env ok
# Say hello.
fn main(name: string = "world") {
  print("hello, {name}")
}
```

```
$ chmod +x hello
$ ./hello Ada
hello, Ada
```

ok treats the `#!` line as a comment. Windows ignores it, so there you run the file as `ok ./hello`. Note the `./`: `ok hello` would look for a task named `hello`, because ok treats a name as a script only when it ends in `.ok` or contains a path separator.

## Where to go next

- [Tasks](tasks.md) uses the same signatures for project tasks, so `ok deploy production --role admin` works the same way.
- [Running commands](commands.md) covers what most scripts do next: start programs.
- The spec has the fine print on [the script command line](../SPEC.md#14-script-cli-main).
