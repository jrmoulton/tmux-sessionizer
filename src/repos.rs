use aho_corasick::{AhoCorasickBuilder, MatchKind};
use error_stack::{report, Report, ResultExt};
use gix::{worktree::Proxy, Submodule};
use std::{
    collections::{HashMap, VecDeque},
    fs,
};

use crate::{
    configs::{Config, SearchDirectory},
    dirty_paths::DirtyUtf8Path,
    session::{Session, SessionContainer, SessionType},
    Result, TmsError,
};

pub trait Prunable {
    fn is_prunable(&self) -> bool;
}

impl Prunable for Proxy<'_> {
    fn is_prunable(&self) -> bool {
        !self.base().is_ok_and(|path| path.exists())
    }
}

pub fn find_repos(config: &Config) -> Result<HashMap<String, Vec<Session>>> {
    let directories = config.search_dirs().change_context(TmsError::ConfigError)?;
    let mut repos: HashMap<String, Vec<Session>> = HashMap::new();
    let mut to_search: VecDeque<SearchDirectory> = directories.into();

    let excluder = if let Some(excluded_dirs) = &config.excluded_dirs {
        Some(
            AhoCorasickBuilder::new()
                .match_kind(MatchKind::LeftmostFirst)
                .build(excluded_dirs)
                .change_context(TmsError::IoError)?,
        )
    } else {
        None
    };

    while let Some(file) = to_search.pop_front() {
        if let Some(ref excluder) = excluder {
            if excluder.is_match(&file.path.to_string()?) {
                continue;
            }
        }

        if let Ok(repo) = gix::open(&file.path) {
            if !repo.main_repo().is_ok_and(|r| r == repo) {
                continue;
            }

            let session_name = file
                .path
                .file_name()
                .ok_or_else(|| {
                    Report::new(TmsError::GitError).attach_printable("Not a valid repository name")
                })?
                .to_string()?;

            let session = Session::new(session_name, SessionType::Git(repo));
            if let Some(list) = repos.get_mut(&session.name) {
                list.push(session);
            } else {
                repos.insert(session.name.clone(), vec![session]);
            }
        } else if file.path.is_dir() && file.depth > 0 {
            match fs::read_dir(&file.path) {
                Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    eprintln!(
                        "Warning: insufficient permissions to read '{0}'. Skipping directory...",
                        file.path.to_string()?
                    );
                }
                Err(e) => {
                    let report = report!(e)
                        .change_context(TmsError::IoError)
                        .attach_printable(format!("Could not read directory {:?}", file.path));
                    return Err(report);
                }
                Ok(read_dir) => {
                    let mut subdirs = read_dir
                        .filter_map(|dir_entry| {
                            if let Ok(dir) = dir_entry {
                                Some(SearchDirectory::new(dir.path(), file.depth - 1))
                            } else {
                                None
                            }
                        })
                        .collect::<VecDeque<SearchDirectory>>();

                    if !subdirs.is_empty() {
                        to_search.append(&mut subdirs);
                    }
                }
            }
        }
    }
    Ok(repos)
}

pub fn find_submodules<'a>(
    submodules: impl Iterator<Item = Submodule<'a>>,
    parent_name: &String,
    repos: &mut impl SessionContainer,
    config: &Config,
) -> Result<()> {
    for submodule in submodules {
        let repo = match submodule.open() {
            Ok(Some(repo)) => repo,
            _ => continue,
        };
        let path = match repo.work_dir() {
            Some(path) => path,
            _ => continue,
        };
        let submodule_file_name = path
            .file_name()
            .ok_or_else(|| {
                Report::new(TmsError::GitError).attach_printable("Not a valid submodule name")
            })?
            .to_string()?;
        let session_name = format!("{}>{}", parent_name, submodule_file_name);
        let name = if let Some(true) = config.display_full_path {
            path.display().to_string()
        } else {
            session_name.clone()
        };

        if config.recursive_submodules == Some(true) {
            if let Ok(Some(submodules)) = repo.submodules() {
                find_submodules(submodules, &name, repos, config)?;
            }
        }
        let session = Session::new(session_name, SessionType::Git(repo));
        repos.insert_session(name, session);
    }
    Ok(())
}
