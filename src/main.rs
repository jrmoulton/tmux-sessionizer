mod repos;

use anyhow::{anyhow, Context, Result};
use clap::{Arg, ArgMatches, Command};
use git2::Repository;
use repos::{DirtyUtf8Path, RepoContainer};
use serde_derive::{Deserialize, Serialize};
use skim::prelude::*;
use std::{
    collections::{HashMap, VecDeque},
    fs,
    io::Cursor,
    process,
};

#[derive(Default, Debug, Serialize, Deserialize)]
struct Config {
    search_path: String,
    excluded_dirs: Vec<String>,
}

fn main() -> Result<()> {
    let matches = Command::new("tmux-sessionizer")
        .author("Jared Moulton <jaredmoulton3@gmail.com>")
        .version("0.1.0")
        .about("Scan for all git folders in a specified directory, select one and open it as a new tmux session")
        .subcommand(
            Command::new("config")
                .arg_required_else_help(true)
                .arg(
                    Arg::new("search path")
                        .short('p')
                        .long("path")
                        .required(false)
                        .takes_value(true)
                        .help("The path to search through")
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
        )
        .subcommand(Command::new("kill")
            .about("Kill the current tmux session and jump to another"))
        .get_matches();

    handle_sub_commands(matches)?;

    // This point is reached only if a subcommand is not given
    let config: Config = confy::load("tms")?;
    let default_path = if !config.search_path.is_empty() {
        config.search_path
    } else {
        return Err(anyhow!(
            "You must configure a default search path with `tms config` "
        ));
    };

    let repos = find_git_repos(&default_path, config.excluded_dirs)?;
    let repo_name = get_single_selection(&repos)?;

    let found_repo = repos
        .find(&repo_name)
        .context("Could not find the internal representation of the selected repository")?;

    let sessions = String::from_utf8(execute_tmux_command("tmux list-sessions -F #S")?.stdout)?;
    let mut sessions = sessions.lines();
    let session_previously_existed = sessions.any(|line| {
        // tmux will return the output with extra ' and \n characters
        line.to_owned().retain(|char| char != '\'' && char != '\n');
        line == repo_name
    });

    if !session_previously_existed {
        execute_tmux_command(&format!(
            "tmux new-session -ds {repo_name} -c {default_path}/{repo_name}"
        ))?;
        set_up_tmux_env(found_repo, &repo_name)?;
    }
    execute_tmux_command(&format!(
        "tmux switch-client -t {}",
        repo_name.replace('.', "_")
    ))?;
    Ok(())
}

fn set_up_tmux_env(repo: &Repository, repo_name: &str) -> Result<()> {
    if repo.is_bare() {
        if repo.worktrees()?.is_empty() {
            // Add the default branch as a tree (usually either main or master)
            let head = repo.head()?;
            let path_to_default_tree =
                format!("{}{}", repo.path().to_string()?, head.shorthand().unwrap());
            let path = std::path::Path::new(&path_to_default_tree);
            repo.worktree(
                head.shorthand().unwrap(),
                path,
                Some(git2::WorktreeAddOptions::new().reference(Some(&head))),
            )?;
        }
        for tree in repo.worktrees()?.iter() {
            let window_name = tree.unwrap().to_string();
            let path_to_tree = repo.find_worktree(tree.unwrap())?.path().to_string()?;

            execute_tmux_command(&format!(
                "tmux new-window -t {repo_name} -n {window_name} -c {path_to_tree}"
            ))?;
        }
        // Kill that first extra window
        execute_tmux_command(&format!("tmux kill-window -t {repo_name}:1"))?;
    } else {
        activate_py_env(repo, repo_name, 50)?;
    }
    Ok(())
}

fn execute_tmux_command(command: &str) -> Result<process::Output> {
    let args: Vec<&str> = command.split(' ').skip(1).collect();
    Ok(process::Command::new("tmux")
        .args(args)
        .output()
        .unwrap_or_else(|_| panic!("Failed to execute the tmux command `{command}`")))
}

fn handle_sub_commands(matches: ArgMatches) -> Result<()> {
    match matches.subcommand() {
        Some(("config", sub_cmd_matches)) => {
            let mut defaults: Config = confy::load("tms")?;
            defaults.search_path = match sub_cmd_matches.value_of("search path") {
                Some(name) => {
                    if name
                        .chars()
                        .rev()
                        .next()
                        .context("The path must be at least 1 character long")?
                        == '/'
                    {
                        let mut name = name.to_string();
                        name.pop();
                        name
                    } else {
                        name.to_string()
                    }
                }
                None => defaults.search_path,
            };
            match sub_cmd_matches.values_of("excluded dirs") {
                Some(dirs) => defaults
                    .excluded_dirs
                    .extend(dirs.into_iter().map(|str| str.to_string())),
                None => {}
            }
            match sub_cmd_matches.value_of("remove dir") {
                Some(dir) => defaults.excluded_dirs.retain(|x| x != dir),
                None => {}
            }
            let config = Config {
                search_path: defaults.search_path,
                excluded_dirs: defaults.excluded_dirs,
            };
            confy::store("tms", config)?;
            println!("Configuration has been stored");
            std::process::exit(0);
        }
        Some(("kill", _)) => {
            let mut current_session =
                String::from_utf8(execute_tmux_command("tmux display-message -p '#S'")?.stdout)?;
            current_session.retain(|x| x != '\'' && x != '\n');
            // TODO: Just switch to some rando open tmux session (or create a new one) not assume
            // config
            execute_tmux_command("tmux switch-client -t _config")?;
            execute_tmux_command(&format!("tmux kill-session -t {current_session}"))?;
            std::process::exit(1);
        }
        _ => Ok(()),
    }
}

fn get_single_selection(repos: &HashMap<String, Repository>) -> Result<String> {
    let options = SkimOptionsBuilder::default()
        .height(Some("50%"))
        .multi(false)
        .color(Some("bw"))
        .build()
        .unwrap();
    let item_reader = SkimItemReader::default();
    let item = item_reader.of_bufread(Cursor::new(repos.to_string()));
    let skim_output = Skim::run_with(&options, Some(item)).unwrap();
    if skim_output.is_abort {
        return Err(anyhow!("No selection made"));
    }
    Ok(skim_output.selected_items[0].output().to_string())
}

fn find_git_repos(
    default_path: &str,
    excluded_dirs: Vec<String>,
) -> Result<HashMap<String, Repository>> {
    let mut repos = HashMap::new();
    let mut to_search = VecDeque::new();
    to_search.extend(fs::read_dir(&default_path)?);
    while !to_search.is_empty() {
        let file = to_search.pop_front().unwrap()?;
        if !excluded_dirs.contains(&file.file_name().to_string()?) {
            if let Ok(repo) = git2::Repository::open(file.path()) {
                let name = file.path().file_name().unwrap().to_string()?;
                repos.insert(name, repo);
            } else if file.path().is_dir() {
                to_search.extend(fs::read_dir(file.path())?);
            }
        }
    }
    Ok(repos)
}

fn activate_py_env(found_repo: &Repository, found_name: &str, max_files_checks: u32) -> Result<()> {
    let mut find_py_env = VecDeque::new();
    find_py_env.extend(fs::read_dir(found_repo.path().parent().unwrap())?);

    let mut count = 0;
    while !find_py_env.is_empty() && count < max_files_checks {
        let file = find_py_env.pop_front().unwrap()?;
        count += 1;
        if file.file_name().to_str().unwrap().contains("pyvenv") {
            std::process::Command::new("tmux")
                .arg("send-keys")
                .arg("-t")
                .arg(found_name)
                .arg(format!(
                    "source {}/bin/activate",
                    file.path().parent().unwrap().to_str().unwrap()
                ))
                .arg("Enter")
                .output()?;
            execute_tmux_command(&format!("tmux send-keys -t {found_name} clear Enter",))?;
            return Ok(());
        } else if file.path().is_dir() {
            find_py_env.extend(fs::read_dir(file.path())?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    // Need to test that tmux sessions can be created, session switched to, multiple windows can be
    // opened for bare repositories, python environments can be found and activated
    // #[test]
    // fn read_search_path() {
    //     use crate::Config;

    //     confy::store(
    //         "tms-test",
    //         Config {
    //             search_path: "/home".to_string(),
    //             excluded_dirs: Vec::new(),
    //         },
    //     )
    //     .unwrap();
    //     let config: Config = confy::load("tms-test").unwrap();
    //     assert_eq!(config.search_path, "/home");
    // }
}
