use aho_corasick::{AhoCorasickBuilder, MatchKind};
use error_stack::ResultExt;
use git2::Submodule;
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
        if should_skip_file(&file, &excluder)? {
            continue;
        }

        if let Ok(repo) = git2::Repository::open(file.path.clone()) {
            process_repository(&file, repo, &mut repos, config)?;
        } else if should_search_directory(&file) {
            process_directory(&file, &mut to_search)?;
        }
    }
    Ok(repos)
}

fn should_skip_file(
    file: &SearchDirectory,
    excluder: &Option<aho_corasick::AhoCorasick>,
) -> Result<bool> {
    if let Some(ref excluder) = excluder {
        if excluder.is_match(&file.path.to_string()?) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn should_search_directory(file: &SearchDirectory) -> bool {
    file.path.is_dir() && file.depth > 0
}

fn process_bare_repository(
    file: &SearchDirectory,
    repos: &mut HashMap<String, Vec<Session>>,
) -> Result<()> {
    match fs::read_dir(&file.path) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    let git_file = entry_path.join(".git");
                    if git_file.exists() && git_file.is_file() {
                        if let Ok(worktree_repo) = git2::Repository::open(&entry_path) {
                            let session_name = entry_path
                                .file_name()
                                .expect("The file name doesn't end in `..`")
                                .to_string_lossy()
                                .to_string();

                            let parent = file
                                .path
                                .file_name()
                                .expect("The file name doesn't end in `..`")
                                .to_string_lossy()
                                .to_string();
                            let session = Session::new(
                                format!("{}#{}", parent, session_name.clone()),
                                SessionType::Git(worktree_repo),
                            );
                            repos.insert(session_name, vec![session]);
                        }
                    }
                }
            }
        }
        Err(e) => {
            eprintln!(
                "Warning: couldn't read bare repository directory '{}': {}",
                file.path.to_string_lossy(),
                e
            );
        }
    }
    Ok(())
}

fn process_repository(
    file: &SearchDirectory,
    repo: git2::Repository,
    repos: &mut HashMap<String, Vec<Session>>,
    config: &Config,
) -> Result<()> {
    if repo.is_worktree() {
        return Ok(());
    }

    if repo.is_bare() && config.list_worktrees == Some(true) {
        process_bare_repository(file, repos)?;
        return Ok(());
    }

    let session_name = file
        .path
        .file_name()
        .expect("The file name doesn't end in `..`")
        .to_string()?;

    let session = Session::new(session_name.clone(), SessionType::Git(repo));
    if let Some(list) = repos.get_mut(&session.name) {
        list.push(session);
    } else {
        repos.insert(session.name.clone(), vec![session]);
    }
    Ok(())
}

fn process_directory(
    file: &SearchDirectory,
    to_search: &mut VecDeque<SearchDirectory>,
) -> Result<()> {
    match fs::read_dir(&file.path) {
        Err(ref e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!(
                "Warning: insufficient permissions to read '{0}'. Skipping directory...",
                file.path.to_string()?
            );
            Ok(())
        }
        result => {
            let read_dir = result
                .change_context(TmsError::IoError)
                .attach_printable_lazy(|| format!("Could not read directory {:?}", file.path))?
                .map(|dir_entry| dir_entry.expect("Found non-valid utf8 path").path());

            for dir in read_dir {
                to_search.push_back(SearchDirectory::new(dir, file.depth - 1));
            }
            Ok(())
        }
    }
}

pub fn find_submodules(
    submodules: Vec<Submodule>,
    parent_name: &String,
    repos: &mut impl SessionContainer,
    config: &Config,
) -> Result<()> {
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
        let session_name = format!("{}>{}", parent_name, submodule_file_name);
        let name = if let Some(true) = config.display_full_path {
            path.display().to_string()
        } else {
            session_name.clone()
        };

        if config.recursive_submodules == Some(true) {
            if let Ok(submodules) = repo.submodules() {
                find_submodules(submodules, &name, repos, config)?;
            }
        }
        let session = Session::new(session_name, SessionType::Git(repo));
        repos.insert_session(name, session);
    }
    Ok(())
}
