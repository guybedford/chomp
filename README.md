# CHOMP

[![Crates.io](https://img.shields.io/badge/crates.io-chompbuild-green.svg)](https://crates.io/crates/chompbuild)
[![Discord](https://img.shields.io/badge/chat-on%20disord-green.svg?logo=discord)](https://discord.gg/5E9zrhguTy)


Chomp is frontend task runner with advance featured and a focus on _ease-of-use_ and _**not** 'getting in the way'_!

## Why is Chomp so easy to use?

Chomps setups itself up with a single commmand!

```bash
chomp --init --import-scripts
```

Now you can run any task in your project's npm scripts using Chomp!

- _Where you ran `npm run <task>` before, now run `chomp <task>`!_

The only difference is, with Chomp—it's faster. And, with a few more tweaks, you get smart caching, parallelism, and more!

## What features does Chomp provide?

Chomp is an advanced task runner. It provides features similar to [turbo](https://turbo.build/repo) and [nx](https://nx.dev/) but focuses on ease of use; *not monorepos.

### Parallelism
  - Chomp [runs tasks in parallel](./docs/task.md#serial-dependencies), based on an extecuted task's dependencies!
### Watch/Serve
  - Chomp [watches any task](./docs/task.md#watched-rebuilds) by including a `--watch` or `--serve` option! Read more about the power of [`--watch`](./docs/task.md#watched-rebuilds) and [`--serve`](./docs/task.md#static-server).
### A JS extension system
  - Chomp has a [JS extension system](./docs/extensions.md) that allows you to extend Chomp with your own custom tasks
### Smart caching
  - Chomp [caches tasks](./docs/task.md#task-caching) based on task dependencies like other tasks or updated files. You don't have to worry about it!

> \*Chomp works for monrepos but it's achitectured for ease of use and not getting in the way first.
## Why is Chomp right for me?

1. You get an advanced task runner with a single command!
1. You enable and manage advanced features with single line updates!
1. You don't have to re-invent your project.

Chomp a great option for frontend projects where the goal is getting common advance task runner features, like smart caching, without the complexity and overhead!

Additionally, you get these features with 0 coding pattern changes. Chomp builds off of a project's established tasks without breaking them. **_AKA, you can still run `npm run <task>`; you just don't Chomp's rich feature set!_**

## Install

If you use [Cargo](https://rustup.rs/), run:

```
cargo install chompbuild
```

If you don't use Cargo, run:

```
npm install -g chomp
```

> Note: npm scripts add over 100ms to the script run time.

Common platform binaries are also available for [all releases](https://github.com/guybedford/chomp/releases).

To quickly setup Chomp in a GitHub Actions CI workflow, see the [Chomp GitHub Action](https://github.com/guybedford/chomp-action).

## Documentation

* [CLI Usage](https://github.com/guybedford/chomp/blob/main/docs/cli.md)
* [Chompfile Definitions](https://github.com/guybedford/chomp/blob/main/docs/chompfile.md)
* [Task Definitions](https://github.com/guybedford/chomp/blob/main/docs/task.md)
* [Extensions](https://github.com/guybedford/chomp/blob/main/docs/extensions.md)

## Getting Started

### Migrating from npm Scripts

To convert an existing project using npm `"scripts"` to Chomp, run:

```sh
$ chomp --init --import-scripts
√ chompfile.toml created with 2 package.json script tasks imported.
```

or the shorter version:

```sh
$ chomp -Ii
√ chompfile.toml created with 2 package.json script tasks imported.
```

Then use `chomp <name>` instead of `npm run <name>`, and enjoy the new features of task dependence, incremental builds and parallelism!

### Hello World

`chomp` works against a [`chompfile.toml`](https://github.com/guybedford/chomp/blob/main/docs/chompfile.md) [TOML configuration](https://toml.io/) in the same directory as the `chomp` command is run.

Chomp builds up tasks as trees of files which depend on other files, then runs those tasks with [maximum parallelism](https://github.com/guybedford/chomp/blob/main/docs/task.md#task-dependence).

For example, here's a task called `hello` which builds `hello.txt` based on the contents of `name.txt`, which itself is built by another command:

chompfile.toml
```toml
version = 0.1

[[task]]
target = 'name.txt'
run = '''
  echo "No name.txt, writing one."
  echo "World" > name.txt
'''

[[task]]
name = 'hello'
target = 'hello.txt'
dep = 'name.txt'
run = '''
  echo "Hello $(cat name.txt)" > hello.txt
'''
```

with this file saved, the hello command will run all dependency commands before executing its own command:

```sh
$ chomp hello

🞂 name.txt
No name.txt, writing one.
√ name.txt [4.4739ms]
🞂 hello.txt
√ hello.txt [5.8352ms]

$ cat hello.txt
Hello World
```

Finally it populates the `hello.txt` file with the combined output.

Subsequent runs use the mtime of the target files to determine what needs to be rerun.

Rerunning the `hello` command will see that the `hello.txt` target is defined, and that the `name.txt` dependency didn't change, so it will skip running the command again:

```sh
chomp hello

● name.txt [cached]
● hello.txt [cached]
```

Changing the contents of `name.txt` will then invalidate the `hello.txt` target only, not rerunning the `name.txt` command:

```sh
$ echo "Chomp" > name.txt
$ chomp hello

● name.txt [cached]
  hello.txt invalidated by name.txt
🞂 hello.txt
√ hello.txt [5.7243ms]

$ cat hello.txt
Hello Chomp
```

Array [`deps`](https://github.com/guybedford/chomp/blob/main/docs/task.md#task-dependence) can be defined for targets, whose targets will then be run first with [invalidation based on target / deps mtime comparisons](https://github.com/guybedford/chomp/blob/main/docs/task.md#task-caching) per the standard Makefile approach.

In Windows, Powershell is used and Bash on posix systems. Since both `echo` and `>` are defined on both systems the above works cross-platform (Powershell is automatically put into UTF-8 mode for `>` to work similarly).

Note that `&&` and `||` are not supported in Powershell, so multiline scripts and `;` are preferred instead.

#### JS Tasks

Alternatively we can use `engine = 'node'` or `engine = 'deno'` to write JavaScript in the `run` function instead:

chompfile.toml
```toml
version = 0.1

[[task]]
target = 'name.txt'
engine = 'node'
run = '''
  import { writeFile } from 'fs/promises';
  console.log("No name.txt, writing one.");
  await writeFile(process.env.TARGET, 'World');
'''

[[task]]
name = 'hello'
target = 'hello.txt'
deps = ['name.txt']
engine = 'node'
run = '''
  import { readFile, writeFile } from 'fs/promises';
  const name = (await readFile(process.env.DEP, 'utf8')).trim();
  await writeFile(process.env.TARGET, `Hello ${name}`);
'''
```

Tasks are run with full parallelism permitted by the task graph, which can be controlled via the [`-j` flag](https://github.com/guybedford/chomp/blob/main/docs/cli.md#jobs) to limit the number of simultaneous executions.

Using the [`--watch` flag](https://github.com/guybedford/chomp/blob/main/docs/cli.md#watch) watches all dependencies and applies incremental rebuilds over invalidations only.

Or using `chomp hello --serve` runs a [static file server](https://github.com/guybedford/chomp/blob/main/docs/task.md#static-server) with watched rebuilds.

See the [task documentation](https://github.com/guybedford/chomp/blob/main/docs/task.md) for further details.

#### Monorepos

There is no first-class monorepo support, but some simple techniques can achieve the use cases.

For example, consider a monorepo where `packages/[pkgname]/chompfile.toml` defines per-package tasks.

A base-level `chompfile.toml` could run the `test` task of all the sub-packages with the following `chompfile.toml`:

```toml
[[task]]
name = 'test'
dep = 'packages/#/chompfile.toml'
run = 'chomp -c $DEP test'
```

`chomp test` will then use [task interpolation](https://github.com/guybedford/chomp/blob/main/docs/task.md#task-interpolation) to run the multiple sub-package test tasks in parallel. A similar approach can also be used for a [basic unit testing](https://github.com/guybedford/chomp/blob/main/docs/task.md#testing).

Adding [`serial = 'true'`](https://github.com/guybedford/chomp/blob/main/docs/task.md#serial-dependencies) the interpolation can be made to run in series rather than in parallel.

Cross-project dependencies are [not currently supported](https://github.com/guybedford/chomp/issues/119). Instead if `packages/a/chompfile.toml`'s build task depends on `packages/b/chompfile.toml`'s build task to run first, then `packages/a/chompfile.toml` might look like:

```toml
[[task]]
name = 'build'
run = 'cargo build'
dep = 'build:deps'

[[task]]
name = 'build:deps'
run = 'chomp -c ../a build'
```

This would still be fast so long as `packages/a/chompfile.toml`'s `build` task has its targets and dependencies properly configured to do zero work if the all target mtimes are greater than their dependencies.

### Extensions

Extensions are able to register task templates for use in Chompfiles.

Extensions are loaded using the `extensions` list, which can be any local or remote JS file:

```toml
version = 0.1
extensions = [
  "./local.js",
  "https://remote.com/extension.js"
]
```

A core extensions library is provided with useful templates for the JS ecosystem, with
the short protocol `chomp:ext`, a shorthand for the `@chompbuild/extensions` package contents.

A simple example is included below.

_See the [@chompbuild/extensions package](https://github.com/guybedford/chomp-extensions) for extension descriptions and examples._

#### Example: TypeScript with SWC

To compile TypeScript with the SWC template:

```toml
version = 0.1
extensions = ['chomp@0.1:swc']

[[task]]
name = 'build:typescript'
template = 'swc'
target = 'lib/##.js'
deps = ['src/##.ts']
```

In the above, all `src/**/*.ts` files will be globbed, have SWC run on them, and output into `lib/[file].js` along with their source maps.

The `##` and `#` interpolation syntax are special because unlike glob dependencies (which are also supported), they must be a 1-1 relation from dependency to target.

Only files not existing or whose `src` mtimes are invalidated (or SWC itself is updated) will be rebuilt.

Specific files or patterns can be built directly by name as well, skipping all other build work:

```sh
chomp lib/main.js lib/dep.js

🞂 lib/dep.js
🞂 lib/app.js
√ lib/dep.js [317.2838ms]
√ lib/app.js [310.0831ms]
```

Patterns are also supported for building tasks by name or filename (the below two commands are equivalent):

```sh
$ chomp lib/*.js
$ chomp :build:*
```

To remove the template magic, run `chomp --eject` to convert the `chompfile.toml` into its untemplated form:

```sh
$ chomp --eject

√ chompfile.toml template tasks ejected
```

Resulting in the updated _chompfile.toml_:

```toml
version = 0.1

[[task]]
name = 'build:typescript'
target = 'lib/##.js'
dep = 'src/##.ts'
stdio = 'stderr-only'
run = 'node ./node_modules/@swc/cli/bin/swc.js $DEP -o $TARGET --no-swcrc --source-maps -C jsc.parser.syntax=typescript -C jsc.parser.importAssertions=true -C jsc.parser.topLevelAwait=true -C jsc.parser.importMeta=true -C jsc.parser.privateMethod=true -C jsc.parser.dynamicImport=true -C jsc.target=es2016 -C jsc.experimental.keepImportAssertions=true'
```

# License

Apache-2.0
