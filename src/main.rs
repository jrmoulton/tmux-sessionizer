use clap::Parser;
use configs::ConfigError;
use error_stack::{Report, Result, ResultExt};
use std::fs::canonicalize;
use tms::{
    cli::{Cli, SubCommandGiven},
    configs::{self, SearchDirectory},
    dirty_paths::DirtyUtf8Path,
    find_repos, get_single_selection,
    picker::Preview,
    repos::RepoContainer,
    session_exists, set_up_tmux_env, switch_to_session,
    tmux::Tmux,
    Suggestion, TmsError,
};

fn main() -> Result<(), TmsError> {
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

    let mut search_dirs: Vec<_> = config
        .search_dirs
        .unwrap_or(Vec::new())
        .into_iter()
        .map(|mut search_dir| {
            let expanded_path = shellexpand::full(&search_dir.path.to_string_lossy())
                .change_context(TmsError::IoError)
                .unwrap()
                .to_string();

            search_dir.path = canonicalize(expanded_path)
                .change_context(TmsError::IoError)
                .unwrap();

            search_dir
        })
        .collect();

    // merge old search paths with new search directories
    if let Some(search_paths) = config.search_paths {
        if !search_paths.is_empty() {
            search_dirs.extend(search_paths.into_iter().map(|path| {
                SearchDirectory::new(
                    canonicalize(
                        shellexpand::full(&path)
                            .change_context(TmsError::IoError)
                            .unwrap()
                            .to_string(),
                    )
                    .change_context(TmsError::IoError)
                    .unwrap(),
                    10,
                )
            }));
        }
    }

    if search_dirs.is_empty() {
        return Err(ConfigError::NoDefaultSearchPath)
            .attach_printable(
                "You must configure at least one default search path with the `config` subcommand. E.g `tms config` ",
            )
            .change_context(TmsError::ConfigError);
    }

    // Find repositories and present them with the fuzzy finder
    let repos = find_repos(
        search_dirs,
        config.excluded_dirs,
        config.display_full_path,
        config.search_submodules,
        config.recursive_submodules,
    )?;

    let repo_name = if let Some(str) = get_single_selection(
        &repos.list(),
        Preview::None,
        config.picker_colors,
        config.shortcuts,
        tmux.clone(),
    )? {
        str
    } else {
        return Ok(());
    };

    let found_repo = repos
        .find_repo(&repo_name)
        .expect("The internal representation of the selected repository should be present");
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
    let repo_short_name = (if config.display_full_path == Some(true) {
        std::path::PathBuf::from(&repo_name)
            .file_name()
            .expect("None of the paths here should terminate in `..`")
            .to_string()?
    } else {
        repo_name
    })
    .replace('.', "_");

    if !session_exists(&repo_short_name, &tmux) {
        tmux.new_session(Some(&repo_short_name), Some(&path));
        set_up_tmux_env(found_repo, &repo_short_name, &tmux)?;
    }

    switch_to_session(&repo_short_name, &tmux);

    Ok(())
}
