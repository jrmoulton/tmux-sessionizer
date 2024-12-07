use std::{env, os::unix::process::CommandExt, path::Path, process};

use error_stack::ResultExt;
use git2::Repository;

use crate::{
    configs::Config,
    dirty_paths::DirtyUtf8Path,
    error::{Result, TmsError},
};

#[derive(Clone)]
pub struct Tmux {
    socket_name: String,
}

impl Default for Tmux {
    fn default() -> Self {
        let socket_name = env::var("TMS_TMUX_SOCKET")
            .ok()
            .unwrap_or(String::from("default"));

        Self { socket_name }
    }
}

impl Tmux {
    // Private utility functions

    fn execute_tmux_command(&self, args: &[&str]) -> process::Output {
        process::Command::new("tmux")
            .args(["-L", &self.socket_name])
            .args(args)
            .stdin(process::Stdio::inherit())
            .output()
            .unwrap_or_else(|_| panic!("Failed to execute the tmux command `{args:?}`"))
    }

    fn replace_with_tmux_command(&self, args: &[&str]) -> std::io::Error {
        process::Command::new("tmux")
            .args(["-L", &self.socket_name])
            .args(args)
            .stdin(process::Stdio::inherit())
            .exec()
    }

    fn stdout_to_string(output: process::Output) -> String {
        String::from_utf8(output.stdout)
            .expect("The output of a `tmux` command should always be valid utf-8")
    }

    // Wrapper around various tmux commands

    pub fn tmux(&self) -> process::Output {
        self.execute_tmux_command(&[])
    }

    // sessions

    pub fn new_session(&self, name: Option<&str>, path: Option<&str>) -> process::Output {
        let mut args = vec!["new-session", "-d"];

        if let Some(name) = name {
            args.extend(["-s", name]);
        };

        if let Some(path) = path {
            args.extend(["-c", path]);
        }

        self.execute_tmux_command(&args)
    }

    pub fn list_sessions(&self, format: &str) -> String {
        let output = self.execute_tmux_command(&["list-sessions", "-F", format]);
        Tmux::stdout_to_string(output)
    }

    pub fn current_session(&self, format: &str) -> String {
        let output = self.execute_tmux_command(&[
            "list-sessions",
            "-F",
            format,
            "-f",
            "#{session_attached}",
        ]);
        Tmux::stdout_to_string(output)
    }

    pub fn kill_session(&self, session: &str) -> process::Output {
        self.execute_tmux_command(&["kill-session", "-t", session])
    }

    pub fn rename_session(&self, session_name: &str) -> process::Output {
        self.execute_tmux_command(&["rename-session", session_name])
    }

    pub fn attach_session(&self, session_name: Option<&str>, path: Option<&str>) -> std::io::Error {
        let mut args = vec!["attach-session"];

        if let Some(name) = session_name {
            args.extend(["-t", name]);
        };

        if let Some(path) = path {
            args.extend(["-c", path]);
        }

        self.replace_with_tmux_command(&args)
    }

    pub fn switch_to_session(&self, repo_short_name: &str) {
        if !is_in_tmux_session() {
            self.attach_session(Some(repo_short_name), None);
        } else {
            let result = self.switch_client(repo_short_name);
            if !result.status.success() {
                self.attach_session(Some(repo_short_name), None);
            }
        }
    }

    pub fn session_exists(&self, repo_short_name: &str) -> bool {
        // Get the tmux sessions
        let sessions = self.list_sessions("'#S'");

        // If the session already exists switch to it, else create the new session and then switch
        sessions.lines().any(|line| {
            let mut cleaned_line = line.to_owned();
            // tmux will return the output with extra ' and \n characters
            cleaned_line.retain(|char| char != '\'' && char != '\n');

            cleaned_line == repo_short_name
        })
    }

    pub fn run_session_create_script(
        &self,
        path: &Path,
        session_name: &str,
        config: &Config,
    ) -> Result<()> {
        let command_path = match &config.session_configs {
            Some(sessions) => match sessions.get(session_name) {
                Some(session) => match &session.create_script {
                    Some(create_script) => create_script.to_owned(),
                    None => path.join(".tms-create"),
                },
                None => path.join(".tms-create"),
            },
            None => path.join(".tms-create"),
        };

        self.run_session_script(&command_path, session_name)
    }

    fn run_session_script(&self, command_path: &Path, session_name: &str) -> Result<()> {
        if command_path.exists() {
            self.send_keys(
                &command_path.to_string()?,
                Some(&format!("{}:{{start}}.{{top}}", &session_name)),
            );
        }

        Ok(())
    }

    // windows

    pub fn new_window(
        &self,
        name: Option<&str>,
        path: Option<&str>,
        session: Option<&str>,
    ) -> process::Output {
        let mut args = vec!["new-window"];

        if let Some(name) = name {
            args.extend(["-n", name]);
        };

        if let Some(path) = path {
            args.extend(["-c", path]);
        }

        if let Some(session) = session {
            args.extend(["-t", session])
        }

        self.execute_tmux_command(&args)
    }

    pub fn kill_window(&self, window: &str) -> process::Output {
        self.execute_tmux_command(&["kill-window", "-t", window])
    }

    pub fn list_windows(&self, format: &str, session: Option<&str>) -> String {
        let mut args = vec!["list-windows", "-F", format];

        if let Some(session) = session {
            args.extend(["-t", session]);
        }

        let output = self.execute_tmux_command(&args);
        Tmux::stdout_to_string(output)
    }

    pub fn select_window(&self, window: &str) -> process::Output {
        self.execute_tmux_command(&["select-window", "-t", window])
    }

    // miscellaneous

    pub fn send_keys(&self, command: &str, pane: Option<&str>) -> process::Output {
        let mut args = vec!["send-keys"];

        if let Some(pane) = pane {
            args.extend(["-t", pane]);
        }

        args.extend([command, "Enter"]);

        self.execute_tmux_command(&args)
    }

    pub fn switch_client(&self, session_name: &str) -> process::Output {
        let output = self.execute_tmux_command(&["switch-client", "-t", session_name]);
        if !output.status.success() {
            self.execute_tmux_command(&["attach-session", "-t", session_name])
        } else {
            output
        }
    }

    pub fn display_message(&self, format: &str) -> String {
        let output = self.execute_tmux_command(&["display-message", "-p", format]);
        Tmux::stdout_to_string(output)
    }

    pub fn refresh_client(&self) -> process::Output {
        self.execute_tmux_command(&["refresh-client", "-S"])
    }

    pub fn capture_pane(&self, target_pane: &str) -> process::Output {
        self.execute_tmux_command(&["capture-pane", "-ep", "-t", target_pane])
    }

    pub fn set_up_tmux_env(&self, repo: &Repository, repo_name: &str) -> Result<()> {
        if repo.is_bare() && repo.head().is_ok() {
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
                let path = repo.path().join(head_short);
                repo.worktree(
                    head_short,
                    &path,
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

                self.new_window(Some(&window_name), Some(&path_to_tree), Some(repo_name));
            }
            // Kill that first extra window
            self.kill_window(&format!("{repo_name}:^"));
        }
        Ok(())
    }
}

fn is_in_tmux_session() -> bool {
    std::env::var("TERM_PROGRAM").is_ok_and(|program| program == "tmux")
}
