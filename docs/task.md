# Chomp Tasks

The [`chompfile.toml`](chompfile.md) defines Chomp tasks as a list of Task objects of the form:

**chompfile.toml**
```toml
version = 0.1

default_task = 'echo'

[[task]]
name = 'echo'
run = 'echo "Chomp"'
```

_<div style="text-align: center">An example Chompfile</div>_

`chomp echo` or `chomp` will output the echo command.

## Task API

Tasks support the following optional properties:

* **name**: `String`, the unique task name string.
* **target**: `String`, the file path created or modified by this task. Singular form of `targets`. Singular sugar for a single `targets: [String]`.
* **targets**: `String[]`, the list of file paths created or modified by this task, identical to `target` when there is a single target.
* **dep**: `String`, the task names or file paths this task [depends on](#task-dependence). Singular sugar for a single `deps: [String]`.
* **deps**: `String[]`, the task names of file paths this task [depends on](#task-dependence), identical to `dep` when there is a single dependency.
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

Chomp tasks are primarily characterized by their `"run"` and `"engine"` pair, `"run"` representing the source code of a task execution in the `"engine"` execution environment. Currently supported engines include the shell execution (the default), Node.js (`engine = 'node'`) or Deno (`engine = 'deno'`).

There are two ways to execute in Chomp:

* Execute a task by name - `chomp [name]` or `chomp :[name]` where `[name]` is the `name` field of the task being run.
* Execute a task by filename - `chomp [path]` where `[path]` is the local path relative to the Chompfile being generated.

**chompfile.toml**
```toml
version = 0.1

[[task]]
name = 'my-task'
target = 'output.txt'
run = 'cat "Chomp Chomp" > output.txt'
```
_<div style="text-align: center">This task writes the text `Chomp Chomp` into the file at `output.txt`, defining this file as a target output path of the task so that the task is cached.</div>_

This task writes the text `Chomp Chomp` into the file at `output.txt`, defining this as a target file output of the task.

```sh
$ chomp my-task
$ chomp :my-task
$ chomp output.txt

🞂 output.txt
√ output.txt [3.8352ms]
```

_<div style="text-align: center">The same task can be called by task name (with or without `:` prefix) or by target path.</div>_

The leading `:` can be useful to disambiguate task names from file names when necessary. Setting a `name` on a task is completely optional.

Once the task has been called, with the target file already existing it will treat it as cached and skip subsequent executions:

```sh
$ chomp my-task

● output.txt [cached]
```

### Shell Tasks

The default `engine` is the shell environment - PowerShell on Windows or `sh` on posix machines.

Common commands like `echo`, `pwd`, `cat`, `rm`, `cp`, `cd`, as well as operators like `$(cmd)`, `>`, `>>`, `|` form a subset of shared behaviours that can work when scripting between all platforms. With some care and testing, it is possible to write cross-platform shell task scripts. For PowerShell 5, Chomp will execute PowerShell in UTF-8 mode (applying to `>`, `>>` and `|`), although a BOM will still be output when writing a new file with `>`.

### Environment Variables

In addition to the `run` property, two other useful task properties are `env` and `cwd` which allow customizing the exact execution environment.

In PowerShell, defined environment variables in the task `env` are in addition made available as local variables supporting output via `$NAME` instead of `$Env:Name` for better cross-compatibility with posix shells. This process is explicit only - system-level environment variables are not given this treatment though.

**chompfile.toml**
```toml
version = 0.1

[[task]]
name = 'env-vars'
run = '''
  echo $VAR $Env:VAR
'''
[task.env]
VAR = 'Chomp'

[task.default-env]
ANOTHER = '$VAR'
```

_<div style="text-align: center">Custom environment variables are also exposed as local variables in PowerShell.</div>_

On Windows, `chomp env-vars` will output: `Chomp Chomp Chomp`.

* `ANOTHER = "$VAR"` works as a convenience feature in Chomp for substituting environment variables in other environment variables.

* `default-env` permits the definition of default environment variables which are only set to the default values if these environment variables are not already set in the system environment or via the global Chompfile environment variables. Just like `env`, all variables in `default-env` are also defined as PowerShell local variables, even when they are already set in the environment and the default does not apply.

The following task-level environment variables are always defined:

* `TARGET`: The path to the current target (relative to the Chompfile / default CWD).
* `TARGETS`: The comma-separated list of target paths for multiple targets.
* `DEP`: The path to the dependency (relative to the Chompfile / default CWD).
* `DEPS`: The comma-separated list of dependency paths for multiple dependencies.
* `MATCH` When using [task interpolation](#task-interpolation) this provides the matched interpolation replacement value (although the `TARGET` will always be the fully substituted interpolation target for interpolation tasks).

### Node.js Engine

The `"node"` engine allows writing a Node.js program in the `run` field of a task. This is a useful way to encapsulate cross-platform build scripts which aren't possible with cross-platform shell scripting.

For example, here's how to write a Babel task:

chompfile.toml
```toml
version = 0.1

[[task]]
name = 'build:babel'
target = 'lib/app.js'
dep = 'src/app.js'
engine = 'node'
run = '''
  import babel from '@babel/core';
  import { readFileSync, writeFileSync } from 'fs';

  const input = readFileSync(process.env.DEP, 'utf8');
  const { code, map } = babel.transformSync(input, {
    filename: process.env.DEP,
    babelrc: false,
    configFile: false,
    sourceMaps: true,
    presets: [['@babel/preset-env', {}]],
    targets: ['> 0.25%, not dead']
  });

  writeFileSync(process.env.TARGET, code);
  writeFileSync(process.env.TARGET + '.map', map);
'''
```

### Deno Engine

Just like the `"node"` engine, the `"deno"` engine permits using JS to create build scripts.

The primary benefits being URL import support (no need for package management for tasks) and TypeScript type support (although unfortunately no editor plugins for Chompfiles means it doesn't translate to author time currently). Using a CDN like [JSPM.dev](https://jspm.org/docs/cdn#jspmdev) (importing eg `https://jspm.dev/@babel/core` etc) can be useful for these scripts to load npm packages.

By default the Deno engine will run with full permissions since that is generally the nature of build scripts.

## Task Interpolation

## Task Dependence

## Task Invalidation

## Task Arguments

## Template Tasks

