use std::{error::Error, fmt::Display};

use crate::{configs::Config, execute_tmux_command, ConfigError};
use clap::{Arg, ArgMatches, Command};
use error_stack::{IntoReport, Result, ResultExt};

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
                        .help("The paths to search through. ")
                )
                .arg(
                    Arg::new("default session")
                        .short('s')
                        .long("session")
                        .required(false)
                        .num_args(1)
                        .help("The default session to switch to (if avaliable) when killing another session")
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
                        .value_names(["true", "false"])
                        .value_parser(clap::value_parser!(bool))
                        .long("full-path")
                        .help("Use the full path when displaying directories")
                )
        )
        .subcommand(Command::new("kill")
            .about("Kill the current tmux session and jump to another")
        )
        .subcommand(Command::new("sessions")
            .about("Show running tmux sessions with asterisk on the current session")
        )
        .get_matches()
}

pub(crate) fn handle_sub_commands(cli_args: ArgMatches) -> Result<OptionGiven, CliError> {
    match cli_args.subcommand() {
        // Handle the config subcommand
        Some(("config", sub_cmd_matches)) => {
            let mut defaults = confy::load::<Config>("tms", None)
                .into_report()
                .change_context(CliError::ConfigError)?;
            defaults.search_paths = match sub_cmd_matches.get_many::<String>("search paths") {
                Some(paths) => {
                    let mut paths = paths.map(|x| x.to_string()).collect::<Vec<String>>();
                    paths.iter_mut().for_each(|path| {
                        *path = if path.chars().rev().next().unwrap() == '/' {
                            let mut path = path.to_string();
                            path.pop();
                            path
                        } else {
                            path.to_owned()
                        }
                    });
                    paths
                }
                None => defaults.search_paths,
            };
            defaults.default_session = sub_cmd_matches
                .get_one::<String>("default session")
                .map(|val| val.replace('.', "_"));

            defaults.display_full_path = sub_cmd_matches
                .get_one::<bool>("display full path")
                .copied();

            if let Some(dirs) = sub_cmd_matches.get_many::<String>("excluded dirs") {
                let current_excluded = defaults.excluded_dirs;
                match current_excluded {
                    Some(mut excl_dirs) => {
                        excl_dirs.extend(dirs.into_iter().map(|str| str.to_string()));
                        defaults.excluded_dirs = Some(excl_dirs)
                    }
                    None => {
                        defaults.excluded_dirs =
                            Some(dirs.into_iter().map(|str| str.to_string()).collect());
                    }
                }
            }
            if let Some(dirs) = sub_cmd_matches.get_one::<String>("remove dir") {
                let current_excluded = defaults.excluded_dirs;
                match current_excluded {
                    Some(mut excl_dirs) => {
                        dirs.split(' ')
                            .for_each(|dir| excl_dirs.retain(|x| x != dir));
                        defaults.excluded_dirs = Some(excl_dirs);
                    }
                    None => todo!(),
                }
            }
            let config = Config {
                search_paths: defaults.search_paths,
                excluded_dirs: defaults.excluded_dirs,
                default_session: defaults.default_session,
                display_full_path: defaults.display_full_path,
            };

            confy::store("tms", None, config)
                .into_report()
                .change_context(ConfigError::WriteFailure)
                .attach_printable("Failed to write the config file")
                .change_context(CliError::ConfigError)?;
            println!("Configuration has been stored");
            Ok(OptionGiven::Yes)
        }

        // The kill subcommand will kill the current session and switch to anther one
        Some(("kill", _)) => {
            let defaults = confy::load::<Config>("tms", None)
                .into_report()
                .change_context(ConfigError::LoadError)
                .attach_printable("Failed to load the config file")
                .change_context(CliError::ConfigError)?;
            let mut current_session =
                String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            current_session.retain(|x| x != '\'' && x != '\n');

            let sessions =
                String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            let sessions: Vec<&str> = sessions.lines().collect();

            let to_session = if defaults.default_session.is_some()
                && sessions.contains(&defaults.default_session.clone().unwrap().as_str())
                && current_session != defaults.default_session.clone().unwrap()
            {
                defaults.default_session.unwrap()
            } else if current_session != sessions[0] {
                sessions[0].to_string()
            } else {
                sessions.get(1).unwrap_or_else(|| &sessions[0]).to_string()
            };
            execute_tmux_command(&format!("tmux switch-client -t {to_session}"));
            execute_tmux_command(&format!("tmux kill-session -t {current_session}"));
            Ok(OptionGiven::Yes)
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
            Ok(OptionGiven::Yes)
        }
        _ => Ok(OptionGiven::No),
    }
}

#[derive(Debug)]
pub(crate) enum CliError {
    ConfigError,
}
impl Display for CliError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{self:?}"))
    }
}
impl Error for CliError {}

pub enum OptionGiven {
    Yes,
    No,
}
