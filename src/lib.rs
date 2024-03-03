pub mod cli;
pub mod configs;
pub mod dirty_paths;
pub mod keymap;
pub mod picker;
pub mod repos;
pub mod tmux;

use aho_corasick::{AhoCorasickBuilder, MatchKind};
use error_stack::{Result, ResultExt};
use git2::{Repository, Submodule};
use std::{collections::HashMap, error::Error};
use std::{collections::VecDeque, fmt::Display, fs, path::Path, process};

use configs::{PickerColorConfig, SearchDirectory};
use dirty_paths::DirtyUtf8Path;
use keymap::Keymap;
use picker::{Picker, Preview};
use repos::RepoContainer;
use tmux::Tmux;

pub fn switch_to_session(repo_short_name: &str, tmux: &Tmux) {
    if !is_in_tmux_session() {
        tmux.attach_session(Some(repo_short_name), None);
    } else {
        let result = tmux.switch_client(repo_short_name);
        if !result.status.success() {
            tmux.attach_session(Some(repo_short_name), None);
        }
    }
}

pub fn session_exists(repo_short_name: &str, tmux: &Tmux) -> bool {
    // Get the tmux sessions
    let sessions = tmux.list_sessions("'#S'");

    // If the session already exists switch to it, else create the new session and then switch
    sessions.lines().any(|line| {
        // tmux will return the output with extra ' and \n characters
        line.to_owned().retain(|char| char != '\'' && char != '\n');
        line == repo_short_name
    })
}

pub fn set_up_tmux_env(repo: &Repository, repo_name: &str, tmux: &Tmux) -> Result<(), TmsError> {
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

            tmux.new_window(Some(&window_name), Some(&path_to_tree), Some(repo_name));
        }
        // Kill that first extra window
        tmux.kill_window(&format!("{repo_name}:^"));
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

pub fn is_in_tmux_session() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|program| program == "tmux")
}

pub fn get_single_selection(
    list: &[String],
    preview: Preview,
    colors: Option<PickerColorConfig>,
    keymap: Option<Keymap>,
    tmux: Tmux,
) -> Result<Option<String>, TmsError> {
    let mut picker = Picker::new(list, preview, keymap, tmux).set_colors(colors);

    Ok(picker.run()?)
}

pub fn find_repos(
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

        let file_name = get_repo_name(&file.path, &repos)?;

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

fn get_repo_name(path: &Path, repos: &impl RepoContainer) -> Result<String, TmsError> {
    let mut repo_name = path
        .file_name()
        .expect("The file name doesn't end in `..`")
        .to_string()?;

    repo_name = if repos.find_repo(&repo_name).is_some() {
        if let Some(parent) = path.parent() {
            if let Some(parent) = parent.file_name() {
                format!("{}/{}", parent.to_string()?, repo_name)
            } else {
                repo_name
            }
        } else {
            repo_name
        }
    } else {
        repo_name
    };

    Ok(repo_name)
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
pub enum TmsError {
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
