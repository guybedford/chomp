# CHOMP

> 'JS Make' - parallel task runner for the frontend ecosystem with a JS extension system.

## Install

Make sure you have [Cargo installed](https://rustup.rs/), then run:

```
cargo install chompbuild
```

## Documentation

* [CLI Usage](https://github.com/guybedford/chomp/blob/main/docs/cli.md)
* [Chompfile Definitions](https://github.com/guybedford/chomp/blob/main/docs/chompfile.md)
* [Task Definitions](https://github.com/guybedford/chomp/blob/main/docs/task.md)
* [Extensions](https://github.com/guybedford/chomp/blob/main/docs/extensions.md)

## Getting Started

### Hello World

`chomp` works against a `chompfile.toml` in the same directory as the `chomp` command is run.

For example:

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

with this file saved, running:

```sh
$ chomp hello

🞂 name.txt
No name.txt, writing one.
√ name.txt [4.4739ms]
🞂 hello.txt
√ hello.txt [5.8352ms]
```

will populate the `hello.txt` file.

Subsequent runs, will see that the target is defined, and skip running the command again:

```sh
chomp hello

● name.txt [cached]
● hello.txt [cached]
```

Changing `name.txt` to use a different name will invalidate the `hello.txt` target only:

```sh
$ echo "Chomp" > name.txt
$ chomp hello

● name.txt [cached]
  hello.txt invalidated by name.txt
🞂 hello.txt
√ hello.txt [5.7243ms]
```

Array `deps` can be defined for targets, whose targets will then be run first with invalidation based on target / deps mtime comparisons per the standard Makefile approach.

In Windows, Powershell is used and Bash on posix systems. Since both `echo` and `>` are defined on both systems the above works cross-platform (Powershell is automatically put into UTF-8 mode for `>` to work similarly).

Alternatively use `engine = 'node'` or `engine = 'deno'` to write JavaScript in the `run` function instead:

chompfile.toml
```toml
version = 0.1

[[task]]
target = 'name.txt'
engine = 'node'
run = '''
  import { writeFileSync } from 'fs';
  console.log("No name.txt, writing one.");
  writeFileSync(process.env.TARGET, 'World');
'''

[[task]]
name = 'hello'
target = 'hello.txt'
deps = ['name.txt']
engine = 'node'
run = '''
  import { readFileSync, writeFileSync } from 'fs';
  const name = readFileSync(process.env.DEP, 'utf8').trim();
  writeFileSync(process.env.TARGET, `Hello ${name}`);
'''
```

Tasks are run with full parallelism permitted by the task graph, which can be controlled via the `-j` flag to limit the number of simultaneous executions.

Using the `--watch` flag watches all dependencies and applies incremental rebuilds over invalidations only.

Or using `chomp hello --serve` runs a static file server with watched rebuilds.

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

# Automatically install SWC if not present
[template-options.npm]
auto-install = true

[[task]]
name = 'build:typescript'
template = 'swc'
target = 'lib/#.js'
deps = ['src/#.ts']
```

In the above, all `src/**/*.ts` files will be globbed, have SWC run on them, and output into `lib/[file].js` along with their source maps.

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

# License

GPLv3

For licensing discussion, see https://github.com/guybedford/chomp/issues/62.
