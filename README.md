# CHOMP

> 'JS Make'

## Install

```
cargo install chompbuild
```

## Getting Started

### Hello World

`chomp` works against a `chompfile.toml` in the same directory as the `chomp` command is run.

For example:

chompfile.toml
```toml
version = 0.1

[[task]]
name = 'hello:world'
target = 'hello-world.txt'
run = '''
  echo "Hello World" > hello-world.txt
'''
```

with this file saved, running:

```sh
chomp hello:world

🞂 hello-world.txt
√ hello-world.txt [3.8352ms]
```

will populate the `hello-world.txt` file.

Subsequent runs, will see that the target is defined, and skip running the command again:

```sh
chomp hello:world

● hello-world.txt [cached]
```

Array `deps` can be defined for targets, whose targets will then be run first with invalidation based on target / deps mtime comparisons per the standard Makefile approach.

In Windows, Powershell is used and Bash on posix systems. Since both `echo` and `>` are defined on both systems the above works cross-platform (Powershell is automatically put into UTF-8 mode for `>` to work similarly).

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

Some short examples of extension templates are provided below. See the [@chompbuild/extensions package](https://github.com/guybedford/chomp-extensions) for extension descriptions and examples.

#### npm install

For example, to install an npm library, rather than manually writing an `npm install` call, you can use the `npm` template:

```chompfile.toml
version = 0.1
extensions = ['chomp:npm']

[[task]]
name = 'Install Mocha'
template = 'npm'

[task.options]
auto-install = true
packages = ['mocha']
dev = true
```

The template includes conveniences to skip the install if the package is already present, and also ensure a package.json file is initialized if it does not exist.

#### TypeScript with SWC

To compile TypeScript with the SWC template:

```toml
version = 0.1
extensions = ['chomp:swc']

[[task]]
name = 'typescript'
template = 'swc'
target = 'lib/#.js'
deps = ['src/#.ts']

# Installs SWC automatically if needed
[task.options]
auto-install = true
```

In the above, all `src/**/*.ts` files will be globbed, have SWC run on them, and output into `lib/[file].js` along with their source maps.

Only files not existing or whose `src` mtimes are invalidated (or SWC itself is updated) will be rebuilt.

Specific files only can be build directly by name as well, skipping all other build work:

```sh
chomp lib/main.js lib/dep.js

🞂 lib/dep.js
🞂 lib/app.js
Successfully compiled 2 files with swc.
√ lib/dep.js [317.2838ms]
√ lib/app.js [310.0831ms]
```

Automatic batching of builds into SWC commands is also handled by the extenion via the batching hook.

# License

GPLv3

For licensing discussion, see https://github.com/guybedford/chomp/issues/62.