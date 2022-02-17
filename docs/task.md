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
* **display**: `"none" | "init-status" | "init-only" | "status-only"`, defaults to `"init-status"`. Useful to reduce noise in the output log. Init is the note that the task has begun, while status is the not of task success. Task errors will always be reported even with `display: 'none'`.
* **stdio**: `"none" | "no-stdin" | "stdout-only" | "stderr-only" | "all"`, defaults to `"all"` where stderr and stdout are piped to the main process output and stdin is also accepted. Set to `"no-stdin"` to disable the stdin for tasks. `"stdout-only"` and `"stderr-only"` will output only those streams.
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

#### Environment Variables

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

`ANOTHER = "$VAR"` works as a convenience feature in Chomp for substituting environment variables in other environment variables.

`default-env` permits the definition of default environment variables which are only set to the default values if these environment variables are not already set in the system environment or via the global Chompfile environment variables. Just like `env`, all variables in `default-env` are also defined as PowerShell local variables, even when they are already set in the environment and the default does not apply.

The following task-level environment variables are always defined:

* `TARGET`: The path to the current target (relative to the Chompfile / default CWD).
* `TARGETS`: The comma-separated list of target paths for multiple targets.
* `DEP`: The path to the dependency (relative to the Chompfile / default CWD).
* `DEPS`: The comma-separated list of dependency paths for multiple dependencies.
* `MATCH` When using [task interpolation](#task-interpolation) this provides the matched interpolation replacement value (although the `TARGET` will always be the fully substituted interpolation target for interpolation tasks).

The `PATH` environment variable is automatically extended to include `.bin` in the current folder as well as `node_modules/.bin` in the Chompfile folder.

For example, here is a Babel task (assuming Babel is installed via `npm install @babel/core @babel/cli`):

```toml
version = 0.1

[[task]]
name = 'build:babel'
target = 'lib/app.js'
dep = 'src/app.js'
run = 'babel $DEP -p $TARGET --source-maps'
```

_<div style="text-align: center">Babel task compiling `src/app.js` into `lib/app.js`, and supporting configuration in a `.babelrc` file.</div>_

### Node.js Engine

The `"node"` engine allows writing a Node.js program in the `run` field of a task. This is a useful way to encapsulate cross-platform build scripts which aren't possible with cross-platform shell scripting.

For example, the Babel task in Node.js can be written:

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
  import { basename } from 'path';

  const input = readFileSync(process.env.DEP, 'utf8');
  const { code, map } = babel.transformSync(input, {
    filename: process.env.DEP,
    babelrc: false,
    configFile: false,
    sourceMaps: true,
    presets: [['@babel/preset-env', {
      targets: {
        esmodules: true
      },
      modules: false
    }]],
  });
  writeFileSync(process.env.TARGET, code + '\n//# sourceMappingURL=' + basename(process.env.TARGET) + '.map');
  writeFileSync(process.env.TARGET + '.map', JSON.stringify(map));
'''
```

It is usually preferable to write tasks using shell scripts since they are generally much faster than bootstrapping Node.js or Deno, and can more easily support batching of the same commands.

### Deno Engine

Just like the `"node"` engine, the `"deno"` engine permits using JS to create build scripts.

The primary benefits being URL import support (no need for package management for tasks) and TypeScript type support (although unfortunately no editor plugins for Chompfiles means it doesn't translate to author time currently). Using a CDN like [JSPM.dev](https://jspm.org/docs/cdn#jspmdev) (importing eg `https://jspm.dev/@babel/core` etc) can be useful for these scripts to load npm packages.

By default the Deno engine will run with full permissions since that is generally the nature of build scripts.

## Task Interpolation

Chomp works best when each task builds a single file target, instead of having a large monolithic build.

The Babel task in the previous section takes as input `src/app.js` and outputs `lib/app.js`. When `lib/app.js` has a modified time on the file system greater than the modified time of `src/app.js` then the task is fresh and doesn't need to be rebuilt until `src/app.js` is changed.

To extend this build process from a single file to an entire folder of files Chomp provides task interpolation using the `#` symbol, which acts as a deep glob. The reason arbitrary globs are not supported is due to the requirement of 1-1 reversible mapping between the input and output of the interpolation.

Here's the shell Babel task using interpolation to build a folder of sources:

```toml
version = 0.1

[[task]]
name = 'build:babel'
target = 'lib/#.js'
dep = 'src/#.js'
run = 'babel $DEP -p $TARGET --source-maps'
```
_<div style="text-align: center">`src/**/*.js` is globbed, outputting a corresponding file in `lib`. By treating each file as a separate build, we get natural build parallelization and caching where only files changed in `src` cause rebuilds.</div>_

Only a single interpolation `dep` and `target` can be defined, although additional dependencies or targets may be defined in addition by using the `deps` array instead, for example:

```toml
version = 0.1

[[task]]
name = 'npm:install'
run = 'npm install'

[[task]]
name = 'build:babel'
target = 'lib/#.js'
deps = ['src/#.js', 'npm:install']
run = 'babel $DEP -p $TARGET --source-maps'
```
_<div style="text-align: center">`$DEP` and `$TARGET` will always be the interpolation dependency and target. Additional dependencies and targets can always be defined.</div>_

### Test Pattern

While Chomp is not designed to be a test runner, it can easily provide many the features of one.

Tests can be run with interpolation. Since interpolation expands a glob of dependencies to operate on, this same technique can be used to create targetless tests:

```toml
version = 0.1

[[task]]
name = 'test:unit:#'
dep = ['test/unit/#.js', 'dist/build.js']
run = 'node $DEP'
```
_<div style="text-align: center">Task interpolation without a target runs the task over all dependencies, and is always invalidated, exactly what is needed for a test runner.</div>_

In the above, all files `test/unit/**/*.js` will be expanded by the `test:unit` test resulting in a separate task run for each file. Since no `targets` are defined, the task is always invalidated and re-run.

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

Using dependencies and targets, task graphs are built up through the task pattern in Chomp, where each task can be cached at a fine-grained level. Task dependency inputs can themselves be the result of targets or other tasks. Build order is determined by the graph in this way.

For example, consider a build that compiles with Babel, then builds into a single file with RollupJS.

Rather than using a RollupJS Babel plugin, separating the compilation on the file system enables caching, parallelization, finer-grained generic build control and comprehensive incremental builds with watcher support:

```toml
version = 0.1

[[task]]
name = 'npm:install'
run = 'npm install'
target = 'package-lock.json'
deps = ['package.json']

[[task]]
name = 'build:babel'
target = 'lib/#.js'
deps = ['src/#.js', 'npm:install']
run = 'babel $DEP -p $TARGET --source-maps'

[[task]]
name = 'build:rollup'
deps = 'lib/**/*.js'
target = 'dist/app.js'
run = 'rollup lib/app.js -d dist -m'
```
_<div style="text-align: center">Practical example of a Chomp build graph using task dependence from npm install to per-module Babel compilation to Rollup into a single file or set of files.</div>_

Following the task graph from the lowest level to the highest level:

* `npm:install`: If the `package.json` was modified after the `package-lock.json`, `npm install` is run to ensure the installation is up-to-date.
* `build:babel`: By depending on `npm:install` the previous task is first validated and possibly run to initiate an npm install. Then if for a given source file `src/file.js`, `lib/file.js` does not exist, or the mtime on `src/file.js` was modified after `lib/file.js`. With the task graph, modifying `src/file.js` or `package-lock.json` both cause invalidations resulting in a rebuild.
* `build:rollup`: Since `build:rollup` depends on all deps in `lib`, this will make it depend on all the separate file builds of `build:babel`, and in turn their dependence. If any file `lib/file.js` has an mtime greater than the main build target file `dist/app.js` then the RollupJS build is retriggered, but not that only the Babel compilations needed are rebuilt in this process.

When running `chomp rollup --watch` we now get fine-grained incremental build watching, with the watched file invalidations exactly as we defined with the task dependence graph rules above.

Finally, `chomp rollup --serve` will provide a local static server along with the task watcher for the task. A websocket protocol for in-browser hot-reloading is a [planned future addition](https://github.com/guybedford/chomp/issues/61).

Replacing monolithic JS build systems with make-style file caching all the commonly expected features of JS dev workflows can still be maintained.

### Task Invalidation Rules

The default task invalidation is based on the mtime rules per the example above. The invalidation rule is a binary rule indicating whether or not a given task should rerun or be treated as cached.

The explicit rules of invalidation for this `mtime` invalidation are:

* If no targets are defined for a task, it is always invalidated.
* Otherwise, if no deps are defined for a task, it is invalidated only if the targets do not exist.
* Otherwise, if the mtime of any dep is greater than the mtime of any target, the task is invalidated.

Task invalidation can be customized with the `invalidation` property on a task:

* `invalidation = 'mtime'` (default): This is the default invalidation, as per the rules described above.
* `invalidation = 'always'`: The task is always invalidated and rerun, without exception.
* `invalidation = 'not-found'`: The task is only invalidated when not all targets are defined.

### Task Parallelization

By default all tasks in Chomp are run with full parallelism, which can also be controlled by the [`-j` flag](cli.md#jobs) to choose the maximum number of child processes to spawn.

In addition, task pooling can also be controlled by [extension batching operations](extensions.md#chompregisterbatchername-string-batcher-batch-cmdop-running-batchcmd--batcherresult--undefined).

Dependencies of tasks are always treated as being parallel - to ensure one task always happens before another the best way is usually to treat it as a dependency. For example, the test depends on the build target.

#### Serial Dependencies

In some cases, it can be preferred to write a serial pipeline of steps that should be followed.

This can be achieved by setting `serial = true` on the task:

chompfile.toml
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

## Loading Extensions

Extensions allow encapsulating complex Chompfile configurations.

For example, by encapsulating the Babel and RollupJS compilations as task templates, the main Chompfile
can be simplified to just include the parameters and not the details of task execution.

To make things simpler - Chomp already includes a default extensions library, [Chomp Templates](https://github.com/guybedford/chomp-templates), for these tasks.

Extensions are loaded via the `extensions` list in the Chompfile:

chompfile.toml
```toml
version = 0.1

extensions = ['chomp:npm', 'chomp:babel', 'chomp:rollup']

[[task]]
name = 'npm:install'
template = 'npm'

[[task]]
name = 'build:babel'
template = 'babel'
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
_<div style="text-align: center">Using the `chomp:npm`, `chomp:babel` and `chomp:rollup` template extensions allows writing these tasks fully encapsulating their implementations.</div>_

## Writing Extensions

> Read more on writing templates in the [extensions documentation](extensions.md)

Chomp extensions can be loaded from any URL or local file path (`chomp:[x]` is just a shorthand for `https://ga.jspm.io/npm:@chompbuild/templates@latest/[x].js`).

To write custom templates, create a local extension file `local-extension.js` referencing it in the extensions list of the Chompfile:

```toml
version = 0.1

extensions = ['./local-extension.js']

[[task]]
name = 'npm:install'
template = 'npm'

[[task]]
name = 'build:babel'
template = 'babel'
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
_<div style="text-align: center">Example of defining the `npm`, `babel` and `rollup` templates by loading a local extension at `./local-exdtension.js`.</div>_

Here's a simplified example of creating the `npm`, `babel`, and `rollup` templates:

local-extension.js
```js
Chomp.registerTemplate('npm', function (task) {
  return [{
    name: task.name,
    run: 'npm install'
    target: 'package-lock.json'
    deps: ['package.json']
  }];
});

Chomp.registerTemplate('babel', function (task) {
  const { sourceMaps } = task.templateOptions;
  return [{
    name: task.name,
    target: task.target,
    deps: task.deps,
    run: `babel $DEP -o $TARGET${sourceMaps ? ' --source-maps' : ''}`
  }];
});

Chomp.registerTemplate('rollup', function (task) {
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
_<div style="text-align: center">Chomp extension template registration example for the `npm`, `babel` and `rollup` templates.</div>_

Templates are functions on tasks returning a new list of tasks. All TOML properties apply but with camelCase instead of kebab-case.

Templates can be loaded from any file path or URL. PRs to the default Chomp templates library are welcome, or host your own on your own domain or via an npm CDN. For support on the JSPM CDN, add `"type": "script"` to the package.json since template extensions are currently scripts and not modules.

Remote extensions are loaded once and cached locally by Chomp, regardless of cache headers, to ensure the fastest run time. For this reason it is recommended to always use unique URLs with versions when hosting extensions remotely. The remote extension cache can also be cleared by running `chomp --clear-cache`.

And if it ever feels a little too magical, templates can also be ejected by running `chomp --eject`, transforming the Chompfile into the expanded untemplated form without extensions.
