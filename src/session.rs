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
    Standard(PathBuf),
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
            SessionType::Standard(path) => path,
        }
    }

    pub fn switch_to(&self, tmux: &Tmux, config: &Config) -> Result<()> {
        match &self.session_type {
            SessionType::Git(repo) => self.switch_to_repo_session(repo, tmux, config),
            SessionType::Bookmark(path) => self.switch_to_bookmark_session(tmux, path, config),
            SessionType::Standard(path) => self.switch_to_standard_session(tmux, path, config),
        }
    }

    fn switch_to_repo_session(
        &self,
        repo: &Repository,
        tmux: &Tmux,
        config: &Config,
    ) -> Result<()> {
        let path = if repo.is_bare() {
            repo.path().to_path_buf().to_string()?
        } else {
            repo.workdir()
                .expect("bare repositories should all have parent directories")
                .canonicalize()
                .change_context(TmsError::IoError)?
                .to_string()?
        };
        let session_name = self.name.replace('.', "_");

        if !tmux.session_exists(&session_name) {
            tmux.new_session(Some(&session_name), Some(&path));
            tmux.set_up_tmux_env(repo, &session_name)?;
            tmux.run_session_create_script(self.path(), &session_name, config)?;
        }

        tmux.switch_to_session(&session_name);

        Ok(())
    }

    fn switch_to_bookmark_session(&self, tmux: &Tmux, path: &Path, config: &Config) -> Result<()> {
        let session_name = self.name.replace('.', "_");

        if !tmux.session_exists(&session_name) {
            tmux.new_session(Some(&session_name), path.to_str());
            tmux.run_session_create_script(path, &session_name, config)?;
        }

        tmux.switch_to_session(&session_name);

        Ok(())
    }

    fn switch_to_standard_session(
        &self,
        tmux: &Tmux,
        path: &PathBuf,
        config: &Config,
    ) -> Result<()> {
        let session_name = self.name.to_string();

        if !tmux.session_exists(&session_name) {
            tmux.new_session(Some(&session_name), path.to_str());
            tmux.run_session_create_script(path, &session_name, config)?;
        }

        tmux.switch_to_session(&session_name);
        Ok(())
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

pub fn create_repo_sessions(config: &Config) -> Result<impl SessionContainer> {
    let mut sessions = find_repos(config)?;
    sessions = append_bookmarks(config, sessions)?;

    let sessions = generate_session_container(sessions, config)?;

    Ok(sessions)
}

pub fn create_all_sessions(config: &Config, tmux: &Tmux) -> Result<impl SessionContainer> {
    let repo_sessions = find_repos(config)?;
    let tmux_sessions = tmux.find_tmux_sessions()?;

    // If session already exists through tmux, dont recommend as a new repo_session
    let repo_sessions_filtered: HashMap<String, Vec<Session>> = repo_sessions
        .into_iter()
        .filter(|(k, _v)| !tmux_sessions.contains_key(k))
        .collect();

    let all_sessions = merge_session_maps(repo_sessions_filtered, tmux_sessions);

    generate_session_container(all_sessions, config)
}

pub fn generate_session_container(
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

pub fn append_bookmarks(
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

fn merge_session_maps(
    mut s1: HashMap<String, Vec<Session>>,
    mut s2: HashMap<String, Vec<Session>>,
) -> HashMap<String, Vec<Session>> {
    let mut ret: HashMap<String, Vec<Session>> = HashMap::new();

    for (key, mut new_sessions) in s1.drain() {
        ret.entry(key)
            .or_insert_with(Vec::new)
            .append(&mut new_sessions);
    }
    for (key, mut new_sessions) in s2.drain() {
        ret.entry(key)
            .or_insert_with(Vec::new)
            .append(&mut new_sessions);
    }
    ret
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

    #[test]
    fn test_merge_session_maps_non_overlapping() {
        let mut map1 = HashMap::new();
        let mut map2 = HashMap::new();

        let session_a = Session::new(
            "Session A".to_string(),
            SessionType::Standard(PathBuf::from("/path/to/a")),
        );
        let session_b = Session::new(
            "Session B".to_string(),
            SessionType::Standard(PathBuf::from("/path/to/b")),
        );

        map1.insert("key1".to_string(), vec![session_a]);
        map2.insert("key2".to_string(), vec![session_b]);

        let merged = merge_session_maps(map1, map2);

        // Expect both keys to exist independently.
        assert_eq!(merged.len(), 2);
        assert!(merged.contains_key("key1"));
        assert!(merged.contains_key("key2"));
        assert_eq!(merged["key1"].len(), 1);
        assert_eq!(merged["key2"].len(), 1);
    }

    #[test]
    fn test_merge_session_maps_overlapping() {
        let mut map1 = HashMap::new();
        let mut map2 = HashMap::new();

        let session_a = Session::new(
            "Session A".to_string(),
            SessionType::Standard(PathBuf::from("/path/to/a")),
        );
        let session_b = Session::new(
            "Session B".to_string(),
            SessionType::Standard(PathBuf::from("/path/to/b")),
        );

        // Both maps have the same key "shared_key"
        map1.insert("shared_key".to_string(), vec![session_a]);
        map2.insert("shared_key".to_string(), vec![session_b]);

        let merged = merge_session_maps(map1, map2);

        // Expect one key "shared_key" with both sessions, map1's session first.
        assert_eq!(merged.len(), 1);
        let sessions = merged.get("shared_key").unwrap();
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "Session A");
        assert_eq!(sessions[1].name, "Session B");
    }

    #[test]
    fn test_merge_session_maps_empty() {
        let map1: HashMap<String, Vec<Session>> = HashMap::new();
        let map2: HashMap<String, Vec<Session>> = HashMap::new();

        let merged = merge_session_maps(map1, map2);

        // Expect an empty map.
        assert!(merged.is_empty());
    }
}
