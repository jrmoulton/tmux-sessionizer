#[derive(Default)]
pub struct Repos {
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
    pub fn new() -> Self {
        Self::default()
    }
    pub fn push(&mut self, repo: git2::Repository) {
        self.repos.push(repo);
    }
    pub fn find(&self, name: &str) -> Option<&git2::Repository> {
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
