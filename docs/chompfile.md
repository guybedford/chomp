# Chompfile

Chomp projects are defined by a `chompfile.toml`, with Chompfiles defined using the [TOML configuration format](https://toml.io/).

The default Chompfile is `chompfile.toml`, located in the same directory as the `chomp` binary is being run from.

Custom configuration can be used via `chomp -c custom.toml` or `chomp -c ./nested/chompfile.toml`.

All paths within a Chompfile are relative to the Chompfile itself regardless of the invocation CWD.

## Example

To create a new Chomp project, create a new file called `chompfile.toml` and add the following lines:

chompfile.toml
```toml
version = 0.1

[[task]]
name = 'build'
run = 'echo "Chomp Chomp"'
```

In the command line, type `chomp build` or just `chomp` (_"build"_ is the default task when none is given):

```sh
$ chomp

🞂 :build
Chomp Chomp
√ :build [6.3661ms]
```

to get the runner output.

Every Chompfile must start with the `version = 0.1` version number, at least until the project stabilizes.

See the [task documentation](tasks.md) for defining tasks.

## Chompfile Definitions

The Chompfile supports the following definitions:

chompfile.toml
```toml
# Every Chompfile must start with the Chompfile version, currently 0.1
version = 0.1

# The default task name to run when `chomp` is run without any CLI arguments
default_task = "test"

# List of Chomp Extensions to load
extensions = ["extension-path"]

# Environment variables for all runs
[env]
ENV_VAR = "value"

# Default environment variables to only set if not already for all runs
[env-default]
DEFAULT_VAR = "value"

# Static server options for `chomp --serve`
[server]
# Static server root path, relative to the Chomp file
root = "public"
# Static server port
port = 1010

# Default template options by registered template name
# When multiple tasks use the same template, this avoids duplicated `[template-options]` at the task level
[template-options.<template name>]
key = value

# Task definitions
# Tasks are a TOML list of Task objects, which define the task graph
[[task]]
name = "TASK"
run = "shell command"
```

See the [task documentation](task.md) for defining tasks, and the [extension documentation](extensions.md) for defining Chompfile extensions.
