pub mod cli;
pub mod configs;
pub mod dirty_paths;
pub mod error;
pub mod keymap;
pub mod marks;
pub mod picker;
pub mod repos;
pub mod session;
pub mod tmux;

use configs::Config;
use std::process;

use crate::{
    error::{Result, TmsError},
    picker::{Picker, PickerItem, Preview},
    tmux::Tmux,
};
use std::collections::HashSet;

pub fn execute_command(command: &str, args: Vec<String>) -> process::Output {
    process::Command::new(command)
        .args(args)
        .stdin(process::Stdio::inherit())
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute command `{command}`"))
}

pub fn get_single_selection(
    list: Vec<PickerItem>,
    running_sessions: HashSet<String>,
    preview: Option<Preview>,
    config: &Config,
    tmux: &Tmux,
) -> Result<Option<PickerItem>> {
    let mut picker = Picker::new(
        list,
        running_sessions,
        preview,
        config.shortcuts.as_ref(),
        config.input_position.unwrap_or_default(),
        tmux,
    )
    .set_colors(config.picker_colors.as_ref());

    picker.run()
}
