# CLI Flags

```
Chomp 0.1.0

USAGE:
    chomp [FLAGS] [OPTIONS] [TARGET]... [-- <ARGS>...]

FLAGS:
    -C, --clear-cache    Clear URL extension cache
        --eject          Ejects templates into tasks saving the rewritten chompfile.toml
    -f, --force          Force rebuild targets
    -F, --format         Format and save the chompfile.toml
    -h, --help           Prints help information
    -l, --list           List the available chompfile tasks
    -s, --serve          Run a local dev server
    -V, --version        Prints version information
    -w, --watch          Watch the input files for changes

OPTIONS:
    -c, --config <CONFIG>              Custom chompfile path [default: chompfile.toml]
    -j, --jobs <N>                     Maximum number of jobs to run in parallel
    -p, --port <PORT>                  Custom port to serve
    -r, --server-root <server-root>    Server root path

ARGS:
    <TARGET>...    Generate a target or list of targets
    <ARGS>...      Custom task args
```
