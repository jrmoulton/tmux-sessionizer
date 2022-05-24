# tmux-sessionizer (tms)

The fastest way to manage projects as tmux sessions

## What is tmux-sessionizer?

Tmux Sessionizer is a tmux session manager that is based on ThePrimeagen's
[tmux-sessionizer](https://github.com/ThePrimeagen/.dotfiles/blob/master/bin/.local/bin/tmux-sessionizer)
but is opinionated and personalized to my specific tmux workflow. And it's awesome. Git worktrees
are automatically opened as new windows, specific directories can be excluded, a default session can
be set, killing a session jumps you to a default, and it is a goal to handle more edge case
scenarios. 

This CLI tool also pairs well with my [git-repo-clone tool](https://github.com/jrmoulton/git-repo-clone).

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


## Notes

I use the `tmux sessions` command to get a tmux styled output of the running session and I use that
output on the bottom right of my tmux status bar. E.g. ![tmux status bar](images/tmux-status-bar.png)

If there is an easier way to do this someone please tell me. 
