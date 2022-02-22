# Chomp Tasks

The [`chompfile.toml`](chompfile.md) defines Chomp tasks as a list of Task objects of the form:

_chompfile.toml_
```toml
version = 0.1

default_task = 'echo'

[[task]]
name = 'echo'
run = 'echo "Chomp"'
```

_<div style="text-align: center">An example Chompfile.</div>_

Running `chomp echo` or `chomp` will output the echo command.

## Task API

Tasks support the following optional properties:

* **name**: `String`, the unique task name string.
* **target**: `String`, the file path created or modified by this task. Singular sugar for a single `targets: [String]`.
* **targets**: `String[]`, the list of file paths created or modified by this task, identical to `target` when there is a single target.
* **dep**: `String`, the task names or file paths this task [depends on](#task-dependence). Singular sugar for a single `deps: [String]`.
* **deps**: `String[]`, the task names of file paths this task [depends on](#task-dependence), identical to `dep` when there is a single dependency.
* **serial**: `Boolean`, whether [task dependencies](#task-dependence) should be processed in serial order. Defaults to false for parallel task processing.
* **invalidation**: `"always" | "mtime" (default) | "not-found"`, the [task caching invalidation rules](#task-invalidation). By default a task is cached based on its target path having an mtime greater than its dependencies per "make" semantics. `"always"` never caches, and `"not-found"` will never rerun the task if the target exists.
* **display**: `"none" | "init-status" | "init-only" | "status-only"`, defaults to `"init-status"`. Useful to reduce noise in the output log. Init is the note that the task has begun, while status is the note of task success or caching. Task errors will always be reported even with `display: 'none'`.
* **stdio**: `"none" | "no-stdin" | "stdout-only" | "stderr-only" | "all"`, defaults to `"all"` where stderr and stdout are piped to the main process output and stdin is also accepted. Set to `"no-stdin"` to disable the stdin for tasks. `"stdout-only"` and `"stderr-only"` will output only those streams.
* **engine**: `"node" | "deno" | "cmd" (default)`, the [execution engine](#task-execution) to use for the `run` string. For `node` or `deno` it is a Node.js or Deno program source string as if executed in the current directory.
* **run**: `String`, the source code string to run in the `engine`.
* **cwd**: `String`, the working directory to use for the `engine` execution.
* **env**: `{ [key: String]: String }`, custom environment variables to set for the `engine` execution.
* **env-default**: `{ [key: String]: String }`, custom default environment variables to set for the `engine` execution, only if not already present in the system environment.
* **env-replace**: `Boolean`, defaults to `true`. Whether to support `${{VAR}}` style static environment variable replacements in the `env` and `env-default` environment variable declarations and the `run` script of Shell engine tasks.
* **template**: `String`, a registered template name to use for task generation as a [template task](#extensions).
* **template-options**: `{ [option: String]: any }`, the dictionary of options to apply to the `template` [template generation](#extensions), as defined by the template itself.

## Task Execution

Chomp tasks are primarily characterized by their `"run"` and `"engine"` pair, `"run"` representing the source code of a task execution in the `"engine"` execution environment. Currently supported engines include the shell execution (the default), Node.js (`engine = 'node'`) or Deno (`engine = 'deno'`).

There are two ways to execute in Chomp:

* Execute a task by _name_ - `chomp [name]` or `chomp :[name]` where `[name]` is the `name` field of the task being run.
* Execute a task by _target_ file path - `chomp [target]` where `[target]` is the local file path to generate relative to the Chompfile being run.

_chompfile.toml_
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

### Task Completion

A task is considered to have succeeded if it completes with a zero exit code, and the target or targets
expected of the task all exist.

If the spawned process returns a non-zero exit code the task and all its parents will be marked as failed.

If after completion, any of the targets defined for the task still do not exist, then the task is also marked as failed.

### Shell Tasks

The default `engine` is the shell environment - PowerShell on Windows or `sh` on posix machines.

Common commands like `echo`, `pwd`, `cat`, `rm`, `cp`, `cd`, as well as operators like `$(cmd)`, `>`, `>>`, `|` form a subset of shared behaviours that can work when scripting between all platforms. With some care and testing, it is possible to write cross-platform shell task scripts. For PowerShell 5, Chomp will execute PowerShell in UTF-8 mode (applying to `>`, `>>` and `|`), although a BOM will still be output when writing a new file with `>`. Since `&&` and `||` are not supported in Powershell, multiline scripts and `;` are preferred instead.

For example, here is an SWC task (assuming Babel is installed via `npm install @swc/core @swc/cli -D`):

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'build:swc'
target = 'lib/app.js'
dep = 'src/app.ts'
run = 'swc $DEP -o $TARGET --source-maps'
```

_<div style="text-align: center">SWC task compiling the TypeScript module `src/app.ts` into a JS module `lib/app.js`, and supporting configuration in an `.swcrc` file.</div>_

The above works without having to reference the full `node_modules/.bin/swc` command prefix since `node_modules/.bin` is automatically included in the Chomp spawned `PATH`.

### Environment Variables

In addition to the `run` property, two other useful task properties are `env` and `cwd` which allow customizing the exact execution environment.

In PowerShell, defined environment variables in the task `env` are in addition made available as local variables supporting output via `$NAME` instead of `$Env:Name` for better cross-compatibility with posix shells. This process is explicit only - system-level environment variables are not given this treatment though.

In addition, static environment variable replacements are available via `${{VAR}}`, with optional spacing. Replacements that cannot be resolved to a known environment variable will be replaced with an empty string. Static replacements are available for environment variables and the shell engine run command. Set `env-replace = false` to disable static environment variable replacement for a given task.

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'env-vars'
run = '''
  ${{ECHO}} $PARAM1 $PARAM2
'''
[task.env]
PARAM1 = 'Chomp'

[task.default-env]
ECHO = 'echo'
PARAM2 = '${{ PARAM1 }}'
```

_<div style="text-align: center">Custom environment variables are also exposed as local variables in PowerShell, while `${{VAR}}` provides static replacements.</div>_

On both Posix and Windows, `chomp env-vars` will output: `Chomp Chomp`, unless the system has overrides of the `CMD` or `PARAM2` environment variables to alternative values.

`default-env` permits the definition of default environment variables which are only set to the default values if these environment variables are not already set in the system environment or via the global Chompfile environment variables. Just like `env`, all variables in `default-env` are also defined as PowerShell local variables, even when they are already set in the environment and the default does not apply.

The following task-level environment variables are always defined:

* `TARGET`: The path to the primary target (the interpolation target or first target).
* `TARGETS`: The `:`-separated list of target paths for multiple targets.
* `DEP`: The path to the primary dependency (the interpolation dependency or first dependency file).
* `DEPS`: The `:`-separated list of expanded dependency paths.
* `MATCH` When using [task interpolation](#task-interpolation) this provides the matched interpolation replacement value (although the `TARGET` will always be the fully substituted interpolation target for interpolation tasks).

The `PATH` environment variable is automatically extended to include `.bin` in the current folder as well as `node_modules/.bin` in the Chompfile folder.

### Node.js Engine

The `"node"` engine allows writing a Node.js program in the `run` field of a task. This is a useful way to encapsulate cross-platform build scripts which aren't possible with cross-platform shell scripting.

For example, the same SWC task in Node.js can be written:

_chompfile.toml_ls
```toml
version = 0.1

[[task]]
name = 'build:swc'
target = 'lib/app.js'
dep = 'src/app.ts'
engine = 'node'
run = '''
  import swc from '@swc/core';
  import { readFileSync, writeFileSync } from 'fs';
  import { basename } from 'path';

  const input = readFileSync(process.env.DEP, 'utf8');

  const { code, map } = await swc.transform(input, {
    filename: process.env.DEP,
    sourceMaps: true,
    jsc: {
      parser: {
        syntax: "typescript",
      },
      transform: {},
    },
  });

  writeFileSync(process.env.TARGET, code + '\n//# sourceMappingURL=' + basename(process.env.TARGET) + '.map');
  writeFileSync(process.env.TARGET + '.map', JSON.stringify(map));
'''
```

It is usually preferable to write tasks using shell scripts since they can be much faster than bootstrapping Node.js or Deno, and can more easily support [batching](extensions.md#chompregisterbatchername-string-batcher-batch-cmdop-running-batchcmd--batcherresult--undefined).

> It is usually easier to use the existing [`chomp:swc` experimental template extension](#extensions) instead of writing your own custom task for SWC.

### Deno Engine

Just like the `"node"` engine, the `"deno"` engine permits using JS to create build scripts.

The primary benefits being URL import support (no need for package management for tasks) and TypeScript type support (although unfortunately no editor plugins for Chompfiles means it doesn't translate to author time currently). Using a CDN like [JSPM.dev](https://jspm.org/docs/cdn#jspmdev) (importing eg `https://jspm.dev/[pkg]` etc) can be useful for these scripts to load npm packages.

By default the Deno engine will run with full permissions since that is generally the nature of build scripts.

## Task Interpolation

Chomp works best when each task builds a single file target, instead of having a large monolithic build.

To extend the previous example to build all of `src` into `lib`, we use **task interpolation** with the `#` which means the same thing as a `**/*` glob, but it retains the important property of being a reversible mapping which is necessary for tracing task invalidations.

Replacing `app` with `#` in the previous [SWC Shell example](#shell-tasks), we can achieve the full folder build:

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'build:swc'
target = 'lib/#.js'
dep = 'src/#.ts'
run = 'swc $DEP -o $TARGET --source-maps'
```
_<div style="text-align: center">Chomp task compiling all `.ts` files in `src` into JS modules in `lib`.</div>_

By treating each file as a separate build, we get natural build parallelization and caching where only files changed in `src` cause rebuilds.

Just like any other target, interpolation targets can be built directly (or even with globbing):

```sh
$ chomp lib/app.js
```
_<div style="text-align: center">When building an exact interpolation target, only the minimum work is done to build `lib/app.js` - no other files in `src` need to be checked other than `src/app.js`.</div>_

Only a single interpolation `dep` and `target` can be defined (with the `#` interpolation character), although additional dependencies or targets may be defined in addition by using the `deps` array instead, for example to make each compilation depend on the npm install:

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'npm:install'
run = 'npm install'

[[task]]
name = 'build:swc'
target = 'lib/#.js'
deps = ['src/#.js', 'npm:install']
run = 'swc $DEP -o $TARGET --source-maps'
```
_<div style="text-align: center">`$DEP` and `$TARGET` will always be the primary dependency and target (the interpolation item or the first in the list). Additional dependencies and targets can always be defined.</div>_

### Testing

While Chomp is not designed to be a test runner, it can easily provide many the features of one.

Tests can be run with interpolation. Since interpolation expands a glob of dependencies to operate on, this same technique can be used to create targetless tests:

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'test:unit:#'
display = 'status'
stdio = 'stderr-only'
dep = ['test/unit/#.js', 'dist/build.js']
run = 'node $DEP'
```
_<div style="text-align: center">Task interpolation without a target runs the task over all dependencies, and is always invalidated, exactly what is needed for a test runner.</div>_

In the above, all files `test/unit/**/*.js` will be expanded by the `test:unit` test resulting in a separate task run for each file. Since no `targets` are defined, the task is always invalidated and re-run.

Using the `display` and `stdio` options it is also possible to hide any test output and the command init logs in the reporter.

By using `#` in the `name` of the task, individual test or test patterns can be run by name or using glob patterns:

```sh
$ chomp --watch test:unit:some-test test:unit:some-suite-*
```

The above would run the tests `test/unit/some-test.js`, and all `test/unit/some-suite-*.js`, watching the full build graph and every unit test file for changes and rerunning the tests on change.

Alternatively all unit tests can be run by passing the empty string replacement:

```sh
$ chomp test:unit:
$ chomp test:unit:**/*
```
_<div style="text-align: center">Both lines above are equivalent given the task name `test:unit:#`, running all the unit tests.</div>_

## Task Dependence

Using the `deps` and `targets` properties (which are interchangeable with their singular forms `dep` and `target` for a single list item), task dependence graphs are built.

When processing a task, the task graph is constructed and processed in graph order where a task will not begin until its dependencies have completed processing.

Dependencies of tasks are always treated as being parallel - to ensure one task always happens before another the best way is usually to treat it as a dependency. For example by having a test task depend on the build target.

Task parallelization can be controlled by the [`-j` flag](cli.md#jobs) to set the maximum number of parallel child processes to spawn.

For example, here is a build that compiles with SWC, then builds into a single file with RollupJS:

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'npm:install'
run = 'npm install'

[[task]]
name = 'build:swc'
target = 'lib/#.js'
deps = ['src/#.js', 'npm:install']
run = 'swc $DEP -o $TARGET --source-maps'

[[task]]
name = 'build:rollup'
dep = 'lib/**/*.js'
target = 'dist/app.js'
run = 'rollup lib/app.js -d dist -m'
```

_<div style="text-align: center">Practical example of a Chomp build graph using task dependence from the npm install to per-file SWC compilation to Rollup into a single file or set of files.</div>_

Following the task graph from the top, since `build:rollup` depends on all deps in `lib`, this will make it depend on all the separate file interpolation jobs of `build:swc` and in turn their dependence. With each of the `build:swc` tasks depending on `npm:install`, this task is always run first. Then only once the `npm install` is completed successfully, the compilation of all `src/**/*.ts` into `lib/#.js` will happen with full parallelization in the task graph.

Task dependency inputs can themselves be the result of targets of other tasks. Build order is fully determined by the graph in this way.

## Watched Rebuilds

Taking the [previous example](#task-dependence) and running:

```sh
$ chomp build:rollup --watch
```
_<div style="text-align: center">Fine-grained watched rebuilds are a first-class feature in Chomp.</div>_

will build the `dist/app.js` file and then continue watching all of the input files in `src/**/*.ts` as well as the `package.json`. A change to any of these files will then trigger a granular live rebuild of only the changed TypeScript file or files.

## Static Server

As a convenience a simple local static file server is also provided:

```sh
$ chomp build-rollup --serve
```
_<div style="text-align: center">Running the Chomp static server.</div>_

This behaves identically to the watched rebuilds above, but will also serve the folder on localhost for browser and URL tests. This may seem outside of the expected features for a task runner, but it is actually closely associated with the watched rebuild events - a websocket protocol for in-browser hot-reloading is a [planned future addition](https://github.com/guybedford/chomp/issues/61).

Server configuration can be controlled via the [`serve`](cli.md#serve) options in the Chompfile or the [`--server-root`](cli.md#server-root) and [`--port`](cli.md#port) flags.

By separating monolithic builds into sub-compilations on the file system this enables caching, parallelization, finer-grained generic build control and comprehensive incremental builds with watcher support. Replacing monolithic JS build systems with make-style file caching all the commonly expected features of JS dev workflows can still be maintained.

## Task Caching

Tasks are cached when the _modified time_ of their `targets` is more recent than the modified time of their `deps` per standard Make-style semantics.

For example, if we change the npm task definition from the previous example to define the `dep` as the `package.json` and the `target` as the `package-lock.json`:

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'npm:install'
run = 'npm install'
target = 'package-lock.json'
dep = 'package.json'
```

The `npm install` operation will now be treated as cached and skipped, unless the `package.json` has been more recently modified than the `package-lock.json`.

The invalidation rule is a binary rule indicating whether or not a given task should rerun or be treated as cached.

The explicit rules of invalidation for this `mtime` invalidation are:

* If no targets are defined for a task, it is always invalidated.
* Otherwise, if no deps are defined for a task, it is invalidated only if the targets do not exist.
* Otherwise, if the mtime of any dep is greater than the mtime of any target, the task is invalidated.

Task invalidation can be customized with the `invalidation` property on a task:

* `invalidation = 'mtime'` _(default)_: This is the default invalidation, as per the rules described above.
* `invalidation = 'always'`: The task is always invalidated and rerun, without exception.
* `invalidation = 'not-found'`: The task is only invalidated when not all targets are defined.

## Serial Dependencies

In some cases, it can be preferred to write a serial pipeline of steps that should be followed.

This can be achieved by setting `serial = true` on the task:

_chompfile.toml_
```toml
version = 0.1

[[task]]
name = 'test'
serial = true
deps = ['test:a', 'test:b', 'test:c']

[[tas]]
name = 'test:a'
run = 'echo a'

[[task]]
name = 'test:b'
run = 'echo b'

[[task]]
name = 'test:c'
run = 'echo c'
```
_<div style="text-align: center">Example of a serial `test` task executing `test:a` then `test:b` then `testc` in sequence.</div>_

Running `chomp test` with the above, will run each of `test:a`, `test:b` and `test:c` one after the other to completion instead of running their dependence graphs in parallel by default, logging `a b c` every time.

## Extensions

Extensions are loaded via the `extensions` list in the Chompfile, and can define custom task templates, which can encapsulate the details of a task execution into a simpler definition.

For convenience Chomp provides an experimental [core extensions library](https://github.com/guybedford/chomp-extensions).

For example, to replace the npm, SWC and RollupJS compilations from the previous examples with their extension templates:

_chompfile.toml_
```toml
version = 0.1

extensions = ['chomp:npm', 'chomp:swc', 'chomp:rollup']

[[task]]
name = 'npm:install'
template = 'npm'

[[task]]
name = 'build:swc'
template = 'swc'
target = 'lib/#.js'
deps = ['src/#.js', 'npm:install']
[task.template-options]
source-maps = true

[[task]]
name = 'build:rollup'
template = 'rollup'
deps = 'lib/**/*.js'
[task.template-options]
outdir = 'dist'
entries = ['lib/app.js']
```
_<div style="text-align: center">Using the `chomp:npm`, `chomp:swc` and `chomp:rollup` [experimental core extensions](https://github.com/guybedford/chomp-extensions) allows writing these tasks encapsulated from their implementations.</div>_

Templates can be loaded from any file path or URL.

### Remote Extensions

Extensions support any `https://` URLs or local file paths.

Remote extensions are loaded once and cached locally by Chomp, regardless of cache headers, to ensure the fastest run time.

The remote extension cache can be cleared by running `chomp --clear-cache`.

### Ejection

`chomp --eject` transforms the Chompfile into the expanded untemplated form without extensions, allowing an opt-out from extension template workflows if it ever feels too magical. In this way templates become a sort of task construction utility.

### Writing Templates

> Read more on writing extensions in the [extensions documentation](extensions.md)

Chomp extensions can be loaded from any URL or local file path. To write custom templates, create a local extension file `local-extension.js` referencing it in the extensions list of the Chompfile:

_chompfile.toml_
```toml
version = 0.1

extensions = ['./local-extension.js']
```

_local-extension.js_
```js
Chomp.registerTemplate('npm', function (task) {
  return [{
    name: task.name,
    run: 'npm install',
    target: 'package-lock.json',
    deps: ['package.json']
  }];
});

Chomp.registerTemplate('swc', function (task) {
  const { sourceMaps } = task.templateOptions;
  return [{
    name: task.name,
    targets: task.targets,
    deps: task.deps,
    run: `swc $DEP -o $TARGET${sourceMaps ? ' --source-maps' : ''}`
  }];
});

Chomp.registerTemplate('rollup', function (task) {
  if (task.targets.length > 0)
    throw new Error('Targets is not supported by the Rollup template, use the "outdir" and "entries" template options instead.');
  const { outdir, entries } = task.templateOptions;
  const targets = entries.map(entry => outdir + '/' + entry.split('/').pop());
  return [{
    name: task.name,
    deps: task.deps,
    targets,
    run: `rollup ${entries.join(' ')} -d ${outdir} -m`
  }];
});
```
_<div style="text-align: center">Chomp extension template registration example loaded via a local extension at `local-extension.js` for the `npm`, `swc` and `rollup` templates.</div>_

Templates are functions on tasks returning a new list of tasks. All TOML properties apply but with _camelCase_ instead of _kebab-case_.

PRs to the default Chomp extensions library are welcome, or host your own on your own domain or via an npm CDN. For support on the JSPM CDN, add `"type": "script"` to the `package.json` of the package to avoid incorrect processing since template extensions are currently scripts and not modules.

Because remote extensions are cached, it is recommended to always use unique URLs with versions when hosting extensions remotely. 

See the extensions documentation for the full [extensions API](extensions.md#api).
