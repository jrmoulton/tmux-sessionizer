mod cli;
mod configs;
mod dirty_paths;
mod repos;

use crate::{
    cli::{create_app, handle_sub_commands, SubCommandGiven},
    dirty_paths::DirtyUtf8Path,
};
use configs::ConfigError;
use error_stack::{IntoReport, Report, Result, ResultExt};
use git2::Repository;
use repos::RepoContainer;
use skim::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    error::Error,
    fmt::Display,
    fs,
    io::Cursor,
    process,
};
use aho_corasick::{AhoCorasickBuilder, MatchKind};

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

    if config.search_paths.is_empty() {
        return Err(ConfigError::NoDefaultSearchPath)
            .into_report()
            .attach_printable(
                "You must configure at least one default search path with the `config` subcommand. E.g `tms config` ",
            )
            .change_context(TmsError::ConfigError);
    }

    // Find repositories and present them with the fuzzy finder
    let repos = find_repos(
        config.search_paths,
        config.excluded_dirs,
        config.display_full_path,
    )?;
    let repo_name = get_single_selection(repos.repo_string(), None)?;
    let found_repo = repos
        .find_repo(&repo_name)
        .expect("The internal representation of the selected repository should be present");
    let path = if found_repo.is_bare() {
        found_repo.path().to_string()?
    } else {
        found_repo
            .path()
            .parent()
            .expect("bare repositories should all have parent directories")
            .to_string()?
    };
    let repo_short_name = std::path::PathBuf::from(&repo_name)
        .file_name()
        .expect("None of the paths here should terminate in `..`")
        .to_string()?;

    // Get the tmux sessions
    let sessions = String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
        .into_report()
        .expect("The tmux command static string should always be valid utf-9");
    let mut sessions = sessions.lines();

    // If the session already exists switch to it, else create the new session and then switch
    let session_previously_existed = sessions.any(|line| {
        // tmux will return the output with extra ' and \n characters
        line.to_owned().retain(|char| char != '\'' && char != '\n');
        line == repo_name
    });
    if !session_previously_existed {
        execute_tmux_command(&format!(
            "tmux new-session -ds {repo_short_name } -c {path}",
        ));
        set_up_tmux_env(found_repo, &repo_short_name)?;
    }

    execute_tmux_command(&format!(
        "tmux switch-client -t {}",
        repo_short_name.replace('.', "_")
    ));
    Ok(())
}

pub(crate) fn set_up_tmux_env(repo: &Repository, repo_name: &str) -> Result<(), TmsError> {
    if repo.is_bare() {
        if repo
            .worktrees()
            .into_report()
            .change_context(TmsError::GitError)?
            .is_empty()
        {
            // Add the default branch as a tree (usually either main or master)
            let head = repo
                .head()
                .into_report()
                .change_context(TmsError::GitError)?;
            let head_short = head
                .shorthand()
                .ok_or(TmsError::NonUtf8Path)
                .into_report()
                .attach_printable("The selected repository has an unusable path")?;
            let path_to_default_tree = format!("{}{}", repo.path().to_string()?, head_short);
            let path = std::path::Path::new(&path_to_default_tree);
            repo.worktree(
                head_short,
                path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&head))),
            )
            .into_report()
            .change_context(TmsError::GitError)?;
        }
        for tree in repo
            .worktrees()
            .into_report()
            .change_context(TmsError::GitError)?
            .iter()
        {
            let tree = tree
                .ok_or(TmsError::NonUtf8Path)
                .into_report()
                .attach_printable(format!(
                    "The path to the found sub-tree {tree:?} has a non-utf8 path",
                ))?;
            let window_name = tree.to_string();
            let path_to_tree = repo
                .find_worktree(tree)
                .into_report()
                .change_context(TmsError::GitError)?
                .path()
                .to_string()?;

            execute_tmux_command(&format!(
                "tmux new-window -t {repo_name} -n {window_name} -c {path_to_tree}"
            ));
        }
        // Kill that first extra window
        execute_tmux_command(&format!("tmux kill-window -t {repo_name}:1"));
    } else {
        // Extra stuff?? I removed launching python environments here but that could be exposed in the configuration
    }
    Ok(())
}

pub fn execute_tmux_command(command: &str) -> process::Output {
    let args: Vec<&str> = command.split(' ').skip(1).collect();
    process::Command::new("tmux")
        .args(args)
        .stdin(process::Stdio::inherit())
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute the tmux command `{command}`"))
}

fn get_single_selection(list: String, preview: Option<&str>) -> Result<String, TmsError> {
    let options = SkimOptionsBuilder::default()
        .height(Some("50%"))
        .preview(preview)
        .multi(false)
        .color(Some("dark"))
        .build()
        .map_err(TmsError::FuzzyFindError)?;
    let item_reader = SkimItemReader::default();
    let item = item_reader.of_bufread(Cursor::new(list));
    let skim_output = Skim::run_with(&options, Some(item))
        .ok_or_else(|| TmsError::FuzzyFindError("Fuzzy finder internal errors".into()))?;
    if skim_output.is_abort {
        return Err(Report::new(TmsError::CliError).attach_printable("No selection made"));
    }
    Ok(skim_output
        .selected_items
        .get(0)
        .ok_or(TmsError::CliError)
        .into_report()
        .attach_printable("No selection made")?
        .output()
        .to_string())
}

fn find_repos(
    paths: Vec<String>,
    excluded_dirs: Option<Vec<String>>,
    display_full_path: Option<bool>,
) -> Result<impl RepoContainer, TmsError> {
    let mut repos = HashMap::new();
    let mut to_search = VecDeque::new();

    for path in paths {
        to_search.push_back(
            std::fs::canonicalize(
                shellexpand::full(&path)
                    .into_report()
                    .change_context(TmsError::IoError)?
                    .to_string(),
            )
            .into_report()
            .change_context(TmsError::IoError)?,
        )
    }

    let excluded_dirs = match excluded_dirs {
        Some(excluded_dirs) => excluded_dirs,
        None => Vec::new(),
    };
    let excluder = AhoCorasickBuilder::new()
        .match_kind(MatchKind::LeftmostFirst)
        .build(&excluded_dirs)
        .unwrap();
    while let Some(file) = to_search.pop_front() {
        let file_name = file
            .file_name()
            .expect("The file name doesn't end in `..`")
            .to_string()?;
        if !excluder.is_match(&file.as_path().to_string().unwrap()) {
            if let Ok(repo) = git2::Repository::open(file.clone()) {
                let name = if let Some(true) = display_full_path {
                    file.to_string()?
                } else {
                    file_name
                };
                repos.insert_repo(name, repo);
            } else if file.is_dir() {
                to_search.extend(
                    fs::read_dir(file)
                        .into_report()
                        .change_context(TmsError::IoError)?
                        .map(|dir_entry| dir_entry.expect("Found non-valid utf8 path").path()),
                );
            }
        }
    }
    Ok(repos)
}

#[derive(Debug)]
pub struct Suggestion(&'static str);
impl Display for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use owo_colors::OwoColorize;
        f.write_str(
            &owo_colors::OwoColorize::bold(&format!("Suggestion: {}", self.0))
                .green()
                .to_string(),
        )
    }
}

#[derive(Debug)]
pub(crate) enum TmsError {
    CliError,
    GitError,
    NonUtf8Path,
    FuzzyFindError(String),
    IoError,
    ConfigError,
}
impl Display for TmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CliError => write!(f, "Cli Error"),
            Self::ConfigError => write!(f, "Config Error"),
            Self::GitError => write!(f, "Git Error"),
            Self::NonUtf8Path => write!(f, "Non Utf-8 Path"),
            Self::IoError => write!(f, "IO Error"),
            Self::FuzzyFindError(inner) => write!(f, "Error with fuzzy finder {inner}"),
        }
    }
}
impl Error for TmsError {}

#[cfg(test)]
mod tests;
