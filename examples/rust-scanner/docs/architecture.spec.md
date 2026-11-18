# The scanner

## What the scanner refuses {#refuses selects="concept:project-scan kind:ensures,requires"}

`parse_config` is given exactly one directory and never searches for another. It refuses a
directory that holds no `config.json`, and the message names the directory it refused.
