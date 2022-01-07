# CHOMP

> "JS Make"

## Install

```
cargo install chomp-build
```

## Hello World

`chomp` works against a `chompfile.toml` in the same directory as the `chomp` command is run.

For example:

chompfile.toml
```toml
version = 0.1

[[task]]
  name = "hello:world"
  run = """
    echo "HELLO WORLD"
  """
```

with this file saved, running:

```sh
chomp hello:world
```

will output:

```
○ :hello:world
HELLO WORLD
```

# License

MIT
