# CLI Flags

Usage:

```
chomp [FLAGS/OPTIONS] <TARGET>...
```

Chomp takes the following arguments and flags:

* [`<TARGET>...`](#target): List of targets to build
* [`-C, --clear-cache`](#clear-cache): Clear URL extension cache
* [`-c, --config`](#config): Custom chompfile project or path [default: chompfile.toml]
* [`--eject`](#eject): Ejects templates into tasks saving the rewritten chompfile.toml
* [`-f, --force`](#force): Force rebuild targets
* [`-F, --format`](#format): Format and save the chompfile.toml
* [`-h, --help`](#help): Prints help information
* [`-I, --import-scripts`](#import-scripts): Import npm package.json "scripts" into the chompfile.toml
* [`-i, --init`](#init): Initialize the chompfile.toml if it does not exist
* [`-j, --jobs`](#jobs): Maximum number of jobs to run in parallel
* [`-l, --list`](#list): List the available chompfile tasks
* [`-p, --port`](#port): Custom port to serve
* [`-s, --serve`](#serve): Run a local dev server
* [`-r, --server-root`](#server-root): Server root path
* [`-V, --version`](#version): Prints version information
* [`-w, --watch`](#watch): Watch the input files for changes

## Target

The main arguments of the `chomp` command are a list of targets to build.

Build targets can be task names, file paths relative to the `chompfile.toml`, or glob patterns of task names or file paths to build.

To disambiguate task names from file paths, task names can always be referenced with a `:` prefix - `chomp :test` instead of `chomp test`.

Only the necessary work to produce the provided targets will be performed, taking into account [task dependence](task.md#task-dependence).

When no target is provided, the `default-task` defined in the Chompfile is run, if set.

## Clear Cache

When loading Chomp extensions from external URLs via the [`extensions` configuration](task.md#loading-extensions),
remote extensions are cached in the user-local `[cachedir]/.chomp/` folder.

Extensions are cached permanently regardless of cache headers to optimize for task run execution time.

Run `chomp --clear-cache` to clear these caches.

Where possible, use unique versioned URLs for remote extensions.

## Config

Usually Chomp will look for `chompfile.toml` within the current working directory.

Running `chomp -c ./path/to/chompfile.toml` allows running Chomp on a folder that is not the current working directory,
or running Chomp against a Chompfile with another name than `chompfile.toml`.

## Force

When running a task, the default [invalidation rules](task.md#task-invalidation-rules) of that [task dependence graph](task.md#task-dependence) will apply.

To treat all tasks in the target graph as invalidated, the `chomp -f task` flag can be useful to ensure everything is fresh.

## Format

`chomp --format` will apply the default serialization formatting to the `chompfile.toml` file.

Note this command will overwrite the existing `chompfile.toml` with the new formatting.

This command is compatible with the [`--config`](#config) flag to choosing the Chompfile to operate on.

Due to limitations with the Rust TOML implementation, comments are currently stripped by this operation.

## Help

CLI help is available via `chomp -h`.

## Jobs

Sets the maximum number of task runs to spawn in parallel. Defaults to the logical CPU count.

By default tasks in Chomp are run with [maximum parallelization](task.md#task-parallelization).

## List

`chomp --list` will output a listing of the named tasks of the current `chompfile.toml` or Chompfile specified by [`--config`](#config).

## Port

When using [`chomp --serve`](#serve) to run a local static server, customizes the static server port. Defaults to `8080`.

## Serve

Enables the file watcher, and runs a static server with the optionally [`--port`](#port) and [`--server-root`](#server-root), which are also customizable in the [Chompfile](chompfile.md).

When serving, a list of [task targets](#target) is still taken to watch.

## Server Root

When using [`chomp --serve`](#serve) to run a local static server, customizes the site root to serve. Defaults to the same folder as the Chompfile.

## Version

The current Chomp version is available via `chomp --version`

## Watch

The `--watch` flag instructs Chomp to continue running after completing the tasks, and listen to any changes to all files that were touched by the [task dependency graph](task.md#task-dependence).

A [list of targets](#target) is supplied like any other Chomp run, which then informs which files are watched.
