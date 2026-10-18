# hdf

A fuzzy finder for bash history. Pick a command to put it on the prompt line, or delete it from
the history, every occurrence of it.

## Install

Build and install the binary into `~/.cargo/bin`:

```bash
cargo install --path .
```

Load the key bindings in `~/.bashrc`. Copy `home/.hdfrc` anywhere and source it:

```bash
[[ -f ~/.hdfrc ]] && . ~/.hdfrc
```

Optionally, install the bash completion of the options. With bash-completion it loads on demand
from:

```bash
cp completions/_hdf ~/.local/share/bash-completion/completions/hdf
```

Without bash-completion, source `completions/_hdf` in `~/.bashrc` instead.

## Usage

Page Up or Page Down opens hdf with the typed text as the query. Enter puts the chosen command on
the prompt line, Esc puts back the edited query.

With bash 4.4 or later hdf shows your prompt in place of `>`.

| Key                         | Action                                      |
| --------------------------- | ------------------------------------------- |
| Up, Down, PgUp, PgDn        | move the selection (also Ctrl-P, Ctrl-N)    |
| Enter, Tab                  | pick the selected command                   |
| Ctrl-D                      | delete the selected command from history    |
| Left, Right, Home, End      | move the cursor (also Ctrl-A, Ctrl-E)       |
| Alt-B, Alt-F, Ctrl-Arrows   | move the cursor by word                     |
| Backspace, Delete           | delete a character                          |
| Alt-Backspace, Ctrl-W       | delete a word                               |
| Ctrl-U                      | delete to the start of the query            |
| Esc                         | quit, keeping the query                     |
| Ctrl-C                      | quit, leaving the prompt line as it was     |

`hdf --delete-last [N]` deletes the last N commands from history, 1 by default, and prints them.

`hdf --help` lists all options and exit statuses.

## Config

`hdf --init-config` writes the defaults to `~/.config/hdf/config.toml` (or under
`$XDG_CONFIG_HOME`). Keys left out keep their defaults:

```toml
[colors]
prompt   = { fg = "blue", bold = true }
count    = "dark-gray"     # matches/commands
deleted  = "green"         # the status after Ctrl-D
error    = "red"           # Ctrl-D failed
match    = { fg = "yellow", bold = true }
dim      = "dark-gray"     # times, run counts, cuts, the divider
selected = { bg = "238" }  # the selected row
```

A color is a name (`yellow`, `light-blue`, `dark-gray`), `0` to `255` or `#rrggbb`, or a table of
`fg`, `bg`, `bold`, `italic`, `underlined` and `reversed`.

## Development

`rake` runs rubocop, `cargo test`, clippy and the Ruby tests, which drive hdf in a pseudo terminal.
