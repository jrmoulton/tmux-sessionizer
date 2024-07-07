pub mod cli;
pub mod configs;
pub mod dirty_paths;
pub mod error;
pub mod keymap;
pub mod picker;
pub mod repos;
pub mod session;
pub mod tmux;

use configs::Config;
use std::process;

use crate::{
    error::{Result, TmsError},
    picker::{Picker, Preview},
    tmux::Tmux,
};

pub fn execute_command(command: &str, args: Vec<String>) -> process::Output {
    process::Command::new(command)
        .args(args)
        .stdin(process::Stdio::inherit())
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute command `{command}`"))
}

pub fn get_single_selection(
    list: &[String],
    preview: Preview,
    config: &Config,
    tmux: &Tmux,
) -> Result<Option<String>> {
    let mut picker = Picker::new(list, preview, config.shortcuts.as_ref(), tmux)
        .set_colors(config.picker_colors.as_ref());

    picker.run()
}
