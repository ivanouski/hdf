# hdf

Searches bash history and prints the chosen command. Commands can be deleted from the history on
the way, every occurrence of them.

## Setup

`install.rb` at the root of the repo installs hdf with `cargo install --path hdf`, into
`~/.cargo/bin`, stows `.hdfrc` to `~/.hdfrc` and the bash completion of its options to
`~/.completions/_hdf`. Load it in `.bashrc`:

```bash
[[ -f ~/.hdfrc ]] && . ~/.hdfrc
```

Page Up and Page Down then open hdf with the typed text as the query. Enter and Tab put the chosen
command on the prompt line, Esc puts back the query as edited.

With bash 4.4 or later `.hdfrc` passes the shell prompt, `--prompt "${PS1@P}"`, so hdf shows it in
place of `>`, in its colors, and lists the commands under the query, the times before them. A
prompt narrower than the times or wider than 60% of the terminal keeps the usual indent.

## Keys

| Key                      | Action                                                     |
| ------------------------ | ---------------------------------------------------------- |
| Up, Down, Ctrl-P, Ctrl-N | move the selection                                         |
| PgUp, PgDn               | move the selection, like Up and Down                       |
| Enter, Tab               | print the selected command; without matches, quit like Esc |
| Ctrl-D                   | delete the selected command, every occurrence of it        |
| Left, Right, Home, End   | move the cursor in the query                               |
| Ctrl-A, Ctrl-E           | move the cursor to the start, the end                      |
| Alt-B, Alt-F             | move the cursor a word back, forward                       |
| Alt/Ctrl-Left, -Right    | move the cursor a word back, forward                       |
| Backspace, Delete        | delete the character before, at the cursor                 |
| Alt-Backspace            | delete the word before the cursor                          |
| Ctrl-W                   | delete back to whitespace                                  |
| Ctrl-U                   | delete the query before the cursor                         |
| Esc                      | quit                                                       |
| Ctrl-C                   | quit, leaving the prompt line as it was                    |

## Deleting the last commands

`hdf --delete-last` deletes the last command from the history file, `hdf --delete-last 10` the last
10. It prints the deleted commands.

`hdf --help` lists the options and exit statuses.

## Config

hdf reads `$XDG_CONFIG_HOME/hdf/config.toml`, or `~/.config/hdf/config.toml`. For now it holds the
colors. Every key is optional, the ones left out keep their defaults.

`hdf --init-config` writes the defaults below to that file and prints its path. It leaves a
config that is already there alone.

Each key is a color for the text, or a table of `fg`, `bg`, `bold`, `italic`, `underlined` and
`reversed`. Colors are names like `yellow`, `light-blue` and `dark-gray`, `0` to `255`, or
`#rrggbb`.

The defaults:

```toml
[colors]
prompt   = { fg = "blue", bold = true }
count    = "dark-gray"     # matches/commands
deleted  = "green"         # the status after Ctrl-D
error    = "red"           # Ctrl-D failed
match    = { fg = "yellow", bold = true }
dim      = "dark-gray"     # times and numbers of runs
selected = { bg = "238" }  # the selected row, under the match
```

An example for a light terminal theme, with a light selection and a darker match:

```toml
[colors]
selected = { fg = "black", bg = "254" }
match    = { fg = "#b35900", bold = true }
```

A broken config, an unknown color or key, makes hdf exit with 2 and name the file, line and
column.
