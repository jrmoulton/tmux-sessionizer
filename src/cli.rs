use std::fs::canonicalize;

use crate::{
    configs::{Config, SearchDirectory},
    execute_tmux_command, get_single_selection, TmsError,
};
use clap::{Arg, ArgMatches, Command};
use error_stack::{Result, ResultExt};

pub(crate) fn create_app() -> ArgMatches {
    Command::new("tms")
        .author("Jared Moulton <jaredmoulton3@gmail.com>")
        .version(clap::crate_version!())
        .about("Scan for all git folders in specified directories, select one and open it as a new tmux session")
        .subcommand(
            Command::new("config")
                .arg_required_else_help(true)
                .about("Configure the defaults for search paths and excluded directories")
                .arg(
                    Arg::new("search paths")
                        .short('p')
                        .long("paths")
                        .required(false)
                        .num_args(1..)
                        .help("The paths to search through. Shell like expansions such as `~` are supported")
                )
                .arg(
                    Arg::new("default session")
                        .short('s')
                        .long("session")
                        .required(false)
                        .num_args(1)
                        .help("The default session to switch to (if available) when killing another session")
                )
                .arg(
                    Arg::new("excluded dirs")
                        .long("excluded")
                        .required(false)
                        .num_args(1..)
                        .help("As many directory names as desired to not be searched over")
                )
                .arg(
                    Arg::new("remove dir")
                        .required(false)
                        .num_args(1..)
                        .long("remove")
                        .help("As many directory names to be removed from the exclusion list")
                )
                .arg(
                    Arg::new("display full path")
                        .required(false)
                        .num_args(1)
                        .value_name("true | false")
                        .value_parser(clap::value_parser!(bool))
                        .long("full-path")
                        .help("Use the full path when displaying directories")
                )
                .arg(
                    Arg::new("search submodules")
                        .required(false)
                        .num_args(1)
                        .value_name("true | false")
                        .value_parser(clap::value_parser!(bool))
                        .long("search-submodules")
                        .help("Also show initialized submodules")
                )
                .arg(
                    Arg::new("max depth")
                        .required(false)
                        .num_args(1..)
                        .value_parser(clap::value_parser!(usize))
                        .short('d')
                        .long("max-depth")
                        .help("The maximum depth to traverse when searching for repositories in the search paths, length should match the number of search paths if specified (defaults to 10)")
                )
        )
        .subcommand(Command::new("start").about("Initialize tmux with the default sessions"))
        .subcommand(Command::new("switch").about("Display other sessions with a fuzzy finder and a preview window"))
        .subcommand(Command::new("windows").about("Display the current session's windows with a fuzzy finder and a preview window"))
        .subcommand(Command::new("kill")
            .about("Kill the current tmux session and jump to another")
        )
        .subcommand(Command::new("sessions")
            .about("Show running tmux sessions with asterisk on the current session")
        )
        .get_matches()
}

pub(crate) fn handle_sub_commands(cli_args: ArgMatches) -> Result<SubCommandGiven, TmsError> {
    // Get the configuration from the config file
    let mut config = Config::new().change_context(TmsError::ConfigError)?;
    match cli_args.subcommand() {
        Some(("start", _sub_cmd_matches)) => {
            if let Some(sessions) = config.sessions {
                for session in sessions {
                    let mut sesssion_start_string = String::from("tmux new-session -d");
                    if let Some(session_name) = session.name {
                        sesssion_start_string.push_str(&format!(" -s {session_name}"));
                    }
                    if let Some(session_path) = session.path {
                        sesssion_start_string.push_str(&format!(
                            " -c {}",
                            shellexpand::full(&session_path).change_context(TmsError::IoError)?
                        ))
                    }
                    execute_tmux_command(&sesssion_start_string);
                    drop(sesssion_start_string); // just to be clear that this string is done
                    if let Some(windows) = session.windows {
                        for window in windows {
                            let mut window_start_string = String::from("tmux new-window");
                            if let Some(window_name) = window.name {
                                window_start_string.push_str(&format!(" -n {window_name}"));
                            }
                            if let Some(window_path) = window.path {
                                window_start_string.push_str(&format!(
                                    " -c {}",
                                    shellexpand::full(&window_path)
                                        .change_context(TmsError::IoError)?
                                ));
                            }
                            execute_tmux_command(&window_start_string);
                            if let Some(window_command) = window.command {
                                execute_tmux_command(&format!(
                                    "tmux send-keys {window_command} Enter"
                                ));
                            }
                        }
                        execute_tmux_command("tmux kill-window -t :1");
                    }
                }
                execute_tmux_command("tmux attach");
            } else {
                execute_tmux_command("tmux");
            }
            Ok(SubCommandGiven::Yes)
        }

        Some(("switch", _sub_cmd_matches)) => {
            let sessions = String::from_utf8(
                execute_tmux_command(
                    "tmux list-sessions -F '#{?session_attached,,#{session_name}}",
                )
                .stdout,
            )
            .unwrap();
            let sessions: Vec<String> = sessions
                .replace('\'', "")
                .replace("\n\n", "\n")
                .trim()
                .split('\n')
                .map(|s| s.to_string())
                .collect();

            if let Some(target_session) =
                get_single_selection(&sessions, Some("tmux capture-pane -ept {}".to_string()))?
            {
                execute_tmux_command(&format!(
                    "tmux switch-client -t {}",
                    target_session.replace('.', "_")
                ));
            }

            Ok(SubCommandGiven::Yes)
        }

        Some(("windows", _sub_cmd_matches)) => {
            let windows = String::from_utf8(
                execute_tmux_command("tmux list-windows -F '#{?window_attached,,#{window_name}}")
                    .stdout,
            )
            .unwrap();
            let windows: Vec<String> = windows
                .replace('\'', "")
                .replace("\n\n", "\n")
                .trim()
                .split('\n')
                .map(|s| s.to_string())
                .collect();
            if let Some(target_window) =
                get_single_selection(&windows, Some("tmux capture-pane -ept {}".to_string()))?
            {
                execute_tmux_command(&format!(
                    "tmux select-window -t {}",
                    target_window.replace('.', "_")
                ));
            }

            Ok(SubCommandGiven::Yes)
        }
        // Handle the config subcommand
        Some(("config", sub_cmd_matches)) => {
            let max_depths = match sub_cmd_matches.get_many::<usize>("max depth") {
                Some(depths) => depths.collect::<Vec<_>>(),
                None => Vec::new(),
            };
            config.search_dirs = match sub_cmd_matches.get_many::<String>("search paths") {
                Some(paths) => Some(
                    paths
                        .into_iter()
                        .zip(max_depths.into_iter().chain(std::iter::repeat(&10)))
                        .map(|(path, depth)| {
                            let path = if path.ends_with('/') {
                                let mut modified_path = path.clone();
                                modified_path.pop();
                                modified_path
                            } else {
                                path.clone()
                            };
                            shellexpand::full(&path)
                                .map(|val| (val.to_string(), *depth))
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

            if let Some(default_session) = sub_cmd_matches
                .get_one::<String>("default session")
                .map(|val| val.replace('.', "_"))
            {
                config.default_session = Some(default_session);
            }

            if let Some(display) = sub_cmd_matches.get_one::<bool>("display full path") {
                config.display_full_path = Some(display.to_owned());
            }

            if let Some(submodules) = sub_cmd_matches.get_one::<bool>("search submodules") {
                config.search_submodules = Some(submodules.to_owned());
            }

            if let Some(dirs) = sub_cmd_matches.get_many::<String>("excluded dirs") {
                let current_excluded = config.excluded_dirs;
                match current_excluded {
                    Some(mut excl_dirs) => {
                        excl_dirs.extend(dirs.into_iter().map(|str| str.to_string()));
                        config.excluded_dirs = Some(excl_dirs)
                    }
                    None => {
                        config.excluded_dirs =
                            Some(dirs.into_iter().map(|str| str.to_string()).collect());
                    }
                }
            }
            if let Some(dirs) = sub_cmd_matches.get_one::<String>("remove dir") {
                let current_excluded = config.excluded_dirs;
                match current_excluded {
                    Some(mut excl_dirs) => {
                        dirs.split(' ')
                            .for_each(|dir| excl_dirs.retain(|x| x != dir));
                        config.excluded_dirs = Some(excl_dirs);
                    }
                    None => todo!(),
                }
            }

            config.save().change_context(TmsError::ConfigError)?;
            println!("Configuration has been stored");
            Ok(SubCommandGiven::Yes)
        }

        // The kill subcommand will kill the current session and switch to another one
        Some(("kill", _)) => {
            let mut current_session =
                String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            current_session.retain(|x| x != '\'' && x != '\n');

            let sessions =
                String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            let sessions: Vec<&str> = sessions.lines().collect();

            let to_session = if config.default_session.is_some()
                && sessions.contains(&config.default_session.as_deref().unwrap())
                && current_session != config.default_session.as_deref().unwrap()
            {
                config.default_session.as_deref().unwrap()
            } else if current_session != sessions[0] {
                sessions[0]
            } else {
                sessions.get(1).unwrap_or_else(|| &sessions[0])
            };
            execute_tmux_command(&format!("tmux switch-client -t {to_session}"));
            execute_tmux_command(&format!("tmux kill-session -t {current_session}"));
            Ok(SubCommandGiven::Yes)
        }

        // The sessions subcommand will print the sessions with an asterisk over the current
        // session
        Some(("sessions", _)) => {
            let mut current_session =
                String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            current_session.retain(|x| x != '\'' && x != '\n');
            let current_session_star = format!("{current_session}*");
            let sessions =
                String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
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
            Ok(SubCommandGiven::Yes)
        }
        _ => Ok(SubCommandGiven::No(config)),
    }
}

pub enum SubCommandGiven {
    Yes,
    No(Config),
}
