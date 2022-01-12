# CHOMP

> "JS Make"

## Install

```
cargo install chomp-build
```

## Getting Started

### Hello World

`chomp` works against a `chompfile.toml` in the same directory as the `chomp` command is run.

For example:

chompfile.toml
```toml
version = 0.1

[[task]]
  name = "hello:world"
  target = "hello-world.txt"
  run = """
    echo "Hello World" > hello-world.txt
  """
```

with this file saved, running:

```sh
chomp hello:world
```

will populate the `hello-world.txt` file.

Subsequent runs, will see that the target is defined, and skip running the command again.

Array `deps` can be defined for targets, whose targets will then be run first with invalidation based on target / deps mtime comparisons per the standard Makefile approach.

In Windows, Powershell is used and Bash on posix systems. Since both `echo` and `>` are defined on both systems the above works cross-platform (Powershell is automatically put into UTF-8 mode for `>` to work similarly).

### Templates

Templates provide the ability to construct tasks programatically with embedded JS inside of chompfiles.

A number of system templates are provided out of the box in the templates directory of this repo. Template PRs can be made to add new default support to Chomp.

#### npm install

For example, to install an npm library, rather than manually writing an `npm install` call, you can use the `npm` template:

```chompfile.toml
version = 0.1

[[task]]
  name = "Install Mocha"
  template = "npm"
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

[[task]]
  name = "typescript"
  template = "swc"
  target = "lib/#.js"
  deps = ["src/#.ts]
  # Installs SWC automatically if needed
  [task.options]
    auto-install = true
```

In the above, all `src/**/*.ts` files will be globbed, have SWC run on them, and output into `lib/[file].js` along with their source maps.

Only files not existing or whose `src` mtimes are invalidated (or SWC itself is updated) will be rebuilt.

# License

MIT
