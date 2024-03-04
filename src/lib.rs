pub mod cli;
pub mod configs;
pub mod dirty_paths;
pub mod error;
pub mod keymap;
pub mod picker;
pub mod repos;
pub mod tmux;

use error::TmsError;
use error_stack::{Result, ResultExt};
use git2::Repository;
use std::{fmt::Display, process};

use configs::PickerColorConfig;
use dirty_paths::DirtyUtf8Path;
use keymap::Keymap;
use picker::{Picker, Preview};
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
#[derive(Debug)]
pub struct Suggestion(&'static str);
impl Display for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crossterm::style::Stylize;
        f.write_str(&format!("Suggestion: {}", self.0).green().bold().to_string())
    }
}
