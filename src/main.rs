use serde_derive::{Deserialize, Serialize};
use std::{collections::VecDeque, fs, io::Cursor};

use clap::{Arg, Command};
use skim::prelude::*;

#[derive(Default)]
struct Repos {
    repos: Vec<git2::Repository>,
}
impl ToString for Repos {
    fn to_string(&self) -> String {
        let mut return_string = String::new();
        for repo in &self.repos {
            if repo.is_bare() {
                return_string.push_str(&format!(
                    "{}\n",
                    repo.path().file_name().unwrap().to_str().unwrap()
                ));
            } else {
                return_string.push_str(&format!(
                    "{}\n",
                    repo.path()
                        .parent()
                        .unwrap()
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                ));
            }
        }
        return_string
    }
}
impl Repos {
    fn new() -> Self {
        Self::default()
    }
    fn push(&mut self, repo: git2::Repository) {
        self.repos.push(repo);
    }
    fn find(&self, name: String) -> Option<&git2::Repository> {
        for repo in &self.repos {
            if repo.is_bare() {
                let temp = repo.path().file_name().unwrap().to_str().unwrap();
                if temp == name {
                    return Some(repo);
                }
            } else {
                let temp = repo
                    .path()
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap();
                if temp == name {
                    return Some(repo);
                }
            }
        }
        None
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
struct Config {
    search_path: String,
    excluded_dirs: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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
        .get_matches();

    match matches.subcommand() {
        Some(("config", sub_m)) => {
            let mut defaults: Config = confy::load("tms").unwrap();
            defaults.search_path = match sub_m.value_of("search path") {
                Some(name) => {
                    if name.chars().rev().nth(0).unwrap() == '/' {
                        let mut name = name.to_string();
                        name.pop();
                        name
                    } else {
                        name.to_string()
                    }
                }
                None => defaults.search_path,
            };
            match sub_m.values_of("excluded dirs") {
                Some(dirs) => defaults
                    .excluded_dirs
                    .extend(dirs.into_iter().map(|str| str.to_string())),
                None => {}
            }
            match sub_m.value_of("remove dir") {
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
        _ => {}
    }

    let config: Config = confy::load("tms").unwrap();
    let default_path = if !config.search_path.is_empty() {
        config.search_path
    } else {
        println!("You must configure a default search path with `tms config` ");
        std::process::exit(1);
    };
    let mut repos = Repos::new();

    let mut to_search = VecDeque::new();
    to_search.extend(fs::read_dir(&default_path).unwrap());

    while !to_search.is_empty() {
        let file = to_search.pop_front().unwrap().unwrap();
        if !config
            .excluded_dirs
            .contains(&file.file_name().to_str().unwrap().to_string())
        {
            if let Ok(repo) = git2::Repository::open(file.path()) {
                repos.push(repo);
            } else if file.path().is_dir() {
                to_search.extend(fs::read_dir(file.path()).unwrap());
            }
        }
    }

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
        println!("No selection made");
        std::process::exit(1);
    }
    let selected = skim_output.selected_items[0].output().to_string();
    let found = repos.find(selected.clone()).unwrap();
    let found_name = if found.is_bare() {
        found.path().file_name().unwrap()
    } else {
        found.path().parent().unwrap().file_name().unwrap()
    };

    let session_exists = String::from_utf8(
        std::process::Command::new("tmux")
            .arg("list-sessions")
            .arg("-F")
            .arg("#S")
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .contains(found_name.to_str().unwrap());

    std::process::Command::new("tmux")
        .arg("new-session")
        .arg("-ds")
        .arg(&selected)
        .arg("-c")
        .arg(format!("{}/{}", default_path, &selected))
        .output()
        .unwrap();

    if !session_exists {
        if found.is_bare() {
            if found.worktrees().unwrap().is_empty() {
                let head = found.head().unwrap();
                let temp = &format!(
                    "{}{}",
                    found.path().to_str().unwrap(),
                    head.shorthand().unwrap()
                );
                let path = &std::path::Path::new(temp);
                found
                    .worktree(
                        &head.shorthand().unwrap(),
                        path,
                        Some(&git2::WorktreeAddOptions::new().reference(Some(&head))),
                    )
                    .unwrap();
            }
            for tree in found.worktrees().unwrap().iter() {
                std::process::Command::new("tmux")
                    .arg("new-window")
                    .arg("-t")
                    .arg(found.path().file_name().unwrap())
                    .arg("-n")
                    .arg(tree.unwrap())
                    .arg("-c")
                    .arg(found.find_worktree(tree.unwrap()).unwrap().path())
                    .output()
                    .unwrap();
            }
        } else {
            let mut find_py_env = VecDeque::new();
            find_py_env.extend(fs::read_dir(found.path().parent().unwrap()).unwrap());

            let mut count = 0;
            const MAX_FILE_CHECKS: i32 = 50;
            while !find_py_env.is_empty() && count < MAX_FILE_CHECKS {
                let file = find_py_env.pop_front().unwrap().unwrap();
                count += 1;
                if file
                    .file_name()
                    .to_str()
                    .unwrap()
                    .to_string()
                    .contains("pyvenv")
                {
                    // tmux send-keys -t $selected_name "clear" Enter
                    std::process::Command::new("tmux")
                        .arg("send-keys")
                        .arg("-t")
                        .arg(found_name)
                        .arg(format!(
                            "source {}/bin/activate",
                            file.path().parent().unwrap().to_str().unwrap()
                        ))
                        .arg("Enter")
                        .output()
                        .unwrap();
                    std::process::Command::new("tmux")
                        .arg("send-keys")
                        .arg("-t")
                        .arg(found_name)
                        .arg("clear")
                        .arg("Enter")
                        .output()
                        .unwrap();
                    break;
                } else if file.path().is_dir() {
                    find_py_env.extend(fs::read_dir(file.path()).unwrap());
                }
            }
        }
    }
    std::process::Command::new("tmux")
        .arg("switch-client")
        .arg("-t")
        .arg(found_name)
        .output()
        .unwrap();
    Ok(())
}
