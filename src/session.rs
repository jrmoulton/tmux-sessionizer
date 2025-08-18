use std::{
    collections::HashMap,
    path::PathBuf,
};

use crate::{
    configs::Config,
    repos::{find_submodules, RepoProvider},
    Result,
};

#[derive(Clone)]
pub struct Session {
    pub name: String,
    pub path: PathBuf,
    pub session_type: SessionType,
}

#[derive(Clone)]
pub enum SessionType {
    Git,
    Path,
}

impl Session {
    pub fn new(name: String, path: PathBuf, session_type: SessionType) -> Self {
        Session {
            name,
            path,
            session_type,
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

pub fn generate_session_container(
    mut sessions: HashMap<String, Vec<Session>>,
    config: &Config,
) -> Result<HashMap<String, Session>> {
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
        session.path.display().to_string()
    } else {
        session.name.clone()
    };
    if let SessionType::Git = &session.session_type {
        if config.search_submodules == Some(true) {
            if let Ok(repo) = RepoProvider::open(&session.path, config) {
                if let Ok(Some(submodules)) = repo.submodules() {
                    find_submodules(submodules, &visible_name, sessions, config)?;
                }
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
        let current_path = &current_session.path;
        let mut current_depth = 1;

        while equal {
            equal = false;
            if let Some(current_str) = current_path.iter().rev().nth(current_depth) {
                for session in &mut *duplicate_sessions {
                    if let Some(str) = session.path.iter().rev().nth(current_depth) {
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
            let mut iterator = session.path.iter().rev();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_session_name_deduplication() {
        let mut test_sessions = vec![
            Session::new(
                "test".into(),
                "/search/path/to/proj1/test".into(),
                SessionType::Path,
            ),
            Session::new(
                "test".into(),
                "/search/path/to/proj2/test".into(),
                SessionType::Path,
            ),
            Session::new(
                "test".into(),
                "/other/path/to/projects/proj2/test".into(),
                SessionType::Path,
            ),
        ];

        let deduplicated = deduplicate_sessions(&mut test_sessions);

        assert_eq!(deduplicated[0].name, "projects/proj2/test");
        assert_eq!(deduplicated[1].name, "to/proj2/test");
        assert_eq!(deduplicated[2].name, "to/proj1/test");
    }
}
