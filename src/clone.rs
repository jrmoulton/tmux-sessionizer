use std::{
    fmt::Display,
    io::{stdout, Stdout, Write},
    path::Path,
    time::{Duration, Instant},
};

use crate::{error::TmsError, Result};

use crossterm::{cursor, terminal, ExecutableCommand};
use error_stack::ResultExt;
use git2::{build::RepoBuilder, FetchOptions, Progress, RemoteCallbacks, Repository};

const UPDATE_INTERVAL: Duration = Duration::from_millis(300);

struct Rate(usize);

impl Display for Rate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 > 1024 * 1024 {
            let rate = self.0 as f64 / 1024.0 / 1024.0;
            write!(f, "{:.2} MB/s", rate)
        } else {
            let rate = self.0 as f64 / 1024.0;
            write!(f, "{:.2} kB/s", rate)
        }
    }
}

struct CloneSnapshot {
    time: Instant,
    bytes_transferred: usize,
    stdout: Stdout,
    lines: u16,
}

impl CloneSnapshot {
    pub fn new() -> Self {
        let stdout = stdout();
        Self {
            time: Instant::now(),
            bytes_transferred: 0,
            stdout,
            lines: 0,
        }
    }

    pub fn update(&mut self, progress: &Progress) -> Result<()> {
        let now = Instant::now();
        let difference = now.duration_since(self.time);
        if difference < UPDATE_INTERVAL {
            return Ok(());
        }

        let transferred = progress.received_bytes() - self.bytes_transferred;
        let rate = Rate(transferred / (difference.as_millis() as usize) * 1000);

        let network_pct = (100 * progress.received_objects()) / progress.total_objects();
        let index_pct = (100 * progress.indexed_objects()) / progress.total_objects();

        let total = (network_pct + index_pct) / 2;

        if self.lines > 0 {
            self.stdout
                .execute(cursor::MoveUp(self.lines))
                .change_context(TmsError::IoError)?;
            self.stdout
                .execute(terminal::Clear(terminal::ClearType::FromCursorDown))
                .change_context(TmsError::IoError)?;
        }

        let mut lines = 0;

        if network_pct < 100 {
            writeln!(
                self.stdout,
                "Received {:3}% ({:5}/{:5})",
                network_pct,
                progress.received_objects(),
                progress.total_objects(),
            )
            .change_context(TmsError::IoError)?;
            lines += 1
        }

        if index_pct < 100 {
            writeln!(
                self.stdout,
                "Indexed {:3}% ({:5}/{:5})",
                index_pct,
                progress.indexed_objects(),
                progress.total_objects(),
            )
            .change_context(TmsError::IoError)?;
            lines += 1;
        }

        if network_pct < 100 {
            writeln!(self.stdout, "{} ", rate).change_context(TmsError::IoError)?;
            lines += 1;
        }

        if progress.total_objects() > 0 && progress.received_objects() == progress.total_objects() {
            let delta_pct = (100 * progress.indexed_deltas()) / progress.total_deltas();
            writeln!(
                self.stdout,
                "Resolving deltas {:3}% ({:5}/{:5})",
                delta_pct,
                progress.indexed_deltas(),
                progress.total_deltas()
            )
            .change_context(TmsError::IoError)?;
            lines += 1;
        }
        write!(self.stdout, "{:3}% ", total).change_context(TmsError::IoError)?;
        for _ in 0..(total / 3) {
            write!(self.stdout, "█").change_context(TmsError::IoError)?;
        }
        writeln!(self.stdout).change_context(TmsError::IoError)?;
        lines += 1;
        self.time = Instant::now();
        self.bytes_transferred = progress.received_bytes();
        self.lines = lines;

        Ok(())
    }
}

pub fn git_clone(repo: &str, target: &Path) -> Result<Repository> {
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(git_credentials_callback);

    let mut state = CloneSnapshot::new();
    callbacks.transfer_progress(move |progress| {
        state.update(&progress).ok();
        true
    });
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks);
    let mut builder = RepoBuilder::new();
    builder.fetch_options(fo);

    builder
        .clone(repo, target)
        .change_context(TmsError::GitError)
}

fn git_credentials_callback(
    user: &str,
    user_from_url: Option<&str>,
    _cred: git2::CredentialType,
) -> std::result::Result<git2::Cred, git2::Error> {
    let user = match user_from_url {
        Some(user) => user,
        None => user,
    };

    git2::Cred::ssh_key_from_agent(user)
}
