use std::{
    collections::HashMap,
    env::current_dir,
    fs::canonicalize,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use crate::{
    configs::{
        CloneRepoSwitchConfig, Config, ConfigExport, SearchDirectory, SessionSortOrderConfig,
    },
    dirty_paths::DirtyUtf8Path,
    execute_command, get_single_selection,
    marks::{marks_command, MarksCommand},
    picker::Preview,
    repos::Prunable,
    session::{create_sessions, SessionContainer},
    tmux::Tmux,
    Result, TmsError,
};
use clap::{Args, Parser, Subcommand};
use clap_complete::{ArgValueCandidates, CompletionCandidate};
use error_stack::ResultExt;
use gix::Repository;
use ratatui::style::Color;

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
    /// Clone repository and create a new session for it
    CloneRepo(CloneRepoCommand),
    /// Initialize empty repository
    InitRepo(InitRepoCommand),
    /// Bookmark a directory so it is available to select along with the Git repositories
    Bookmark(BookmarkCommand),
    /// Open a session
    OpenSession(OpenSessionCommand),
    /// Manage list of sessions that can be instantly accessed by their index
    Marks(MarksCommand),
}

#[derive(Debug, Args)]
#[clap(args_conflicts_with_subcommands = true)]
pub struct ConfigCommand {
    #[command(flatten)]
    args: ConfigArgs,
    #[command(subcommand)]
    subcommand: Option<ConfigSubCommand>,
}

#[derive(Debug, Subcommand)]
pub enum ConfigSubCommand {
    /// List current config including all default values
    List(ConfigSubCommandArgs),
}

#[derive(Debug, Args)]
pub struct ConfigSubCommandArgs {
    #[arg(short, long)]
    /// List only defaults without user set values
    defaults: bool,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
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
    #[arg(long, value_name = "true | false")]
    ///Only include sessions from search paths in the switcher
    switch_filter_unknown: Option<bool>,
    #[arg(long, short = 'd', value_name = "max depth", num_args = 1..)]
    /// The maximum depth to traverse when searching for repositories in search paths, length
    /// should match the number of search paths if specified (defaults to 10)
    max_depths: Option<Vec<usize>>,
    #[arg(long, value_name = "#rrggbb")]
    /// Background color of the highlighted item in the picker
    picker_highlight_color: Option<Color>,
    #[arg(long, value_name = "#rrggbb")]
    /// Text color of the hightlighted item in the picker
    picker_highlight_text_color: Option<Color>,
    #[arg(long, value_name = "#rrggbb")]
    /// Color of the borders between widgets in the picker
    picker_border_color: Option<Color>,
    #[arg(long, value_name = "#rrggbb")]
    /// Color of the item count in the picker
    picker_info_color: Option<Color>,
    #[arg(long, value_name = "#rrggbb")]
    /// Color of the prompt in the picker
    picker_prompt_color: Option<Color>,
    #[arg(long, value_name = "Alphabetical | LastAttached")]
    /// Set the sort order of the sessions in the switch command
    session_sort_order: Option<SessionSortOrderConfig>,
    #[arg(long, value_name = "Always | Never | Foreground", verbatim_doc_comment)]
    /// Whether to automatically switch to the new session after the `clone-repo` command finishes
    /// `Always` will always switch tmux to the new session
    /// `Never` will always create the new session in the background
    /// When set to `Foreground`, the new session will only be opened in the background if the active
    /// tmux session has changed since starting the clone process (for long clone processes on larger repos)
    clone_repo_switch: Option<CloneRepoSwitchConfig>,
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

#[derive(Debug, Args)]
pub struct InitRepoCommand {
    /// Name of the repository to initialize
    repository: String,
}

#[derive(Debug, Args)]
pub struct BookmarkCommand {
    #[arg(long, short)]
    /// Delete instead of add a bookmark
    delete: bool,
    /// Path to bookmark, if left empty bookmark the current directory.
    path: Option<String>,
}

#[derive(Debug, Args)]
pub struct OpenSessionCommand {
    #[arg(add = ArgValueCandidates::new(open_session_completion_candidates))]
    /// Name of the session to open.
    session: Box<str>,
}

impl Cli {
    pub fn handle_sub_commands(&self, tmux: &Tmux) -> Result<SubCommandGiven> {
        // Get the configuration from the config file
        let config = Config::new().change_context(TmsError::ConfigError)?;

        match &self.command {
            Some(CliCommand::Start) => {
                start_command(config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::Switch) => {
                switch_command(config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::Windows) => {
                windows_command(&config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }
            // Handle the config subcommand
            Some(CliCommand::Config(args)) => {
                config_command(args, config)?;
                Ok(SubCommandGiven::Yes)
            }

            // The kill subcommand will kill the current session and switch to another one
            Some(CliCommand::Kill) => {
                kill_subcommand(config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            // The sessions subcommand will print the sessions with an asterisk over the current
            // session
            Some(CliCommand::Sessions) => {
                sessions_subcommand(tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            // Rename the active session and the working directory
            // rename
            Some(CliCommand::Rename(args)) => {
                rename_subcommand(args, tmux)?;
                Ok(SubCommandGiven::Yes)
            }
            Some(CliCommand::Refresh(args)) => {
                refresh_command(args, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::CloneRepo(args)) => {
                clone_repo_command(args, config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::InitRepo(args)) => {
                init_repo_command(args, config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::Bookmark(args)) => {
                bookmark_command(args, config)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::OpenSession(args)) => {
                open_session_command(args, config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            Some(CliCommand::Marks(args)) => {
                marks_command(args, config, tmux)?;
                Ok(SubCommandGiven::Yes)
            }

            None => Ok(SubCommandGiven::No(config.into())),
        }
    }
}

fn start_command(config: Config, tmux: &Tmux) -> Result<()> {
    if let Some(sessions) = &config.sessions {
        for session in sessions {
            let session_path = session
                .path
                .as_ref()
                .map(shellexpand::full)
                .transpose()
                .change_context(TmsError::IoError)?;

            tmux.new_session(session.name.as_deref(), session_path.as_deref());

            if let Some(windows) = &session.windows {
                for window in windows {
                    let window_path = window
                        .path
                        .as_ref()
                        .map(shellexpand::full)
                        .transpose()
                        .change_context(TmsError::IoError)?;

                    tmux.new_window(window.name.as_deref(), window_path.as_deref(), None);

                    if let Some(window_command) = &window.command {
                        tmux.send_keys(window_command, None);
                    }
                }
                tmux.kill_window(":1");
            }
        }
        tmux.attach_session(None, None);
    } else {
        tmux.tmux();
    }

    Ok(())
}

fn switch_command(config: Config, tmux: &Tmux) -> Result<()> {
    let sessions = tmux
        .list_sessions("'#{?session_attached,,#{session_name}#,#{session_last_attached}}'")
        .replace('\'', "")
        .replace("\n\n", "\n");

    let mut sessions: Vec<(&str, &str)> = sessions
        .trim()
        .split('\n')
        .filter_map(|s| s.split_once(','))
        .collect();

    if let Some(SessionSortOrderConfig::LastAttached) = config.session_sort_order {
        sessions.sort_by(|a, b| b.1.cmp(a.1));
    }

    let mut sessions: Vec<String> = sessions.into_iter().map(|s| s.0.to_string()).collect();
    if let Some(true) = config.switch_filter_unknown {
        let configured = create_sessions(&config)?;

        sessions = sessions
            .into_iter()
            .filter(|session| configured.find_session(session).is_some())
            .collect::<Vec<String>>();
    }

    if let Some(target_session) =
        get_single_selection(&sessions, Preview::SessionPane, &config, tmux)?
    {
        tmux.switch_client(&target_session.replace('.', "_"));
    }

    Ok(())
}

fn windows_command(config: &Config, tmux: &Tmux) -> Result<()> {
    let windows = tmux.list_windows("'#{?window_attached,,#{window_id} #{window_name}}'", None);

    let windows: Vec<String> = windows
        .replace('\'', "")
        .replace("\n\n", "\n")
        .trim()
        .split('\n')
        .map(|s| s.to_string())
        .collect();

    if let Some(target_window) = get_single_selection(&windows, Preview::WindowPane, config, tmux)?
    {
        if let Some((windex, _)) = target_window.split_once(' ') {
            tmux.select_window(windex);
        }
    }
    Ok(())
}

fn config_command(cmd: &ConfigCommand, mut config: Config) -> Result<()> {
    match &cmd.subcommand {
        None => {}
        Some(ConfigSubCommand::List(args)) => {
            let config = if args.defaults {
                Config::default()
            } else {
                config
            };
            let config = ConfigExport::from(config);
            let toml_pretty =
                toml::to_string_pretty(&config).change_context(TmsError::ConfigError)?;
            println!("{}", toml_pretty);
            return Ok(());
        }
    };
    let args = &cmd.args;
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
                .collect::<Result<Vec<(String, usize)>>>()?
                .iter()
                .map(|(path, depth)| {
                    canonicalize(path)
                        .map(|val| SearchDirectory::new(val, *depth))
                        .change_context(TmsError::IoError)
                })
                .collect::<Result<Vec<SearchDirectory>>>()?,
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

    if let Some(switch_filter_unknown) = args.switch_filter_unknown {
        config.switch_filter_unknown = Some(switch_filter_unknown.to_owned());
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
        picker_colors.highlight_color = Some(*color);
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_highlight_text_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.highlight_text_color = Some(*color);
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_border_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.border_color = Some(*color);
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_info_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.info_color = Some(*color);
        config.picker_colors = Some(picker_colors);
    }
    if let Some(color) = &args.picker_prompt_color {
        let mut picker_colors = config.picker_colors.unwrap_or_default();
        picker_colors.prompt_color = Some(*color);
        config.picker_colors = Some(picker_colors);
    }

    if let Some(order) = &args.session_sort_order {
        config.session_sort_order = Some(order.to_owned());
    }

    if let Some(switch) = &args.clone_repo_switch {
        config.clone_repo_switch = Some(switch.to_owned());
    }

    config.save().change_context(TmsError::ConfigError)?;
    println!("Configuration has been stored");
    Ok(())
}

fn kill_subcommand(config: Config, tmux: &Tmux) -> Result<()> {
    let mut current_session = tmux.display_message("'#S'");
    current_session.retain(|x| x != '\'' && x != '\n');

    let sessions = tmux
        .list_sessions("'#{?session_attached,,#{session_name}#,#{session_last_attached}}'")
        .replace('\'', "")
        .replace("\n\n", "\n");

    let mut sessions: Vec<(&str, &str)> = sessions
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
            .any(|session| session.0 == config.default_session.as_deref().unwrap())
        && current_session != config.default_session.as_deref().unwrap()
    {
        config.default_session.as_deref()
    } else {
        sessions.first().map(|s| s.0)
    };
    if let Some(to_session) = to_session {
        tmux.switch_client(to_session);
    }
    tmux.kill_session(&current_session);

    Ok(())
}

fn sessions_subcommand(tmux: &Tmux) -> Result<()> {
    let mut current_session = tmux.display_message("'#S'");
    current_session.retain(|x| x != '\'' && x != '\n');
    let current_session_star = format!("{current_session}*");

    let sessions = tmux
        .list_sessions("#S")
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
    tmux.refresh_client();

    Ok(())
}

fn rename_subcommand(args: &RenameCommand, tmux: &Tmux) -> Result<()> {
    let new_session_name = &args.name;

    let current_session = tmux
        .display_message("'#S'")
        .trim()
        .replace('\'', "")
        .to_string();

    let panes = tmux.list_windows(
        "'#{window_index}.#{pane_index},#{pane_current_command},#{pane_current_path}'",
        None,
    );

    let mut paneid_to_pane_deatils: HashMap<String, HashMap<String, String>> = HashMap::new();
    let all_panes: Vec<String> = panes
        .trim()
        .split('\n')
        .map(|window| {
            let mut _window: Vec<&str> = window.split(',').collect();

            let pane_index = _window[0].replace('\'', "");
            let pane_details: HashMap<String, String> = HashMap::from([
                (String::from("command"), _window[1].to_string()),
                (
                    String::from("cwd"),
                    _window[2].to_string().replace('\'', ""),
                ),
            ]);

            paneid_to_pane_deatils.insert(pane_index.to_string(), pane_details);

            pane_index.to_string()
        })
        .collect();

    let first_pane_details = &paneid_to_pane_deatils[all_panes.first().unwrap()];

    let new_session_path: String =
        String::from(&first_pane_details["cwd"]).replace(&current_session, new_session_name);

    let move_command_args: Vec<String> =
        [first_pane_details["cwd"].clone(), new_session_path.clone()].to_vec();
    execute_command("mv", move_command_args);

    for pane_index in all_panes.iter() {
        let pane_details = &paneid_to_pane_deatils[pane_index];

        let old_path = &pane_details["cwd"];
        let new_path = old_path.replace(&current_session, new_session_name);

        let change_dir_cmd = format!("cd {new_path}");
        tmux.send_keys(&change_dir_cmd, Some(pane_index));
    }

    tmux.rename_session(new_session_name);
    tmux.attach_session(None, Some(&new_session_path));

    Ok(())
}

fn refresh_command(args: &RefreshCommand, tmux: &Tmux) -> Result<()> {
    let session_name = args
        .name
        .clone()
        .unwrap_or(tmux.display_message("'#S'"))
        .trim()
        .replace('\'', "");
    // For each window there should be the branch names
    let session_path = tmux
        .display_message("'#{session_path}'")
        .trim()
        .replace('\'', "");

    let existing_window_names: Vec<_> = tmux
        .list_windows("'#{window_name}'", Some(&session_name))
        .lines()
        .map(|line| line.replace('\'', ""))
        .collect();

    if let Ok(repository) = gix::open(&session_path) {
        let mut num_worktree_windows = 0;
        if let Ok(worktrees) = repository.worktrees() {
            for worktree in worktrees.iter() {
                let worktree_name = worktree.id().to_string();
                if existing_window_names.contains(&worktree_name) {
                    num_worktree_windows += 1;
                    continue;
                }
                if worktree.is_prunable() {
                    // prunable worktrees can have an invalid path so skip that
                    continue;
                }
                num_worktree_windows += 1;
                tmux.new_window(
                    Some(&worktree_name),
                    Some(
                        &worktree
                            .base()
                            .change_context(TmsError::GitError)?
                            .to_string()?,
                    ),
                    Some(&session_name),
                );
            }
        }
        //check if a window is needed for non worktree
        if !repository.is_bare() {
            let count_current_windows = tmux
                .list_windows("'#{window_name}'", Some(&session_name))
                .lines()
                .count();
            if count_current_windows <= num_worktree_windows {
                tmux.new_window(None, Some(&session_path), Some(&session_name));
            }
        }
    }

    Ok(())
}

fn pick_search_path(config: &Config, tmux: &Tmux) -> Result<Option<PathBuf>> {
    let search_dirs = config
        .search_dirs
        .as_ref()
        .ok_or(TmsError::ConfigError)
        .attach_printable("No search path configured")?
        .iter()
        .map(|dir| dir.path.to_string())
        .filter_map(|path| path.ok())
        .collect::<Vec<String>>();

    let path = if search_dirs.len() > 1 {
        get_single_selection(&search_dirs, Preview::Directory, config, tmux)?
    } else {
        let first = search_dirs
            .first()
            .ok_or(TmsError::ConfigError)
            .attach_printable("No search path configured")?;
        Some(first.clone())
    };

    let expanded = path
        .as_ref()
        .map(|path| shellexpand::full(path).change_context(TmsError::IoError))
        .transpose()?
        .map(|path| PathBuf::from(path.as_ref()));
    Ok(expanded)
}

fn clone_repo_command(args: &CloneRepoCommand, config: Config, tmux: &Tmux) -> Result<()> {
    let Some(mut path) = pick_search_path(&config, tmux)? else {
        return Ok(());
    };

    let (_, repo_name) = args
        .repository
        .rsplit_once('/')
        .expect("Repository path contains '/'");
    let repo_name = repo_name.trim_end_matches(".git");
    path.push(repo_name);

    let previous_session = tmux.current_session("#{session_name}");

    let repo = git_clone(&args.repository, &path)?;

    let mut session_name = repo_name.to_string();

    let switch_config = config.clone_repo_switch.unwrap_or_default();

    let switch = match switch_config {
        CloneRepoSwitchConfig::Always => true,
        CloneRepoSwitchConfig::Never => false,
        CloneRepoSwitchConfig::Foreground => {
            let active_session = tmux.current_session("#{session_name}");
            previous_session == active_session
        }
    };

    if tmux.session_exists(&session_name) {
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

    tmux.new_session(Some(&session_name), Some(&path.display().to_string()));
    tmux.set_up_tmux_env(&repo, &session_name)?;
    if switch {
        tmux.switch_to_session(&session_name);
    }

    Ok(())
}

fn git_clone(repo: &str, target: &Path) -> Result<Repository> {
    std::fs::create_dir_all(target).change_context(TmsError::IoError)?;
    let mut cmd = Command::new("git")
        .current_dir(target.parent().ok_or(TmsError::IoError)?)
        .args(["clone", repo, target.to_str().ok_or(TmsError::NonUtf8Path)?])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .change_context(TmsError::GitError)?;

    cmd.wait().change_context(TmsError::GitError)?;
    let repo = gix::open(target).change_context(TmsError::GitError)?;
    Ok(repo)
}

fn init_repo_command(args: &InitRepoCommand, config: Config, tmux: &Tmux) -> Result<()> {
    let Some(mut path) = pick_search_path(&config, tmux)? else {
        return Ok(());
    };
    path.push(&args.repository);

    let repo = gix::init(&path).change_context(TmsError::GitError)?;

    let mut session_name = args.repository.to_string();

    if tmux.session_exists(&session_name) {
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

    tmux.new_session(Some(&session_name), Some(&path.display().to_string()));
    tmux.set_up_tmux_env(&repo, &session_name)?;
    tmux.switch_to_session(&session_name);

    Ok(())
}

fn bookmark_command(args: &BookmarkCommand, mut config: Config) -> Result<()> {
    let path = if let Some(path) = &args.path {
        path.to_owned()
    } else {
        current_dir()
            .change_context(TmsError::IoError)?
            .to_string()
            .change_context(TmsError::IoError)?
    };

    if !args.delete {
        config.add_bookmark(path);
    } else {
        config.delete_bookmark(path);
    }

    config.save().change_context(TmsError::ConfigError)?;

    Ok(())
}

fn open_session_command(args: &OpenSessionCommand, config: Config, tmux: &Tmux) -> Result<()> {
    let sessions = create_sessions(&config)?;

    if let Some(session) = sessions.find_session(&args.session) {
        session.switch_to(tmux, &config)?;
        Ok(())
    } else {
        Err(TmsError::SessionNotFound(args.session.to_string()).into())
    }
}

fn open_session_completion_candidates() -> Vec<CompletionCandidate> {
    Config::new()
        .change_context(TmsError::ConfigError)
        .and_then(|config| create_sessions(&config))
        .map(|sessions| {
            sessions
                .list()
                .iter()
                .map(CompletionCandidate::new)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub enum SubCommandGiven {
    Yes,
    No(Box<Config>),
}
