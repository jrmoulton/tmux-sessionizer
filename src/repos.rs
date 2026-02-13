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
    workspace_store::{SimpleWorkspaceStore, WorkspaceStore},
};
use once_cell::sync::OnceCell;
use std::{
    collections::{HashMap, VecDeque},
    fs::{self},
    path::{Path, PathBuf},
    process::{self, Stdio},
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

impl VcsProviders {
    pub fn new<'a, I>(path: &Path, providers: I) -> Result<Self>
    where
        I: IntoIterator<Item = &'a VcsProviders>,
    {
        providers
            .into_iter()
            .filter_map(|provider| match provider {
                VcsProviders::Git => {
                    let mut flags = 0u8;
                    for entry in path.read_dir().ok()? {
                        let entry = entry.ok()?;
                        let name = entry.file_name();
                        let file_type = entry.file_type().ok()?;

                        match name.to_str() {
                            Some(".git") => {
                                return Some(VcsProviders::Git);
                            }
                            Some("HEAD") if file_type.is_file() => flags |= 0b001,
                            Some("objects") if file_type.is_dir() => flags |= 0b010,
                            Some("refs") if file_type.is_dir() => flags |= 0b100,
                            _ => {}
                        }
                        if flags == 0b111 {
                            return Some(VcsProviders::Git);
                        }
                    }
                    None
                }
                VcsProviders::Jujutsu => path
                    .join(".jj/repo")
                    .exists()
                    .then_some(VcsProviders::Jujutsu),
            })
            .next()
            .ok_or(TmsError::GitError)
            .attach_printable_lazy(|| format!("No repo found in {:#?}", path))
    }

    pub fn open(&self, path: &Path) -> Result<RepoProvider> {
        match self {
            VcsProviders::Git => gix::open(path)
                .map(|repo| RepoProvider::Git(Box::new(repo)))
                .change_context(TmsError::GitError),
            VcsProviders::Jujutsu => {
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
        }
    }
}

pub struct LazyRepoProvider {
    pub path: PathBuf,
    pub provider: VcsProviders,
    resolved: OnceCell<RepoProvider>,
}

impl LazyRepoProvider {
    pub fn new<'a, I>(path: &Path, providers: I) -> Result<Self>
    where
        I: IntoIterator<Item = &'a VcsProviders>,
    {
        let provider = VcsProviders::new(path, providers)?;
        Ok(Self {
            path: path.to_path_buf(),
            provider,
            resolved: OnceCell::new(),
        })
    }

    pub fn new_resolved(path: &Path, provider: VcsProviders, repo: RepoProvider) -> Self {
        Self {
            path: path.to_path_buf(),
            provider,
            resolved: OnceCell::with_value(repo),
        }
    }

    pub fn resolve(&self) -> Result<&RepoProvider> {
        self.resolved
            .get_or_try_init(|| self.provider.open(&self.path))
    }

    pub fn is_worktree(&self) -> bool {
        match self.provider {
            VcsProviders::Git => self.path.join(".git").is_file(),
            VcsProviders::Jujutsu => self.path.join(".jj/repo").is_file(),
        }
    }
}

pub enum RepoProvider {
    Git(Box<Repository>),
    Jujutsu(Workspace),
}

impl From<gix::Repository> for RepoProvider {
    fn from(repo: gix::Repository) -> Self {
        Self::Git(Box::new(repo))
    }
}

impl RepoProvider {
    pub fn open(path: &Path, config: &Config) -> Result<Self> {
        let vcs_provider_config = config
            .vcs_providers
            .clone()
            .unwrap_or_else(|| DEFAULT_VCS_PROVIDERS.to_vec());
        let provider = VcsProviders::new(path, &vcs_provider_config)?;
        provider.open(path)
    }

    pub fn is_worktree(&self) -> bool {
        match self {
            RepoProvider::Git(repo) => !repo.main_repo().is_ok_and(|r| r == **repo),
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
            RepoProvider::Git(repo) => repo.workdir(),
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
            RepoProvider::Jujutsu(workspace) => {
                let loader = workspace.repo_loader();
                let store = loader.store();
                let Ok(repo) = loader.load_at_head() else {
                    return false;
                };
                // currently checked out commit, get from current (default) workspace
                let Some(commit_id) = repo.view().wc_commit_ids().get(workspace.workspace_name())
                else {
                    return false;
                };
                let Ok(commit) = store.get_commit(commit_id) else {
                    return false;
                };
                // if parent is root commit then it's the only possible parent
                let Some(Ok(parent)) = commit.parents().next() else {
                    return false;
                };

                // root commit is direct parent of current commit => repo is effectively bare
                // current commit should be empty
                parent.change_id() == store.root_commit().change_id()
                    && commit.is_empty(&*repo).unwrap_or_default()
            }
        }
    }

    pub fn add_worktree(&self, path: &Path) -> Result<Option<(String, PathBuf)>> {
        match self {
            RepoProvider::Git(_) => {
                let Ok(head) = self.head_name() else {
                    return Ok(None);
                };
                // Add the default branch as a tree (usually either main or master)
                process::Command::new("git")
                    .current_dir(path)
                    .args(["worktree", "add", &head])
                    .stderr(Stdio::inherit())
                    .output()
                    .change_context(TmsError::GitError)?;
                Ok(Some((head.clone(), path.to_path_buf().join(&head))))
            }
            RepoProvider::Jujutsu(_) => {
                process::Command::new("jj")
                    .current_dir(path)
                    .args(["workspace", "add", "-r", "trunk()", "trunk"])
                    .stderr(Stdio::inherit())
                    .output()
                    .change_context(TmsError::GitError)?;
                Ok(Some(("trunk".into(), path.to_path_buf().join("trunk"))))
            }
        }
    }

    pub fn worktrees(&'_ self) -> Result<Vec<Box<dyn Worktree + '_>>> {
        match self {
            RepoProvider::Git(repo) => Ok(repo
                .worktrees()
                .change_context(TmsError::GitError)?
                .into_iter()
                .map(|i| Box::new(i) as Box<dyn Worktree>)
                .collect()),

            RepoProvider::Jujutsu(workspace) => {
                let repo = workspace
                    .repo_loader()
                    .load_at_head()
                    .change_context(TmsError::GitError)?;
                let workspace_store = SimpleWorkspaceStore::load(workspace.repo_path())
                    .change_context(TmsError::GitError)?;
                let workspaces = repo
                    .view()
                    .wc_commit_ids()
                    .keys()
                    .filter(|name| name.as_str() != workspace.workspace_name().as_str())
                    .map(|name| workspace_store.get_workspace_path(name))
                    .filter_map(|opt| opt.ok().flatten());

                let repos = workspaces
                    .filter_map(|path| {
                        if let Ok(RepoProvider::Jujutsu(workspace)) =
                            VcsProviders::Jujutsu.open(&path)
                        {
                            Some(Box::new(workspace) as Box<dyn Worktree>)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
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
    F: FnMut(SearchDirectory, LazyRepoProvider) -> Result<()>,
{
    {
        let directories = config.search_dirs().change_context(TmsError::ConfigError)?;
        let mut to_search: VecDeque<SearchDirectory> = directories.into();
        let vcs_provider_config = config
            .vcs_providers
            .clone()
            .unwrap_or_else(|| DEFAULT_VCS_PROVIDERS.to_vec());

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

            if let Ok(repo) = LazyRepoProvider::new(&file.path, &vcs_provider_config) {
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
        let path = match repo.workdir() {
            Some(path) => path.to_path_buf(),
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
        let session = Session::new(
            session_name,
            SessionType::Git(LazyRepoProvider::new_resolved(
                &path,
                VcsProviders::Git,
                RepoProvider::Git(Box::new(repo)),
            )),
        );
        repos.insert_session(name, session);
    }
    Ok(())
}
