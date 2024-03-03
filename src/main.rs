mod cli;
mod configs;
mod dirty_paths;
mod error;
mod keymap;
mod picker;
mod repos;

use crate::{
    cli::{Cli, SubCommandGiven},
    dirty_paths::DirtyUtf8Path,
    error::TmsError,
    repos::{find_repos, RepoContainer},
};
use clap::Parser;
use configs::PickerColorConfig;
use error_stack::{Report, Result, ResultExt};
use git2::Repository;

use keymap::Keymap;
use picker::Picker;
use std::{fmt::Display, process};

fn main() -> Result<(), TmsError> {
    // Install debug hooks for formatting of error handling
    Report::install_debug_hook::<Suggestion>(|value, context| {
        context.push_body(format!("{value}"));
    });
    #[cfg(any(not(debug_assertions), test))]
    Report::install_debug_hook::<std::panic::Location>(|_value, _context| {});

    // Use CLAP to parse the command line arguments
    let cli_args = Cli::parse();
    let config = match cli_args.handle_sub_commands()? {
        SubCommandGiven::Yes => return Ok(()),
        SubCommandGiven::No(config) => config, // continue
    };

    // Find repositories and present them with the fuzzy finder
    let repos = find_repos(
        config.search_dirs()?,
        config.excluded_dirs,
        config.display_full_path,
        config.search_submodules,
        config.recursive_submodules,
    )?;

    let repo_name = if let Some(str) =
        get_single_selection(&repos.list(), None, config.picker_colors, config.shortcuts)?
    {
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
    let repo_short_name = (if config.display_full_path == Some(true) {
        std::path::PathBuf::from(&repo_name)
            .file_name()
            .expect("None of the paths here should terminate in `..`")
            .to_string()?
    } else {
        repo_name
    })
    .replace('.', "_");

    if !session_exists(&repo_short_name) {
        execute_tmux_command(&format!("tmux new-session -ds {repo_short_name} -c {path}",));
        set_up_tmux_env(found_repo, &repo_short_name)?;
    }

    switch_to_session(&repo_short_name);

    Ok(())
}

pub(crate) fn switch_to_session(repo_short_name: &str) {
    if !is_in_tmux_session() {
        execute_tmux_command(&format!("tmux attach -t {repo_short_name}"));
    } else {
        let result = execute_tmux_command(&format!("tmux switch-client -t {repo_short_name}"));
        if !result.status.success() {
            execute_tmux_command(&format!("tmux attach -t {repo_short_name}"));
        }
    }
}

pub(crate) fn session_exists(repo_short_name: &str) -> bool {
    // Get the tmux sessions
    let sessions = String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
        .expect("The tmux command static string should always be valid utf-8");
    let mut sessions = sessions.lines();

    // If the session already exists switch to it, else create the new session and then switch
    sessions.any(|line| {
        // tmux will return the output with extra ' and \n characters
        line.to_owned().retain(|char| char != '\'' && char != '\n');
        line == repo_short_name
    })
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
    colors: Option<PickerColorConfig>,
    keymap: Option<Keymap>,
) -> Result<Option<String>, TmsError> {
    let mut picker = Picker::new(list, preview_command, keymap).set_colors(colors);

    Ok(picker.run()?)
}

#[derive(Debug)]
pub struct Suggestion(&'static str);
impl Display for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crossterm::style::Stylize;
        f.write_str(&format!("Suggestion: {}", self.0).green().bold().to_string())
    }
}

#[cfg(test)]
mod tests;
