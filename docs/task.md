# Chomp Tasks

The [`chompfile.toml`](chompfile.md) defines Chomp tasks as a list of Task objects of the form:

chompfile.toml
```toml
version = 0.1

default_task = 'echo'

[[task]]
name = 'echo'
run = 'echo "Chomp"'
```

`chomp echo` or `chomp` will output the echo command.

## Task Execution

Chomp tasks are primarily characterized by their `"run"` and `"engine"` pair, representing a task execution in an environment.

There are two ways to execute in Chomp:

* Execute a task by name - `chomp [name]` where `[name]` is the `name` field of the task being run.
* Execute a task by filename - `chomp [path]` where `[path]` is the local path relative to the Chompfile being generated.

For example:

chompfile.toml
```toml
version = 0.1

[[task]]
name = 'my-task'
target = 'output.txt'
run = 'cat "Chomp Chomp" > output.txt'
```

This task writes the text `Chomp Chomp` into the file at `output.txt`, defining this as a target file output of the task.

The following are all equivalent ways to call this task:

```sh
$ chomp my-task
$ chomp :my-task
$ chomp output.txt
```

The leading `:` can be useful to disambiguate task names from file names when necessary. Setting a `name` on a task is completely optional.

### Shell Tasks

The default `engine` is the shell environment - PowerShell on Windows or SH on posix machines.

Command commands like `echo`, `pwd`, `cat`, `rm`, `cp`, `cd`, as well as operators like `$(cmd)`, `>`, `>>`, `|` have a subset of shared behaviours that can work when scripting between platforms. With some care and testing, it is thus possible to write cross-platform shell task scripts. In PowerShell, Chomp will automatically put the streaming commands in UTF-8 mode, although a BOM will still be output when using an operator like `>`.

In addition to the `run` property, two other useful task properties are `env` and `cwd` which allow customizing the exact execution environment:



### Node.js Engine

### Deno Engine

## Task Dependence

## Task Invalidation

## Task Interpolation

## Template Tasks

