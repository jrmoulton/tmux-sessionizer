mod cli;
mod configs;
mod dirty_paths;
mod picker;
mod repos;

use crate::{
    cli::{create_app, handle_sub_commands, SubCommandGiven},
    dirty_paths::DirtyUtf8Path,
};
use aho_corasick::{AhoCorasickBuilder, MatchKind};
use configs::ConfigError;
use configs::SearchDirectory;
use error_stack::{Report, Result, ResultExt};
use git2::{Repository, Submodule};

use picker::Picker;
use repos::RepoContainer;
use std::fs::canonicalize;
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt::Display,
    fs, process,
};

fn main() -> Result<(), TmsError> {
    // Install debug hooks for formatting of error handling
    Report::install_debug_hook::<Suggestion>(|value, context| {
        context.push_body(format!("{value}"));
    });
    #[cfg(any(not(debug_assertions), test))]
    Report::install_debug_hook::<std::panic::Location>(|_value, _context| {});

    // Use CLAP to parse the command line arguments
    let cli_args = create_app();
    let config = match handle_sub_commands(cli_args)? {
        SubCommandGiven::Yes => return Ok(()),
        SubCommandGiven::No(config) => config, // continue
    };

    let mut search_dirs = config.search_dirs.unwrap_or(Vec::new());

    // merge old search paths with new search directories
    if let Some(search_paths) = config.search_paths {
        if !search_paths.is_empty() {
            search_dirs.extend(search_paths.into_iter().map(|path| {
                SearchDirectory::new(
                    canonicalize(
                        shellexpand::full(&path)
                            .change_context(TmsError::IoError)
                            .unwrap()
                            .to_string(),
                    )
                    .change_context(TmsError::IoError)
                    .unwrap(),
                    10,
                )
            }));
        }
    }

    if search_dirs.is_empty() {
        return Err(ConfigError::NoDefaultSearchPath)
            .attach_printable(
                "You must configure at least one default search path with the `config` subcommand. E.g `tms config` ",
            )
            .change_context(TmsError::ConfigError);
    }

    // Find repositories and present them with the fuzzy finder
    let repos = find_repos(
        search_dirs,
        config.excluded_dirs,
        config.display_full_path,
        config.search_submodules,
        config.recursive_submodules,
    )?;

    let repo_name = if let Some(str) = get_single_selection(&repos.list(), None)? {
        str
    } else {
        return Ok(());
    };

    let found_repo = repos
        .find_repo(&repo_name)
        .expect("The internal representation of the selected repository should be present");
    let path = if found_repo.is_bare() {
        found_repo.path().to_string()?
    } else {
        found_repo
            .workdir()
            .expect("bare repositories should all have parent directories")
            .canonicalize()
            .change_context(TmsError::IoError)?
            .to_string()?
    };
    let repo_short_name = std::path::PathBuf::from(&repo_name)
        .file_name()
        .expect("None of the paths here should terminate in `..`")
        .to_string()?
        .replace('.', "_");

    // Get the tmux sessions
    let sessions = String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
        .expect("The tmux command static string should always be valid utf-8");
    let mut sessions = sessions.lines();

    // If the session already exists switch to it, else create the new session and then switch
    let session_previously_existed = sessions.any(|line| {
        // tmux will return the output with extra ' and \n characters
        line.to_owned().retain(|char| char != '\'' && char != '\n');
        line == repo_short_name
    });
    if !session_previously_existed {
        execute_tmux_command(&format!(
            "tmux new-session -ds {repo_short_name } -c {path}",
        ));
        set_up_tmux_env(found_repo, &repo_short_name)?;
    }

    if !is_in_tmux_session() {
        execute_tmux_command(&format!("tmux attach -t {repo_short_name}"));
        return Ok(());
    }

    let result = execute_tmux_command(&format!("tmux switch-client -t {repo_short_name}"));
    if !result.status.success() {
        execute_tmux_command(&format!("tmux attach -t {repo_short_name}"));
    }
    Ok(())
}

pub(crate) fn set_up_tmux_env(repo: &Repository, repo_name: &str) -> Result<(), TmsError> {
    if repo.is_bare() {
        if repo
            .worktrees()
            .change_context(TmsError::GitError)?
            .is_empty()
        {
            // Add the default branch as a tree (usually either main or master)
            let head = repo.head().change_context(TmsError::GitError)?;
            let head_short = head
                .shorthand()
                .ok_or(TmsError::NonUtf8Path)
                .attach_printable("The selected repository has an unusable path")?;
            let path_to_default_tree = format!("{}{}", repo.path().to_string()?, head_short);
            let path = std::path::Path::new(&path_to_default_tree);
            repo.worktree(
                head_short,
                path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&head))),
            )
            .change_context(TmsError::GitError)?;
        }
        for tree in repo.worktrees().change_context(TmsError::GitError)?.iter() {
            let tree = tree.ok_or(TmsError::NonUtf8Path).attach_printable(format!(
                "The path to the found sub-tree {tree:?} has a non-utf8 path",
            ))?;
            let window_name = tree.to_string();
            let path_to_tree = repo
                .find_worktree(tree)
                .change_context(TmsError::GitError)?
                .path()
                .to_string()?;

            execute_tmux_command(&format!(
                "tmux new-window -t {repo_name} -n {window_name} -c {path_to_tree}"
            ));
        }
        // Kill that first extra window
        execute_tmux_command(&format!("tmux kill-window -t {repo_name}:^"));
    } else {
        // Extra stuff?? I removed launching python environments here but that could be exposed in the configuration
    }
    Ok(())
}

pub fn execute_command(command: &str, args: Vec<String>) -> process::Output {
    process::Command::new(command)
        .args(args)
        .stdin(process::Stdio::inherit())
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute command `{command}`"))
}

pub fn execute_tmux_command(command: &str) -> process::Output {
    let args: Vec<&str> = command.split(' ').skip(1).collect();
    process::Command::new("tmux")
        .args(args)
        .stdin(process::Stdio::inherit())
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute the tmux command `{command}`"))
}


fn is_in_tmux_session() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|program| program == "tmux")
}


fn get_single_selection(
    list: &[String],
    preview_command: Option<String>,
) -> Result<Option<String>, TmsError> {
    let mut picker = Picker::new(list, preview_command);

    Ok(picker.run()?)
}

fn find_repos(
    directories: Vec<SearchDirectory>,
    excluded_dirs: Option<Vec<String>>,
    display_full_path: Option<bool>,
    search_submodules: Option<bool>,
    recursive_submodules: Option<bool>,
) -> Result<impl RepoContainer, TmsError> {
    let mut repos = HashMap::new();
    let mut to_search = VecDeque::new();

    for search_directory in directories {
        to_search.push_back(search_directory);
    }

    let excluded_dirs = match excluded_dirs {
        Some(excluded_dirs) => excluded_dirs,
        None => Vec::new(),
    };
    let excluder = AhoCorasickBuilder::new()
        .match_kind(MatchKind::LeftmostFirst)
        .build(excluded_dirs)
        .change_context(TmsError::IoError)?;
    while let Some(file) = to_search.pop_front() {
        if excluder.is_match(&file.path.to_string()?) {
            continue;
        }

        let file_name = file
            .path
            .file_name()
            .expect("The file name doesn't end in `..`")
            .to_string()?;

        if let Ok(repo) = git2::Repository::open(file.path.clone()) {
            if repo.is_worktree() {
                continue;
            }
            let name = if let Some(true) = display_full_path {
                file.path.to_string()?
            } else {
                file_name
            };
            if search_submodules == Some(true) {
                if let Ok(submodules) = repo.submodules() {
                    find_submodules(
                        submodules,
                        &name,
                        &mut repos,
                        display_full_path,
                        recursive_submodules,
                    )?;
                }
            }
            repos.insert_repo(name, repo);
        } else if file.path.is_dir() && file.depth > 0 {
            let read_dir = fs::read_dir(file.path)
                .change_context(TmsError::IoError)?
                .map(|dir_entry| dir_entry.expect("Found non-valid utf8 path").path());
            for dir in read_dir {
                to_search.push_back(SearchDirectory::new(dir, file.depth - 1))
            }
        }
    }
    Ok(repos)
}

fn find_submodules(
    submodules: Vec<Submodule>,
    parent_name: &String,
    repos: &mut impl RepoContainer,
    display_full_path: Option<bool>,
    recursive: Option<bool>,
) -> Result<(), TmsError> {
    for submodule in submodules.iter() {
        let repo = match submodule.open() {
            Ok(repo) => repo,
            _ => continue,
        };
        let path = match repo.workdir() {
            Some(path) => path,
            _ => continue,
        };
        let submodule_file_name = path
            .file_name()
            .expect("The file name doesn't end in `..`")
            .to_string()?;
        let name = if let Some(true) = display_full_path {
            path.to_string()?
        } else {
            format!("{}>{}", parent_name, submodule_file_name)
        };

        if recursive == Some(true) {
            if let Ok(submodules) = repo.submodules() {
                find_submodules(submodules, &name, repos, display_full_path, recursive)?;
            }
        }
        repos.insert_repo(name, repo);
    }
    Ok(())
}

#[derive(Debug)]
pub struct Suggestion(&'static str);
impl Display for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crossterm::style::Stylize;
        f.write_str(&format!("Suggestion: {}", self.0).green().bold().to_string())
    }
}

#[derive(Debug)]
pub(crate) enum TmsError {
    GitError,
    NonUtf8Path,
    TuiError(String),
    IoError,
    ConfigError,
}
impl Display for TmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigError => write!(f, "Config Error"),
            Self::GitError => write!(f, "Git Error"),
            Self::NonUtf8Path => write!(f, "Non Utf-8 Path"),
            Self::IoError => write!(f, "IO Error"),
            Self::TuiError(inner) => write!(f, "TUI error: {inner}"),
        }
    }
}
impl Error for TmsError {}

#[cfg(test)]
mod tests;
