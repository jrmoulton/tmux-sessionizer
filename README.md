# tmux-sessionizer (tms)

The fastest way to manage projects as tmux sessions

## What is tmux-sessionizer?

Tmux Sessionizer is a tmux session manager that is based on ThePrimeagen's
[tmux-sessionizer](https://github.com/ThePrimeagen/.dotfiles/blob/master/bin/.local/scripts/tmux-sessionizer)
but is opinionated and personalized to my specific tmux workflow. And it's awesome. Git worktrees
are automatically opened as new windows, specific directories can be excluded, a default session can
be set, killing a session jumps you to a default, and it is a goal to handle more edge case
scenarios. 

Tmux has keybindings built-in to allow you to switch between sessions. By default these are `leader-(` and `leader-)`

Switching between windows is done by default with `leader-p` and `leader-n`

![tms-gif](images/tms-v0_1_1.gif)

## Usage

Running `tms` will find the repos and fuzzy find on them

Use `tms --help`
```
USAGE:
    tms [SUBCOMMAND]

OPTIONS:
    -h, --help       Print help information
    -V, --version    Print version information

SUBCOMMANDS:
    config      Configure the defaults for search paths and excluded directories
    help        Print this message or the help of the given subcommand(s)
    kill        Kill the current tmux session and jump to another
    sessions    Show running tmux sessions with asterisk on the current session
```

### Configuring defaults

```
USAGE:
    tms config [OPTIONS]

OPTIONS:
        --excluded <excluded dirs>...
            As many directory names as desired to not be searched over

        --full-path <display full path>
            Use the full path when displaying directories [posible values: true, false]

    -h, --help
            Print help information

    -p, --paths <search paths>...
            The paths to search through. Paths must be full paths (no support for ~)

        --remove <remove dir>...
            As many directory names to be removed from the exclusion list

    -s, --session <default session>
            The default session to switch to (if avaliable) when killing another session
```

## Installation

### Cargo

Install with `cargo install tmux-sessionizer` or

### From source

Clone the repository and install using ```cargo install --path . --force```

## Usage Notes

The 'tms sessions' command can be used to get a styled output of the active sessions with an asterisk on the current session. The configuration would look something like this
```
set -g status-right " #(tms sessions)"
```
E.g. ![tmux status bar](images/tmux-status-bar.png)
If this configuration is used it can be helpful to rebind the default tmux keys for switching sessions so that the status bar is refreshed on every session switch. This can be configured with settings like this.
```
bind -r '(' switch-client -p\; refresh-client -S
bind -r ')' switch-client -n\; refresh-client -S
```
