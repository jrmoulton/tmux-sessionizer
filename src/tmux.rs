use std::{env, process};

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

    pub fn kill_session(&self, session: &str) -> process::Output {
        self.execute_tmux_command(&["kill-session", "-t", session])
    }

    pub fn rename_session(&self, session_name: &str) -> process::Output {
        self.execute_tmux_command(&["rename-session", session_name])
    }

    pub fn attach_session(
        &self,
        session_name: Option<&str>,
        path: Option<&str>,
    ) -> process::Output {
        let mut args = vec!["attach-session"];

        if let Some(name) = session_name {
            args.extend(["-t", name]);
        };

        if let Some(path) = path {
            args.extend(["-c", path]);
        }

        self.execute_tmux_command(&args)
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
        self.execute_tmux_command(&["switch-client", "-t", session_name])
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
}
