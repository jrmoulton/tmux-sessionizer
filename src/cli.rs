use std::{collections::HashMap, fs::canonicalize};

use crate::{
    configs::SearchDirectory,
    configs::{Config, SessionSortOrderConfig},
    dirty_paths::DirtyUtf8Path,
    execute_command, execute_tmux_command, get_single_selection, session_exists, set_up_tmux_env,
    switch_to_session, TmsError,
};
use clap::{Args, Parser, Subcommand};
use error_stack::{Result, ResultExt};
use git2::Repository;

#[derive(Debug, Parser)]
#[command(author, version)]
///Scan for all git folders in specified directorires, select one and open it as a new tmux session
pub struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    #[command(arg_required_else_help = true)]
    /// Configure the defaults for search paths and excluded directories
    Config(Box<ConfigCommand>),
    /// Initialize tmux with the default sessions
    Start,
    /// Display other sessions with a fuzzy finder and a preview window
    Switch,
    /// Display the current session's windows with a fuzzy finder and a preview window
    Windows,
    /// Kill the current tmux session and jump to another
    Kill,
    /// Show running tmux sessions with asterisk on the current session
    Sessions,
    #[command(arg_required_else_help = true)]
    /// Rename the active session and the working directory
    Rename(RenameCommand),
    /// Creates new worktree windows for the selected session
    Refresh(RefreshCommand),
    /// Clone repository into the first search path and create a new session for it
    CloneRepo(CloneRepoCommand),
}

#[derive(Debug, Args)]
pub struct ConfigCommand {
    #[arg(short = 'p', long = "paths", value_name = "search paths", num_args = 1..)]
    /// The paths to search through. Shell like expansions such as '~' are supported
    search_paths: Option<Vec<String>>,
    #[arg(short = 's', long = "session", value_name = "default session")]
    /// The default session to switch to (if available) when killing another session
    default_session: Option<String>,
    #[arg(long = "excluded", value_name = "excluded dirs", num_args = 1..)]
    /// As many directory names as desired to not be searched over
    excluded_dirs: Option<Vec<String>>,
    #[arg(long = "remove", value_name = "remove dir", num_args = 1..)]
    /// As many directory names to be removed from exclusion list
    remove_dir: Option<Vec<String>>,
    #[arg(long = "full-path", value_name = "true | false")]
    /// Use the full path when displaying directories
    display_full_path: Option<bool>,
    #[arg(long, value_name = "true | false")]
    /// Also show initialized submodules
    search_submodules: Option<bool>,
    #[arg(long, value_name = "true | false")]
    /// Search submodules for submodules
    recursive_submodules: Option<bool>,
    #[arg(long, short = 'd', value_name = "max depth", num_args = 1..)]
    /// The maximum depth to traverse when searching for repositories in search paths, length
    /// should match the number of search paths if specified (defaults to 10)
    max_depths: Option<Vec<usize>>,
    #[arg(long, value_name = "#rrggbb")]
    /// Background color of the highlighted item in the picker
    picker_highlight_color: Option<String>,
    #[arg(long, value_name = "#rrggbb")]
    /// Text color of the hightlighted item in the picker
    picker_highlight_text_color: Option<String>,
    #[arg(long, value_name = "#rrggbb")]
    /// Color of the borders between widgets in the picker
    picker_border_color: Option<String>,
    #[arg(long, value_name = "#rrggbb")]
    /// Color of the item count in the picker
    picker_info_color: Option<String>,
    #[arg(long, value_name = "#rrggbb")]
    /// Color of the prompt in the picker
    picker_prompt_color: Option<String>,
    #[arg(long, value_name = "Alphabetical | LastAttach")]
    /// Set the sort order of the sessions in the switch command
    session_sort_order: Option<SessionSortOrderConfig>,
}

#[derive(Debug, Args)]
pub struct RenameCommand {
    /// The new session's name
    name: String,
}

#[derive(Debug, Args)]
pub struct RefreshCommand {
    /// The session's name. If not provided gets current session
    name: Option<String>,
}

#[derive(Debug, Args)]
pub struct CloneRepoCommand {
    /// Git repository to clone
    repository: String,
}

impl Cli {
    pub(crate) fn handle_sub_commands(&self) -> Result<SubCommandGiven, TmsError> {
        // Get the configuration from the config file
        let config = Config::new().change_context(TmsError::ConfigError)?;
        match &self.command {
            Some(CliCommand::Start) => {
                start_command(config)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::Switch) => {
                switch_command(config)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::Windows) => {
                windows_command(config)?;
                Ok(SubCommandGiven::Yes)
            }
            // Handle the config subcommand
            Some(CliCommand::Config(args)) => {
                config_command(args, config)?;
                Ok(SubCommandGiven::Yes)
            }

            // The kill subcommand will kill the current session and switch to another one
            Some(CliCommand::Kill) => {
                kill_subcommand(config)?;
                Ok(SubCommandGiven::Yes)
            }

            // The sessions subcommand will print the sessions with an asterisk over the current
            // session
            Some(CliCommand::Sessions) => {
                sessions_subcommand()?;
                Ok(SubCommandGiven::Yes)
            }

            // Rename the active session and the working directory
            // rename
            Some(CliCommand::Rename(args)) => {
                rename_subcommand(args)?;
                Ok(SubCommandGiven::Yes)
            }
            Some(CliCommand::Refresh(args)) => {
                refresh_command(args)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::CloneRepo(args)) => {
                clone_repo_command(args, config)?;
                Ok(SubCommandGiven::Yes)
            }

            None => Ok(SubCommandGiven::No(config.into())),
        }
    }
}

fn start_command(config: Config) -> Result<(), TmsError> {
    if let Some(sessions) = &config.sessions {
        for session in sessions {
            let mut sesssion_start_string = String::from("tmux new-session -d");
            if let Some(session_name) = &session.name {
                sesssion_start_string.push_str(&format!(" -s {session_name}"));
            }
            if let Some(session_path) = &session.path {
                sesssion_start_string.push_str(&format!(
                    " -c {}",
                    shellexpand::full(&session_path).change_context(TmsError::IoError)?
                ))
            }
            execute_tmux_command(&sesssion_start_string);
            drop(sesssion_start_string); // just to be clear that this string is done
            if let Some(windows) = &session.windows {
                for window in windows {
                    let mut window_start_string = String::from("tmux new-window");
                    if let Some(window_name) = &window.name {
                        window_start_string.push_str(&format!(" -n {window_name}"));
                    }
                    if let Some(window_path) = &window.path {
                        window_start_string.push_str(&format!(
                            " -c {}",
                            shellexpand::full(&window_path).change_context(TmsError::IoError)?
                        ));
                    }
                    execute_tmux_command(&window_start_string);
                    if let Some(window_command) = &window.command {
                        execute_tmux_command(&format!("tmux send-keys {window_command} Enter"));
                    }
                }
                execute_tmux_command("tmux kill-window -t :1");
            }
        }
        execute_tmux_command("tmux attach");
    } else {
        execute_tmux_command("tmux");
    }

    Ok(())
}

fn switch_command(config: Config) -> Result<(), TmsError> {
    let sessions = String::from_utf8(
        execute_tmux_command(
            "tmux list-sessions -F '#{?session_attached,,#{session_name}#,#{session_last_attached}}",
        )
        .stdout,
    )
    .unwrap();
    let cleaned = sessions.replace('\'', "").replace("\n\n", "\n");
    let mut sessions: Vec<(&str, &str)> = cleaned
        .trim()
        .split('\n')
        .filter_map(|s| s.split_once(','))
        .collect();

    if let Some(SessionSortOrderConfig::LastAttached) = config.session_sort_order {
        sessions.sort_by(|a, b| b.1.cmp(a.1));
    }

    let sessions: Vec<String> = sessions.into_iter().map(|s| s.0.to_string()).collect();

    if let Some(target_session) = get_single_selection(
        &sessions,
        Some("tmux capture-pane -ept {}".to_string()),
        config.picker_colors,
        config.shortcuts,
    )? {
        execute_tmux_command(&format!(
            "tmux switch-client -t {}",
            target_session.replace('.', "_")
        ));
    }

    Ok(())
}

fn windows_command(config: Config) -> Result<(), TmsError> {
    let windows = String::from_utf8(
        execute_tmux_command("tmux list-windows -F '#{?window_attached,,#{window_name}}").stdout,
    )
    .unwrap();
    let windows: Vec<String> = windows
        .replace('\'', "")
        .replace("\n\n", "\n")
        .trim()
        .split('\n')
        .map(|s| s.to_string())
        .collect();
    if let Some(target_window) = get_single_selection(
        &windows,
        Some("tmux capture-pane -ept {}".to_string()),
        config.picker_colors,
        config.shortcuts,
    )? {
        execute_tmux_command(&format!(
            "tmux select-window -t {}",
            target_window.replace('.', "_")
        ));
    }
    Ok(())
}

fn config_command(args: &ConfigCommand, mut config: Config) -> Result<(), TmsError> {
    let max_depths = args.max_depths.clone().unwrap_or_default();
    config.search_dirs = match &args.search_paths {
        Some(paths) => Some(
            paths
                .iter()
                .zip(max_depths.into_iter().chain(std::iter::repeat(10)))
                .map(|(path, depth)| {
                    let path = if path.ends_with('/') {
                        let mut modified_path = path.clone();
                        modified_path.pop();
                        modified_path
                    } else {
                        path.clone()
                    };
                    shellexpand::full(&path)
                        .map(|val| (val.to_string(), depth))
                        .change_context(TmsError::IoError)
                })
                .collect::<Result<Vec<(String, usize)>, TmsError>>()?
                .iter()
                .map(|(path, depth)| {
                    canonicalize(path)
                        .map(|val| SearchDirectory::new(val, *depth))
                        .change_context(TmsError::IoError)
                })
                .collect::<Result<Vec<SearchDirectory>, TmsError>>()?,
        ),
        None => config.search_dirs,
    };

    if let Some(default_session) = args
        .default_session
        .clone()
        .map(|val| val.replace('.', "_"))
    {
        config.default_session = Some(default_session);
    }

    if let Some(display) = args.display_full_path {
        config.display_full_path = Some(display.to_owned());
    }

    if let Some(submodules) = args.search_submodules {
        config.search_submodules = Some(submodules.to_owned());
    }

    if let Some(submodules) = args.recursive_submodules {
        config.recursive_submodules = Some(submodules.to_owned());
    }

    if let Some(dirs) = &args.excluded_dirs {
        let current_excluded = config.excluded_dirs;
        match current_excluded {
            Some(mut excl_dirs) => {
                excl_dirs.extend(dirs.iter().map(|str| str.to_string()));
                config.excluded_dirs = Some(excl_dirs)
            }
            None => {
                config.excluded_dirs = Some(dirs.iter().map(|str| str.to_string()).collect());
            }
        }
    }
    if let Some(dirs) = &args.remove_dir {
        let current_excluded = config.excluded_dirs;
        match current_excluded {
            Some(mut excl_dirs) => {
                dirs.iter().for_each(|dir| excl_dirs.retain(|x| x != dir));
                config.excluded_dirs = Some(excl_dirs);
            }
            None => todo!(),
        }
    }

    if let Some(color) = &args.picker_highlight_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.highlight_color = Some(color.to_string());
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_highlight_text_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.highlight_text_color = Some(color.to_string());
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_border_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.border_color = Some(color.to_string());
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_info_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.info_color = Some(color.to_string());
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_prompt_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.prompt_color = Some(color.to_string());
        config.picker_colors = Some(picker_colors);
    }

    if let Some(order) = &args.session_sort_order {
        config.session_sort_order = Some(order.to_owned());
    }

    config.save().change_context(TmsError::ConfigError)?;
    println!("Configuration has been stored");
    Ok(())
}

fn kill_subcommand(config: Config) -> Result<(), TmsError> {
    let mut current_session =
        String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout)
            .expect("The tmux command static string should always be valid utf-9");
    current_session.retain(|x| x != '\'' && x != '\n');

    let sessions = String::from_utf8(execute_tmux_command("tmux list-sessions -F '#{?session_attached,,#{session_name}#,#{session_last_attached}}").stdout)
        .expect("The tmux command static string should always be valid utf-9");
    let cleaned = sessions.replace('\'', "").replace("\n\n", "\n");
    let mut sessions: Vec<(&str, &str)> = cleaned
        .trim()
        .split('\n')
        .filter_map(|s| s.split_once(','))
        .collect();

    if let Some(SessionSortOrderConfig::LastAttached) = config.session_sort_order {
        sessions.sort_by(|a, b| b.1.cmp(a.1));
    }

    let to_session = if config.default_session.is_some()
        && sessions
            .iter()
            .find(|session| session.0 == config.default_session.as_deref().unwrap())
            .is_some()
        && current_session != config.default_session.as_deref().unwrap()
    {
        config.default_session.as_deref()
    } else {
        sessions.first().map(|s| s.0)
    };
    if let Some(to_session) = to_session {
        execute_tmux_command(&format!("tmux switch-client -t {to_session}"));
    }
    execute_tmux_command(&format!("tmux kill-session -t {current_session}"));

    Ok(())
}

fn sessions_subcommand() -> Result<(), TmsError> {
    let mut current_session =
        String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout)
            .expect("The tmux command static string should always be valid utf-9");
    current_session.retain(|x| x != '\'' && x != '\n');
    let current_session_star = format!("{current_session}*");
    let sessions = String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
        .expect("The tmux command static string should always be valid utf-9")
        .split('\n')
        .map(String::from)
        .collect::<Vec<String>>();
    let mut new_string = String::new();
    for session in &sessions {
        if session == &current_session {
            new_string.push_str(&current_session_star);
        } else {
            new_string.push_str(session);
        }
        new_string.push(' ')
    }
    println!("{new_string}");
    std::thread::sleep(std::time::Duration::from_millis(100));
    execute_tmux_command("tmux refresh-client -S");

    Ok(())
}

fn rename_subcommand(args: &RenameCommand) -> Result<(), TmsError> {
    let new_session_name = &args.name;

    let raw_current_session =
        String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout).unwrap();

    let current_session = raw_current_session.trim();
    let panes = String::from_utf8(
                execute_tmux_command("tmux list-panes -s -F '#{window_index}.#{pane_index},#{pane_current_command},#{pane_current_path}'")
                    .stdout,
            )
            .unwrap();

    let mut paneid_to_pane_deatils: HashMap<String, HashMap<String, String>> = HashMap::new();
    let all_panes: Vec<String> = panes
        .trim()
        .split('\n')
        .map(|window| {
            let mut _window: Vec<&str> = window.split(',').collect();

            let pane_index = _window[0];
            let pane_details: HashMap<String, String> = HashMap::from([
                (String::from("command"), _window[1].to_string()),
                (String::from("cwd"), _window[2].to_string()),
            ]);

            paneid_to_pane_deatils.insert(pane_index.to_string(), pane_details);

            _window[0].to_string()
        })
        .collect();

    let first_pane_details = &paneid_to_pane_deatils[all_panes.first().unwrap()];

    let new_session_path: String =
        String::from(&first_pane_details["cwd"]).replace(current_session, new_session_name);

    let move_command_args: Vec<String> =
        [first_pane_details["cwd"].clone(), new_session_path.clone()].to_vec();
    execute_command("mv", move_command_args);

    for pane_index in all_panes.iter() {
        let pane_details = &paneid_to_pane_deatils[pane_index];

        let old_path = &pane_details["cwd"];
        let new_path = old_path.replace(current_session, new_session_name);

        let change_dir_cmd = format!("cd {new_path}");
        execute_tmux_command(&format!(
            "tmux send-keys -t {} \"{}\" Enter",
            pane_index, change_dir_cmd
        ));
    }

    execute_tmux_command(&format!("tmux rename-session {}", new_session_name));
    execute_tmux_command(&format!("tmux attach -c {}", new_session_path));

    Ok(())
}

fn refresh_command(args: &RefreshCommand) -> Result<(), TmsError> {
    let session_name = args
        .name
        .clone()
        .unwrap_or(
            String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout).unwrap(),
        )
        .trim()
        .replace('\'', "");
    // For each window there should be the branch names
    let session_path =
        String::from_utf8(execute_tmux_command("tmux display-message -p '#{session_path}'").stdout)
            .unwrap()
            .trim()
            .replace('\'', "");
    let existing_window_names: Vec<_> = String::from_utf8(
        execute_tmux_command(&format!(
            "tmux list-windows -t {session_name} -F '#{{window_name}}'"
        ))
        .stdout,
    )
    .unwrap()
    .lines()
    .map(|line| line.replace('\'', ""))
    .collect();
    let create_window = |session_name: &str, path_to_tree: &str, window_name: Option<&str>| {
        let args: Vec<_> = [
            Some("new-window"),
            Some("-t"),
            Some(session_name),
            Some("-c"),
            Some(path_to_tree),
            window_name.map(|_| "-n"),
            window_name,
        ]
        .iter()
        .cloned()
        .filter_map(|f| f.map(String::from))
        .collect();
        execute_command("tmux", args);
    };

    if let Ok(repository) = Repository::open(&session_path) {
        let mut num_worktree_windows = 0;
        if let Ok(worktrees) = repository.worktrees() {
            for worktree_name in worktrees.iter().flatten() {
                let worktree = repository
                    .find_worktree(worktree_name)
                    .change_context(TmsError::GitError)?;
                if existing_window_names.contains(&String::from(worktree_name)) {
                    num_worktree_windows += 1;
                    continue;
                }
                if !worktree.is_prunable(None).unwrap_or_default() {
                    num_worktree_windows += 1;
                    // prunable worktrees can have an invalid path so skip that
                    create_window(
                        &session_name,
                        &worktree.path().to_string()?,
                        Some(worktree_name),
                    );
                }
            }
        }
        //check if a window is needed for non worktree
        if !repository.is_bare() {
            let count_current_windows = String::from_utf8(
                execute_tmux_command(&format!(
                    "tmux list-windows -t {session_name} -F '#{{window_name}}'"
                ))
                .stdout,
            )
            .unwrap()
            .lines()
            .count();
            if count_current_windows <= num_worktree_windows {
                create_window(&session_name, &session_path, None);
            }
        }
    }

    Ok(())
}

fn clone_repo_command(args: &CloneRepoCommand, config: Config) -> Result<(), TmsError> {
    let search_dirs = config
        .search_dirs
        .ok_or(TmsError::ConfigError)
        .attach_printable("No search path configured")?;
    let mut path = search_dirs
        .first()
        .ok_or(TmsError::ConfigError)
        .attach_printable("No search path configured")?
        .path
        .clone();

    let (_, repo_name) = args
        .repository
        .rsplit_once('/')
        .expect("Repository path contains '/'");
    let repo_name = repo_name.trim_end_matches(".git");
    path.push(repo_name);

    let repo = Repository::clone(&args.repository, &path).change_context(TmsError::GitError)?;

    let mut session_name = repo_name.to_string();

    if session_exists(&session_name) {
        session_name = format!(
            "{}/{}",
            path.parent()
                .unwrap()
                .file_name()
                .expect("The file name doesn't end in `..`")
                .to_string()?,
            session_name
        );
    }

    execute_tmux_command(&format!(
        "tmux new-session -ds {} -c {}",
        session_name,
        path.display()
    ));
    set_up_tmux_env(&repo, &session_name)?;
    switch_to_session(&session_name);

    Ok(())
}

pub enum SubCommandGiven {
    Yes,
    No(Box<Config>),
}
