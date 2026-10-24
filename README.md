# hdf

A fuzzy finder for bash history. Pick a command to put it on the prompt line, or delete it from
the history, every occurrence of it.

## Install

Build and install the binary into `~/.cargo/bin`:

```bash
cargo install --path .
```

Load the key bindings in `~/.bashrc`, they need bash 4.0 or later. Copy `home/.hdfrc` anywhere and
source it:

```bash
[[ -f ~/.hdfrc ]] && . ~/.hdfrc
```

Optionally, install the bash completion of the options. With bash-completion it loads on demand
from:

```bash
cp completions/_hdf ~/.local/share/bash-completion/completions/hdf
```

Without bash-completion, source `completions/_hdf` in `~/.bashrc` instead.

## Bash history

hdf reads and edits the history file, `$HISTFILE`. Searching and picking work with bash's defaults.

Deleting needs two settings in `~/.bashrc`:

```bash
# Save each command at the prompt and reload the file
PROMPT_COMMAND="history -a; history -c; history -r${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
# Keep hdf --delete-last out of the history, add a pattern for an alias of it too
HISTIGNORE="${HISTIGNORE:+$HISTIGNORE:}hdf --delete-last*"
```

Bash writes commands to the file only on exit. Up and Down save them before opening hdf, but
`hdf --delete-last` runs on its own: without `history -a` at each prompt the file lacks your latest
commands, and it deletes older ones instead. Once the file is saved at each prompt, the
`hdf --delete-last` line itself lands there too, and the next run would delete it, so leave it out
with `HISTIGNORE`.

Ctrl-D changes only the file too. The shell keeps its own copy, which Ctrl-P, Ctrl-N, `!!` and
`history` use, and reloading it at each prompt drops the deleted commands from it.

Optionally, for the rest of hdf:

```bash
# Append to the file on exit instead of overwriting what other shells saved
shopt -s histappend
# Save when each command ran, hdf shows it. Commands saved before have no time
HISTTIMEFORMAT='%F %T '
# Keep more than the 500 commands bash keeps by default
HISTSIZE=10000
HISTFILESIZE=10000
```

## Usage

Up or Down opens hdf with the typed text as the query. Enter runs the chosen command, Tab puts it
on the prompt line to edit, Esc puts back the edited query. Ctrl-P and Ctrl-N still step through
the shell's history as usual, see [Bash history](#bash-history).

Each word of the query matches anywhere in a command, in any order, ignoring case: `push git`
finds `git -C api push`. The commands stay most recent first.

With bash 4.4 or later hdf shows your prompt in place of `>`, unless it takes more than 60% of the
width.

| Key                         | Action                                      |
| --------------------------- | ------------------------------------------- |
| Up, Down                    | move the selection (also Ctrl-P, Ctrl-N)    |
| PgUp, PgDn                  | move the selection by a page                |
| Enter                       | run the selected command                    |
| Tab                         | put the selected command on the line        |
| Ctrl-D                      | delete the selected command from history    |
| Left, Right, Home, End      | move the cursor (also Ctrl-A, Ctrl-E)       |
| Alt-B, Alt-F, Ctrl-Arrows   | move the cursor by word                     |
| Backspace, Delete           | delete a character                          |
| Alt-Backspace, Ctrl-W       | delete a word                               |
| Ctrl-U                      | delete to the start of the query            |
| Esc                         | quit, keeping the query                     |
| Ctrl-C                      | quit, leaving the prompt line as it was     |

With the list above the prompt line, Up, Ctrl-P and PgUp move to older commands, see
[Config](#config).

`hdf --delete-last [N]` deletes the last N commands from history, 1 by default, and prints them.
It needs the history saved at each prompt, see [Bash history](#bash-history).

`hdf --help` lists all options and exit statuses.

## Config

`hdf --init-config` writes the defaults to `~/.config/hdf/config.toml` (or under
`$XDG_CONFIG_HOME`). Keys left out keep their defaults:

```toml
height = 12       # rows of the pane, the terminal's at most
list   = "below"  # or "above" the prompt line, like fzf
border = false    # a line on the far edge of the pane
[colors]
prompt   = { fg = "blue", bold = true }
count    = "dark-gray"     # matches/commands
deleted  = "green"         # the status after Ctrl-D
error    = "red"           # Ctrl-D failed, the count of no matches
match    = { fg = "yellow", bold = true }
dim      = "dark-gray"     # times, run counts, cuts, dividers, border
selected = { bg = "238" }  # the selected row, under the match
```

`height` counts the prompt line and the divider, so it is at least 3.

With `list = "above"` the prompt line is the last row of the pane and the most recent commands
are right over it. Up, Ctrl-P and PgUp then move to older commands, up the list, and Up in bash
opens hdf and keeps going back in history. When the pane closes, the prompt goes back to the row
it was on.

`border = true` draws a line on the edge of the pane away from the prompt line, the last row below
it or the first row above, in the `dim` color. It takes a row from the list, and is left out when
the pane has no room for it and a row of the list.

A color is a name (`yellow`, `light-blue`, `dark-gray`), `0` to `255` or `#rrggbb`, or a table of
`fg`, `bg`, `bold`, `italic`, `underlined` and `reversed`.

## Development

`bundle install`, then `bundle exec rake` runs rubocop, `cargo test`, clippy and the Ruby tests,
which drive hdf in a pseudo terminal.
