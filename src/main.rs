use std::env;

use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;
use error_stack::{Report, ResultExt};

use tms::{
    cli::{Cli, SubCommandGiven},
    configs::Config,
    dirty_paths::DirtyUtf8Path,
    error::{Result, Suggestion, TmsError},
    get_single_selection,
    picker::PickerItem,
    repos::{get_picker_items, RepoProvider},
    session::{Session, SessionType},
    tmux::Tmux,
};

fn main() -> Result<()> {
    // Install debug hooks for formatting of error handling
    Report::install_debug_hook::<Suggestion>(|value, context| {
        context.push_body(format!("{value}"));
    });
    #[cfg(any(not(debug_assertions), test))]
    Report::install_debug_hook::<std::panic::Location>(|_value, _context| {});

    let bin_name = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_name().map(|exe| exe.to_string_lossy().to_string()))
        .unwrap_or("tms".into());
    match CompleteEnv::with_factory(Cli::command)
        .bin(bin_name)
        .try_complete(env::args_os(), None)
    {
        Ok(true) => return Ok(()),
        Err(e) => {
            panic!("failed to generate completions: {e}");
        }
        Ok(false) => {}
    };

    // Use CLAP to parse the command line arguments
    let cli_args = Cli::parse();

    let tmux = Tmux::default();

    let config = match cli_args.handle_sub_commands(&tmux)? {
        SubCommandGiven::Yes => return Ok(()),
        SubCommandGiven::No(config) => config, // continue
    };

    let picker_items = get_picker_items(&config, &tmux)?;
    let running_sessions = tmux.get_running_sessions()?;

    let selected_item =
        if let Some(item) = get_single_selection(picker_items, running_sessions.clone(), None, &config, &tmux)? {
            item
        } else {
            return Ok(());
        };

    match selected_item {
        PickerItem::Project { name, path } => {
            if running_sessions.contains(&name) {
                tmux.switch_client(&name);
            } else {
                let session_type = if path.join(".git").exists() {
                    SessionType::Git
                } else {
                    SessionType::Path
                };
                let session = Session::new(name, path, session_type);
                switch_to_session(&session, &tmux, &config)?;
            }
        }
        PickerItem::TmuxSession(session_name) => {
            tmux.switch_client(&session_name);
        }
    }

    Ok(())
}

fn switch_to_session(session: &Session, tmux: &Tmux, config: &Config) -> Result<()> {
    match &session.session_type {
        SessionType::Git => {
            let repo = RepoProvider::open(&session.path, config)?;
            switch_to_repo_session(session, &repo, tmux, config)
        }
        SessionType::Path => switch_to_path_session(session, tmux, &session.path, config),
    }
}

fn switch_to_repo_session(
    session: &Session,
    repo: &RepoProvider,
    tmux: &Tmux,
    config: &Config,
) -> Result<()> {
    let path = if repo.is_bare() {
        repo.path().to_path_buf().to_string()?
    } else {
        repo.work_dir()
            .expect("bare repositories should all have parent directories")
            .canonicalize()
            .change_context(TmsError::IoError)?
            .to_string()?
    };
    let session_name = session.name.replace('.', "_");

    if !tmux.session_exists(&session_name) {
        tmux.new_session(Some(&session_name), Some(&path));
        tmux.set_up_tmux_env(repo, &session_name, config)?;
        tmux.run_session_create_script(&session.path, &session_name, config)?;
    }

    tmux.switch_to_session(&session_name);

    Ok(())
}

fn switch_to_path_session(
    session: &Session,
    tmux: &Tmux,
    path: &std::path::Path,
    config: &Config,
) -> Result<()> {
    let session_name = session.name.replace('.', "_");

    if !tmux.session_exists(&session_name) {
        tmux.new_session(Some(&session_name), path.to_str());
        tmux.run_session_create_script(path, &session_name, config)?;
    }

    tmux.switch_to_session(&session_name);

    Ok(())
}