use std::{error::Error, fmt::Display};

use crate::{
    configs::{Config, OldConfig},
    execute_tmux_command, ConfigError, Suggestion,
};
use clap::{Arg, ArgMatches, Command};
use error_stack::{IntoReport, Result, ResultExt};

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

pub(crate) fn create_app() -> ArgMatches {
    Command::new("tmux-sessionizer")
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
                        .takes_value(true)
                        .multiple_values(true)
                        .help("The paths to search through. Paths must be full paths (no support for ~)")
                )
                .arg(
                    Arg::new("default session")
                        .short('s')
                        .long("session")
                        .required(false)
                        .takes_value(true)
                        .help("The default session to switch to (if avaliable) when killing another session")
                )
                .arg(
                    Arg::new("excluded dirs")
                        .long("excluded")
                        .required(false)
                        .takes_value(true)
                        .multiple_values(true)
                        .help("As many directory names as desired to not be searched over")
                )
                .arg(
                    Arg::new("remove dir")
                        .required(false)
                        .takes_value(true)
                        .multiple_values(true)
                        .long("remove")
                        .help("As many directory names to be removed from the exclusion list")
                )
                .arg(
                    Arg::new("display full path")
                        .required(false)
                        .takes_value(true)
                        .possible_values(["true", "false"])
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
        Some(("config", sub_cmd_matches)) => {
            let defaults = confy::load::<Config>("tms");
            let mut defaults = match defaults {
                Ok(defaults) => defaults,
                Err(_) => {
                    let old_config = confy::load::<OldConfig>("tms")
                        .into_report()
                        .change_context(CliError::ConfigError)
                        .attach_printable("Could not find a valid configuration").attach(Suggestion("Try using the `config` subcommand to ensure the search paths have been configured"))?;
                    let path = vec![old_config.search_path];
                    Config {
                        search_paths: path,
                        excluded_dirs: Some(old_config.excluded_dirs),
                        ..Default::default()
                    }
                }
            };
            defaults.search_paths = match sub_cmd_matches.values_of("search paths") {
                Some(paths) => {
                    let mut paths = paths.map(|x| x.to_owned()).collect::<Vec<String>>();
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
                .value_of("default session")
                .map(|val| val.replace('.', "_"));

            defaults.display_full_path = match sub_cmd_matches.value_of("display full path") {
                Some(value) => match value {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => unreachable!(),
                },
                None => defaults.display_full_path,
            };

            if let Some(dirs) = sub_cmd_matches.values_of("excluded dirs") {
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
            if let Some(dirs) = sub_cmd_matches.value_of("remove dir") {
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

            confy::store("tms", config)
                .into_report()
                .change_context(ConfigError::WriteFailure)
                .attach_printable("Failed to write the config file")
                .change_context(CliError::ConfigError)?;
            println!("Configuration has been stored");
            Ok(OptionGiven::Yes)
        }
        Some(("kill", _)) => {
            let defaults = confy::load::<Config>("tms")
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
        Some(("sessions", _)) => {
            let sessions =
                String::from_utf8(execute_tmux_command("tmux list-sessions -F #S").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            let mut current_session =
                String::from_utf8(execute_tmux_command("tmux display-message -p '#S'").stdout)
                    .expect("The tmux command static string should always be valid utf-9");
            current_session.retain(|x| x != '\'' && x != '\n');
            let sessions = sessions
                .replace('\n', " ")
                .replace(&current_session, &format!("{current_session}*"));
            println!("{sessions}");
            Ok(OptionGiven::Yes)
        }
        _ => Ok(OptionGiven::No),
    }
}
