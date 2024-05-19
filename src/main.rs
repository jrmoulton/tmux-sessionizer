use std::{collections::HashMap, path::PathBuf};

use clap::Parser;
use error_stack::{Report, ResultExt};

use git2::Repository;
use tms::{
    cli::{Cli, SubCommandGiven},
    dirty_paths::DirtyUtf8Path,
    error::{Result, TmsError},
    get_single_selection,
    picker::Preview,
    repos::find_repos,
    repos::RepoContainer,
    session_exists, set_up_tmux_env, switch_to_session,
    tmux::Tmux,
    Suggestion,
};

fn main() -> Result<()> {
    // Install debug hooks for formatting of error handling
    Report::install_debug_hook::<Suggestion>(|value, context| {
        context.push_body(format!("{value}"));
    });
    #[cfg(any(not(debug_assertions), test))]
    Report::install_debug_hook::<std::panic::Location>(|_value, _context| {});

    // Use CLAP to parse the command line arguments
    let cli_args = Cli::parse();

    let tmux = Tmux::default();

    let config = match cli_args.handle_sub_commands(&tmux)? {
        SubCommandGiven::Yes => return Ok(()),
        SubCommandGiven::No(config) => config, // continue
    };

    let bookmarks = config.bookmark_paths();

    // Find repositories and present them with the fuzzy finder
    let repos = find_repos(
        config.search_dirs().change_context(TmsError::ConfigError)?,
        config.excluded_dirs,
        config.display_full_path,
        config.search_submodules,
        config.recursive_submodules,
    )?;

    let mut dirs = repos.list();

    dirs.append(&mut bookmarks.keys().map(|b| b.to_string()).collect());

    let selected_str = if let Some(str) = get_single_selection(
        &dirs,
        Preview::None,
        config.picker_colors,
        config.shortcuts,
        tmux.clone(),
    )? {
        str
    } else {
        return Ok(());
    };

    if let Some(found_repo) = repos.find_repo(&selected_str) {
        switch_to_repo_session(selected_str, found_repo, &tmux, config.display_full_path)?;
    } else {
        switch_to_bookmark_session(selected_str, &tmux, bookmarks)?;
    }

    Ok(())
}

fn switch_to_repo_session(
    selected_str: String,
    found_repo: &Repository,
    tmux: &Tmux,
    display_full_path: Option<bool>,
) -> Result<()> {
    let path = if found_repo.is_bare() {
        found_repo.path().to_string()?
    } else {
        found_repo
            .workdir()
            .expect("bare repositories should all have parent directories")
            .canonicalize()
            .change_context(TmsError::IoError)?
            .to_string()?
    };
    let repo_short_name = (if display_full_path == Some(true) {
        std::path::PathBuf::from(&selected_str)
            .file_name()
            .expect("None of the paths here should terminate in `..`")
            .to_string()?
    } else {
        selected_str
    })
    .replace('.', "_");

    if !session_exists(&repo_short_name, tmux) {
        tmux.new_session(Some(&repo_short_name), Some(&path));
        set_up_tmux_env(found_repo, &repo_short_name, tmux)?;
    }

    switch_to_session(&repo_short_name, tmux);

    Ok(())
}

fn switch_to_bookmark_session(
    selected_str: String,
    tmux: &Tmux,
    bookmarks: HashMap<String, PathBuf>,
) -> Result<()> {
    let path = &bookmarks[&selected_str];
    let session_name = path
        .file_name()
        .expect("Bookmarks should not end in `..`")
        .to_string()?
        .replace('.', "_");

    if !session_exists(&session_name, tmux) {
        tmux.new_session(Some(&session_name), path.to_str());
    }

    switch_to_session(&session_name, tmux);

    Ok(())
}
