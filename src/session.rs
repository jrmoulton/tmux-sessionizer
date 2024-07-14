use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use error_stack::ResultExt;
use git2::Repository;

use crate::{
    configs::Config,
    dirty_paths::DirtyUtf8Path,
    error::TmsError,
    repos::{find_repos, find_submodules},
    tmux::Tmux,
    Result,
};

pub struct Session {
    pub name: String,
    pub session_type: SessionType,
}

pub enum SessionType {
    Git(Repository),
    Bookmark(PathBuf),
}

impl Session {
    pub fn new(name: String, session_type: SessionType) -> Self {
        Session { name, session_type }
    }

    pub fn path(&self) -> &Path {
        match &self.session_type {
            SessionType::Git(repo) if repo.is_bare() => repo.path(),
            SessionType::Git(repo) => repo.path().parent().unwrap(),
            SessionType::Bookmark(path) => path,
        }
    }

    pub fn switch_to(&self, tmux: &Tmux) -> Result<()> {
        match &self.session_type {
            SessionType::Git(repo) => switch_to_repo_session(&self.name, repo, tmux),
            SessionType::Bookmark(path) => switch_to_bookmark_session(&self.name, tmux, path),
        }
    }
}

pub trait SessionContainer {
    fn find_session(&self, name: &str) -> Option<&Session>;
    fn insert_session(&mut self, name: String, repo: Session);
    fn list(&self) -> Vec<String>;
}

impl SessionContainer for HashMap<String, Session> {
    fn find_session(&self, name: &str) -> Option<&Session> {
        self.get(name)
    }

    fn insert_session(&mut self, name: String, session: Session) {
        self.insert(name, session);
    }

    fn list(&self) -> Vec<String> {
        let mut list: Vec<String> = self.keys().map(|s| s.to_owned()).collect();
        list.sort();

        list
    }
}

pub fn create_sessions(config: &Config) -> Result<impl SessionContainer> {
    let mut sessions = find_repos(config)?;
    sessions = append_bookmarks(config, sessions)?;

    let sessions = generate_session_container(sessions, config)?;

    Ok(sessions)
}

fn generate_session_container(
    mut sessions: HashMap<String, Vec<Session>>,
    config: &Config,
) -> Result<impl SessionContainer> {
    let mut ret = HashMap::new();

    for list in sessions.values_mut() {
        if list.len() == 1 {
            let session = list.pop().unwrap();
            insert_session(&mut ret, session, config)?;
        } else {
            let deduplicated = deduplicate_sessions(list);

            for session in deduplicated {
                insert_session(&mut ret, session, config)?;
            }
        }
    }

    Ok(ret)
}

fn insert_session(
    sessions: &mut impl SessionContainer,
    session: Session,
    config: &Config,
) -> Result<()> {
    let visible_name = if config.display_full_path == Some(true) {
        session.path().display().to_string()
    } else {
        session.name.clone()
    };
    if let SessionType::Git(repo) = &session.session_type {
        if config.search_submodules == Some(true) {
            if let Ok(submodules) = repo.submodules() {
                find_submodules(submodules, &visible_name, sessions, config)?;
            }
        }
    }
    sessions.insert_session(visible_name, session);
    Ok(())
}

fn deduplicate_sessions(duplicate_sessions: &mut Vec<Session>) -> Vec<Session> {
    let mut depth = 1;
    let mut deduplicated = Vec::new();
    while let Some(current_session) = duplicate_sessions.pop() {
        let mut equal = true;
        let current_path = current_session.path();
        let mut current_depth = 1;

        while equal {
            equal = false;
            if let Some(current_str) = current_path.iter().rev().nth(current_depth) {
                for session in &mut *duplicate_sessions {
                    if let Some(str) = session.path().iter().rev().nth(current_depth) {
                        if str == current_str {
                            current_depth += 1;
                            equal = true;
                            break;
                        }
                    }
                }
            }
        }

        deduplicated.push(current_session);
        depth = depth.max(current_depth);
    }

    for session in &mut deduplicated {
        session.name = {
            let mut count = depth + 1;
            let mut iterator = session.path().iter().rev();
            let mut str = String::new();

            while count > 0 {
                if let Some(dir) = iterator.next() {
                    if str.is_empty() {
                        str = dir.to_string_lossy().to_string();
                    } else {
                        str = format!("{}/{}", dir.to_string_lossy(), str);
                    }
                    count -= 1;
                } else {
                    count = 0;
                }
            }

            str
        };
    }

    deduplicated
}

fn append_bookmarks(
    config: &Config,
    mut sessions: HashMap<String, Vec<Session>>,
) -> Result<HashMap<String, Vec<Session>>> {
    let bookmarks = config.bookmark_paths();

    for path in bookmarks {
        let session_name = path
            .file_name()
            .expect("The file name doesn't end in `..`")
            .to_string()?;
        let session = Session::new(session_name, SessionType::Bookmark(path));
        if let Some(list) = sessions.get_mut(&session.name) {
            list.push(session);
        } else {
            sessions.insert(session.name.clone(), vec![session]);
        }
    }

    Ok(sessions)
}

fn switch_to_repo_session(selected_str: &str, found_repo: &Repository, tmux: &Tmux) -> Result<()> {
    let path = if found_repo.is_bare() {
        found_repo.path().to_string()?
    } else {
        found_repo
            .workdir()
            .expect("bare repositories should all have parent directories")
            .canonicalize()
            .change_context(TmsError::IoError)?
            .to_string()?
    };
    let repo_short_name = selected_str.replace('.', "_");

    if !tmux.session_exists(&repo_short_name) {
        tmux.new_session(Some(&repo_short_name), Some(&path));
        tmux.set_up_tmux_env(found_repo, &repo_short_name)?;
    }

    tmux.switch_to_session(&repo_short_name);

    Ok(())
}

fn switch_to_bookmark_session(selected_str: &str, tmux: &Tmux, path: &Path) -> Result<()> {
    let session_name = selected_str.replace('.', "_");

    if !tmux.session_exists(&session_name) {
        tmux.new_session(Some(&session_name), path.to_str());
    }

    tmux.switch_to_session(&session_name);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_session_name_deduplication() {
        let mut test_sessions = vec![
            Session::new(
                "test".into(),
                SessionType::Bookmark("/search/path/to/proj1/test".into()),
            ),
            Session::new(
                "test".into(),
                SessionType::Bookmark("/search/path/to/proj2/test".into()),
            ),
            Session::new(
                "test".into(),
                SessionType::Bookmark("/other/path/to/projects/proj2/test".into()),
            ),
        ];

        let deduplicated = deduplicate_sessions(&mut test_sessions);

        assert_eq!(deduplicated[0].name, "projects/proj2/test");
        assert_eq!(deduplicated[1].name, "to/proj2/test");
        assert_eq!(deduplicated[2].name, "to/proj1/test");
    }
}
