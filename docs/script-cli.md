# Command-line scripts

Declare `fn main(...)` to give a script a command line. Its parameters become positional arguments and flags, with types, defaults, short flags and a `--help` text.

## Declaring main

When a script has a `fn main`, ok builds the command line from its parameters. ok checks the arguments against the signature before any of your code runs, and then calls `main` with them. This script takes a name, a number and a switch:

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

The flags are optional, and they can follow the name:

```output run="ok greet.ok Ada --times 2 --shout"
HELLO, ADA!
HELLO, ADA!
```

The signature is the only description of the command line. The user sees its names, types, defaults and comments in `--help`, and your code receives the checked values. The sections below describe each part of the signature.

Code at the top level of the script runs first, and `main` runs after it. `main` may have no parameters. A script with `fn main()` accepts no arguments.

## Positional parameters

A parameter without `--` is positional. Positional arguments are given in the order of the parameters.

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

A parameter that starts with `--` is a flag. Flags are given by name, in any order, before or after the positional arguments.

- A flag with a value is written `--name value` or `--name=value`.
- A `bool` flag is a switch: `--verbose` makes it `true`, and leaving it out makes it `false`.
- A `bool` flag whose default is `true` is turned off with `--no-name`. Its help lists it as `--[no-]name`.
- A `list[T]` flag is repeated: `--tag a --tag b` gives `["a", "b"]`. Leaving it out gives `[]`.
- An optional flag, `--label: string?`, is `null` when it is not given.
- A flag that has no default and is not optional, such as `--token: string`, is required.

Inside the function, a dash in a flag name becomes an underscore, so `--dry-run` is the variable `dry_run`. This script declares one flag of each kind except a required one:

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

`--` ends the flags. Everything after it is a positional argument, also when it starts with a dash. Use `--` to pass a value such as `-x`, or a file that is named `--help`.

ok's own options, such as `--trace` and `--dry-run`, go before the script name. Everything after the name belongs to the script, so a script can declare a `--dry-run` flag of its own. If you put one of ok's options after the name and the script does not declare it, the error says where the option goes.

## Parameter types

The type of a parameter says how ok turns the text of an argument into a value. ok checks the text before the script runs, so `main` never receives a malformed number.

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

A value of the wrong type is an error:

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

A value that is not among the choices is an error:

```output run="ok deploy.ok prod" exit=2
error: <env> expects one of "staging", "production", got "prod"
  = help: did you mean "production"?
usage: ok deploy.ok <env> [--role <admin|user>]
run with --help for details
```

Inside `main`, a choice is an ordinary string, so `env` is `"production"` here. A default must be one of the choices. For an optional choice, write `null` among the choices, as in `--region: "eu" | "us" | null`. Shell completion offers the choices too, as [Tasks](tasks.md) describes.

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

Switches combine, so `-vq` is `-v -q`. A short flag that takes a value takes the next argument, as in `-o result.json`. It does not combine with other letters, and `-oresult.json` is an error that shows the right form:

```output run="ok convert.ok data.csv -oresult.json" exit=2
error: -o takes a value, given as the next argument
  = help: write -o VALUE (or --out VALUE)
usage: ok convert.ok <input> [--out <path>] [--verbose] [--quiet]
run with --help for details
```

Short letters are case-sensitive. Three letters are reserved: `-h` is always help, and `-C` and `-e` are ok's own options. A switch whose default is `true` has no short form, because it can only be turned off with `--no-name`. A negative number such as `-5` is a value, not a flag.

## Writing the --help text

`--help` (or `-h`) prints help that ok builds from the signature and its comments. The comment block right above `main` is the description, and the comment at the end of a parameter's line is the help for that parameter:

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

In this help text:

- A literal default is shown as its value. Any other default, such as the `env.get(...)` call, is shown as its source text, because printing help never runs your code.
- The usage line always uses the long flag names.
- A `bool` flag that defaults to `true` is listed as `--[no-]name`. `--compress` keeps the default and `--no-compress` turns it off. The usage line shows `--no-compress`, the form that changes something. The same comment stands next to both forms, so write it to describe the setting, here "compress the archive".

`--help` is only recognised before `--`. `ok backup.ok -- --help` passes `--help` as the source directory.

A script without `main` gets help too. The first comment block of the file, after a `#!` line if there is one, is its description, and the usage line is `ok script.ok [ARGS...]`.

## Argument errors and exit codes

For a missing, unknown or malformed argument, ok prints one error line, a hint when there is one, and the usage line, all to stderr. It exits with code 2 without running any of the script:

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

By convention, exit code 2 means that a program was called with wrong arguments. A caller can tell it apart from code 1, which is an error inside the script.

Once the arguments are accepted, the exit code of the script follows the rules on the [Errors](errors.md#exit-codes) page. It is 0 when `main` returns, 1 for an uncaught error, the code of a failed command, or the code that you pass to `exit()`. The value that `main` returns is ignored.

## Reading the arguments without main

A script without a `main` function gets its command-line arguments as the list of strings `args`, exactly as they were given:

```ok file=echo-args.ok
print(args, len(args))
```

```output run="ok echo-args.ok one 'two words' --three"
["one", "two words", "--three"] 3
```

`args` is enough for a short script that takes a file name or two. For a script with flags, declare `main`. Otherwise you have to parse `args`, check the types and write a usage message yourself.

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

ok treats the `#!` line as a comment. Windows ignores it, so there you run the file as `ok ./hello`. The `./` is required. `ok hello` looks for a task named `hello`, because ok treats a name as a script only when it ends in `.ok` or contains a path separator.

## Where to go next

- [Tasks](tasks.md) uses the same signatures for project tasks, so `ok deploy production --role admin` works the same way.
- [Running commands](commands.md) explains how a script starts programs.
- The spec defines [the script command line](../SPEC.md#14-script-cli-main) exactly.
