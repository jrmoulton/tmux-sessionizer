use git2::Repository;
use std::collections::HashMap;

pub trait RepoContainer {
    fn repo_string(&self) -> String;
    fn find_repo(&self, name: &str) -> Option<&Repository>;
    fn insert_repo(&mut self, name: String, repo: Repository);
}

impl RepoContainer for HashMap<String, Repository> {
    fn repo_string(&self) -> String {
        let mut return_string = String::new();
        for name in self.keys() {
            return_string.push_str(&format!("{}\n", name));
        }
        return_string
    }

    fn find_repo(&self, name: &str) -> Option<&Repository> {
        self.get(name)
    }

    fn insert_repo(&mut self, name: String, repo: Repository) {
        self.insert(name, repo);
    }
}
