use aho_corasick::{AhoCorasickBuilder, MatchKind};
use error_stack::{Result, ResultExt};
use git2::{Repository, Submodule};
use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::Path,
};

use crate::{configs::SearchDirectory, dirty_paths::DirtyUtf8Path, TmsError};

pub trait RepoContainer {
    fn find_repo(&self, name: &str) -> Option<&Repository>;
    fn insert_repo(&mut self, name: String, repo: Repository);
    fn list(&self) -> Vec<String>;
}

impl RepoContainer for HashMap<String, Repository> {
    fn find_repo(&self, name: &str) -> Option<&Repository> {
        self.get(name)
    }

    fn insert_repo(&mut self, name: String, repo: Repository) {
        self.insert(name, repo);
    }

    fn list(&self) -> Vec<String> {
        let mut list: Vec<String> = self.keys().map(|s| s.to_owned()).collect();
        list.sort();

        list
    }
}

pub fn find_repos(
    directories: Vec<SearchDirectory>,
    excluded_dirs: Option<Vec<String>>,
    display_full_path: Option<bool>,
    search_submodules: Option<bool>,
    recursive_submodules: Option<bool>,
) -> Result<impl RepoContainer, TmsError> {
    let mut repos = HashMap::new();
    let mut to_search = VecDeque::new();

    for search_directory in directories {
        to_search.push_back(search_directory);
    }

    let excluded_dirs = match excluded_dirs {
        Some(excluded_dirs) => excluded_dirs,
        None => Vec::new(),
    };
    let excluder = AhoCorasickBuilder::new()
        .match_kind(MatchKind::LeftmostFirst)
        .build(excluded_dirs)
        .change_context(TmsError::IoError)?;
    while let Some(file) = to_search.pop_front() {
        if excluder.is_match(&file.path.to_string()?) {
            continue;
        }

        let file_name = get_repo_name(&file.path, &repos)?;

        if let Ok(repo) = git2::Repository::open(file.path.clone()) {
            if repo.is_worktree() {
                continue;
            }
            let name = if let Some(true) = display_full_path {
                file.path.to_string()?
            } else {
                file_name
            };

            if search_submodules == Some(true) {
                if let Ok(submodules) = repo.submodules() {
                    find_submodules(
                        submodules,
                        &name,
                        &mut repos,
                        display_full_path,
                        recursive_submodules,
                    )?;
                }
            }
            repos.insert_repo(name, repo);
        } else if file.path.is_dir() && file.depth > 0 {
            let read_dir = fs::read_dir(file.path)
                .change_context(TmsError::IoError)?
                .map(|dir_entry| dir_entry.expect("Found non-valid utf8 path").path());
            for dir in read_dir {
                to_search.push_back(SearchDirectory::new(dir, file.depth - 1))
            }
        }
    }
    Ok(repos)
}

fn get_repo_name(path: &Path, repos: &impl RepoContainer) -> Result<String, TmsError> {
    let mut repo_name = path
        .file_name()
        .expect("The file name doesn't end in `..`")
        .to_string()?;

    repo_name = if repos.find_repo(&repo_name).is_some() {
        if let Some(parent) = path.parent() {
            if let Some(parent) = parent.file_name() {
                format!("{}/{}", parent.to_string()?, repo_name)
            } else {
                repo_name
            }
        } else {
            repo_name
        }
    } else {
        repo_name
    };

    Ok(repo_name)
}

fn find_submodules(
    submodules: Vec<Submodule>,
    parent_name: &String,
    repos: &mut impl RepoContainer,
    display_full_path: Option<bool>,
    recursive: Option<bool>,
) -> Result<(), TmsError> {
    for submodule in submodules.iter() {
        let repo = match submodule.open() {
            Ok(repo) => repo,
            _ => continue,
        };
        let path = match repo.workdir() {
            Some(path) => path,
            _ => continue,
        };
        let submodule_file_name = path
            .file_name()
            .expect("The file name doesn't end in `..`")
            .to_string()?;
        let name = if let Some(true) = display_full_path {
            path.to_string()?
        } else {
            format!("{}>{}", parent_name, submodule_file_name)
        };

        if recursive == Some(true) {
            if let Ok(submodules) = repo.submodules() {
                find_submodules(submodules, &name, repos, display_full_path, recursive)?;
            }
        }
        repos.insert_repo(name, repo);
    }
    Ok(())
}
