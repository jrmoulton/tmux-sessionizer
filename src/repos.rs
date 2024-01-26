use git2::Repository;
use std::collections::HashMap;

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
