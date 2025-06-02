use aho_corasick::{AhoCorasickBuilder, MatchKind};
use error_stack::{report, Report, ResultExt};
use gix::{Repository, Submodule};
use jj_lib::{
    config::StackedConfig,
    git_backend::GitBackend,
    local_working_copy::{LocalWorkingCopy, LocalWorkingCopyFactory},
    repo::StoreFactories,
    settings::UserSettings,
    workspace::{WorkingCopyFactories, Workspace},
};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self},
    path::{Path, PathBuf},
};

use crate::{
    configs::{Config, SearchDirectory, VcsProviders, DEFAULT_VCS_PROVIDERS},
    dirty_paths::DirtyUtf8Path,
    session::{Session, SessionContainer, SessionType},
    Result, TmsError,
};

pub trait Worktree {
    fn name(&self) -> String;

    fn path(&self) -> Result<PathBuf>;

    fn is_prunable(&self) -> bool;
}

impl Worktree for gix::worktree::Proxy<'_> {
    fn name(&self) -> String {
        self.id().to_string()
    }

    fn path(&self) -> Result<PathBuf> {
        self.base().change_context(TmsError::GitError)
    }

    fn is_prunable(&self) -> bool {
        !self.base().is_ok_and(|path| path.exists())
    }
}

impl Worktree for Workspace {
    fn name(&self) -> String {
        self.working_copy().workspace_name().as_str().to_string()
    }

    fn path(&self) -> Result<PathBuf> {
        Ok(self.workspace_root().to_path_buf())
    }

    fn is_prunable(&self) -> bool {
        false
    }
}

pub enum RepoProvider {
    Git(Repository),
    Jujutsu(Workspace),
}

impl From<gix::Repository> for RepoProvider {
    fn from(repo: gix::Repository) -> Self {
        Self::Git(repo)
    }
}

impl RepoProvider {
    pub fn open(path: &Path, config: &Config) -> Result<Self> {
        fn open_git(path: &Path) -> Result<RepoProvider> {
            gix::open(path)
                .map(RepoProvider::Git)
                .change_context(TmsError::GitError)
        }

        fn open_jj(path: &Path) -> Result<RepoProvider> {
            let user_settings = UserSettings::from_config(StackedConfig::with_defaults())
                .change_context(TmsError::GitError)?;
            let mut store_factories = StoreFactories::default();
            store_factories.add_backend(
                GitBackend::name(),
                Box::new(|settings, store_path| {
                    Ok(Box::new(GitBackend::load(settings, store_path)?))
                }),
            );
            let mut working_copy_factories = WorkingCopyFactories::new();
            working_copy_factories.insert(
                LocalWorkingCopy::name().to_owned(),
                Box::new(LocalWorkingCopyFactory {}),
            );

            Workspace::load(
                &user_settings,
                path,
                &store_factories,
                &working_copy_factories,
            )
            .map(RepoProvider::Jujutsu)
            .change_context(TmsError::GitError)
        }

        let vcs_provider_config = config
            .vcs_providers
            .as_ref()
            .map(|providers| providers.iter())
            .unwrap_or(DEFAULT_VCS_PROVIDERS.iter());

        let results = vcs_provider_config
            .filter_map(|provider| match provider {
                VcsProviders::Git => open_git(path).ok(),
                VcsProviders::Jujutsu => open_jj(path).ok(),
            })
            .take(1);
        results
            .into_iter()
            .next()
            .ok_or(TmsError::GitError)
            .change_context(TmsError::GitError)
    }

    pub fn is_worktree(&self) -> bool {
        match self {
            RepoProvider::Git(repo) => !repo.main_repo().is_ok_and(|r| r == *repo),
            RepoProvider::Jujutsu(repo) => {
                let repo_path = repo.repo_path();
                let workspace_repo_path = repo.workspace_root().join(".jj/repo");
                repo_path != workspace_repo_path
            }
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            RepoProvider::Git(repo) => repo.path(),
            RepoProvider::Jujutsu(repo) => repo.workspace_root(),
        }
    }

    pub fn main_repo(&self) -> Option<PathBuf> {
        match self {
            RepoProvider::Git(repo) => repo.main_repo().map(|repo| repo.path().to_path_buf()).ok(),
            RepoProvider::Jujutsu(repo) => Some(repo.repo_path().to_path_buf()),
        }
    }

    pub fn work_dir(&self) -> Option<&Path> {
        match self {
            RepoProvider::Git(repo) => repo.work_dir(),
            RepoProvider::Jujutsu(repo) => Some(repo.workspace_root()),
        }
    }

    pub fn head_name(&self) -> Result<String> {
        match self {
            RepoProvider::Git(repo) => Ok(repo
                .head_name()
                .change_context(TmsError::GitError)?
                .ok_or(TmsError::GitError)?
                .shorten()
                .to_string()),
            RepoProvider::Jujutsu(_) => Err(TmsError::GitError.into()),
        }
    }
    pub fn submodules(&'_ self) -> Result<Option<impl Iterator<Item = Submodule<'_>>>> {
        match self {
            RepoProvider::Git(repo) => repo.submodules().change_context(TmsError::GitError),
            RepoProvider::Jujutsu(_) => Ok(None),
        }
    }

    pub fn is_bare(&self) -> bool {
        match self {
            RepoProvider::Git(repo) => repo.is_bare(),
            RepoProvider::Jujutsu(_) => false,
        }
    }

    pub fn worktrees(&'_ self, config: &Config) -> Result<Vec<Box<dyn Worktree + '_>>> {
        match self {
            RepoProvider::Git(repo) => Ok(repo
                .worktrees()
                .change_context(TmsError::GitError)?
                .into_iter()
                .map(|i| Box::new(i) as Box<dyn Worktree>)
                .collect()),

            RepoProvider::Jujutsu(workspace) => {
                let mut repos: Vec<RepoProvider> = Vec::new();

                search_dirs(config, |_, repo| {
                    if !repo.is_worktree() {
                        return Ok(());
                    }
                    let Some(path) = repo.main_repo() else {
                        return Ok(());
                    };
                    if workspace.repo_path() == path {
                        repos.push(repo);
                    }
                    Ok(())
                })?;

                let repos = repos
                    .into_iter()
                    .filter_map(|repo| match repo {
                        RepoProvider::Jujutsu(r) => Some(r),
                        _ => None,
                    })
                    .map(|i| Box::new(i) as Box<dyn Worktree>)
                    .collect();
                Ok(repos)
            }
        }
    }
}

pub fn find_repos(config: &Config) -> Result<HashMap<String, Vec<Session>>> {
    let mut repos: HashMap<String, Vec<Session>> = HashMap::new();

    search_dirs(config, |file, repo| {
        if repo.is_worktree() {
            return Ok(());
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
        Ok(())
    })?;
    Ok(repos)
}

fn search_dirs<F>(config: &Config, mut f: F) -> Result<()>
where
    F: FnMut(SearchDirectory, RepoProvider) -> Result<()>,
{
    {
        let directories = config.search_dirs().change_context(TmsError::ConfigError)?;
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

            if let Ok(repo) = RepoProvider::open(&file.path, config) {
                f(file, repo)?;
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
        Ok(())
    }
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
        let session = Session::new(session_name, SessionType::Git(repo.into()));
        repos.insert_session(name, session);
    }
    Ok(())
}
