# Chomp Tasks

The [`chompfile.toml`](chompfile.md) defines Chomp tasks as a list of Task objects of the form:

chompfile.toml
```toml
version = 0.1

default`task = 'echo'

[[task]]
name = 'echo'
run = 'echo "Chomp"'
```

`chomp echo` or `chomp` will output the echo command.

## Task API

Tasks have the following optional properties:

* **name**: `String`, the unique task name string.
* **target**: `String`, the file path created or modified by this task.
* **targets**: `String[]`, the list of file paths created or modified by this task.
* **dep**: `String`, the task names or file paths this task [depends on](#task-dependence).
* **deps**: `String[]`, the task names of file paths this task [depends on](#task-dependence).
* **serial**: `Boolean`, whether [task dependencies](#task-dependence) should be processed in serial order. Defaults to false for parallel task processing.
* **invalidation**: `"always" | "mtime" (default) | "not-found"`, the [task caching invalidation rules](#task-invalidation). By default a task is cached based on its target path having an mtime greater than its dependencies per "make" semantics. `"always"` never caches, and `"not-found"` will never rerun the task if the target exists.
* **display**: `Boolean`, defaults to `true`. Set to `false` to hide the task from being reported in the output log.
* **engine**: `"node" | "deno" | "cmd" (default)`, the [execution engine](#task-execution) to use for the `run` string. For `node` or `deno` it is a Node.js or Deno program source string as if executed in the current directory.
* **run**: `String`, the source code string to run in the `engine`.
* **cwd**: `String`, the working directory to use for the `engine` execution.
* **env**: `{ [key: String]: String }`, custom environment variables to set for the `engine` execution.
* **env-default**: `{ [key: String]: String }`, custom default environment variables to set for the `engine` execution, only if not already present in the system environment.
* **template**: `String`, a registered template name to use for task generation as a [template task](#template-tasks).
* **template-options**: `{ [option: String]: any }`, the dictionary of options to apply to the `template` [template generation](#template-tasks), as defined by the template itself.

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

The default `engine` is the shell environment - PowerShell on Windows or `sh` on posix machines.

Common commands like `echo`, `pwd`, `cat`, `rm`, `cp`, `cd`, as well as operators like `$(cmd)`, `>`, `>>`, `|` form a subset of shared behaviours that can work when scripting between all platforms. With some care and testing, it is possible to write cross-platform shell task scripts. For PowerShell 5, Chomp will execute PowerShell in UTF-8 mode, although a BOM will still be output when using an operator like `>`.

In addition to the `run` property, two other useful task properties are `env` and `cwd` which allow customizing the exact execution environment.

### Node.js Engine

### Deno Engine

## Task Dependence

## Task Invalidation

## Task Interpolation

## Template Tasks

