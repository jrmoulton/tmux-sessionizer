use std::{fs, path::PathBuf, process::Command};

use tempfile::tempdir;
use tms::configs::{Config, SearchDirectory, VcsProviders};
use tms::repos::{find_repos, LazyRepoProvider};

fn config_searching(path: PathBuf, depth: usize) -> Config {
    Config {
        search_dirs: Some(vec![SearchDirectory::new(path, depth)]),
        ..Default::default()
    }
}

fn git_commit(repo: &std::path::Path) {
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "tms-test")
        .env("GIT_AUTHOR_EMAIL", "tms-test@example.com")
        .env("GIT_COMMITTER_NAME", "tms-test")
        .env("GIT_COMMITTER_EMAIL", "tms-test@example.com")
        .status()
        .expect("git commit");
}

#[test]
fn find_repos_includes_gitlink_project() {
    let dir = tempdir().unwrap();
    let search = dir.path().join("search");
    let project = search.join("my-project");
    fs::create_dir_all(&project).unwrap();
    let search = fs::canonicalize(&search).unwrap();
    Command::new("git")
        .args(["init", "--bare"])
        .arg(project.join(".bare"))
        .status()
        .unwrap();
    fs::write(project.join(".git"), "gitdir: .bare\n").unwrap();

    let repos = find_repos(&config_searching(search, 2)).unwrap();

    assert!(
        repos.contains_key("my-project"),
        "expected gitlink project in picker results, got: {:?}",
        repos.keys().collect::<Vec<_>>()
    );
}

#[test]
fn find_repos_excludes_linked_worktree() {
    let dir = tempdir().unwrap();
    let search = dir.path().join("search");
    fs::create_dir_all(&search).unwrap();
    let search = fs::canonicalize(&search).unwrap();
    let main_repo = search.join("main-repo");
    let linked = search.join("linked");
    fs::create_dir_all(&main_repo).unwrap();

    Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(&main_repo)
        .status()
        .unwrap();
    git_commit(&main_repo);
    Command::new("git")
        .args(["worktree", "add", "-b", "linked"])
        .arg(&linked)
        .current_dir(&main_repo)
        .status()
        .unwrap();

    let linked_repo = LazyRepoProvider::new(&linked, &[VcsProviders::Git]).unwrap();
    assert!(
        linked_repo.is_worktree().unwrap(),
        "linked checkout should be classified as a worktree"
    );

    let repos = find_repos(&config_searching(search, 2)).unwrap();

    assert!(
        repos.contains_key("main-repo"),
        "expected main repository in picker results, got: {:?}",
        repos.keys().collect::<Vec<_>>()
    );
    assert!(
        !repos.contains_key("linked"),
        "linked worktree should not appear in picker results, got: {:?}",
        repos.keys().collect::<Vec<_>>()
    );
}
