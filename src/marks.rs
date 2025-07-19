use std::{env::current_dir, path::PathBuf};

use clap::{Args, Subcommand};
use clap_complete::{ArgValueCandidates, CompletionCandidate};
use error_stack::ResultExt;

use crate::{
    configs::Config,
    dirty_paths::DirtyUtf8Path,
    error::{Result, TmsError},
    session::Session,
    tmux::Tmux,
};

#[derive(Debug, Args)]
#[clap(args_conflicts_with_subcommands = true)]
pub struct MarksCommand {
    #[arg(add  = ArgValueCandidates::new(get_completion_candidates))]
    /// The index of the mark to open
    index: Option<usize>,
    #[command(subcommand)]
    cmd: Option<MarksSubCommand>,
}

#[derive(Debug, Subcommand)]
pub enum MarksSubCommand {
    /// List all marks
    List,
    /// Add a session mark
    Set(MarksSetCommand),
    /// Open the session at index
    Open(MarksOpenCommand),
    /// Delete marks
    Delete(MarksDeleteCommand),
}

#[derive(Debug, Args)]
pub struct MarksSetCommand {
    /// Index of mark to set, if empty will append after the last item
    index: Option<usize>,
    #[arg(long, short)]
    /// Path to project directory, if empty will use the current directory
    path: Option<String>,
}

#[derive(Debug, Args)]
pub struct MarksOpenCommand {
    #[arg(add  = ArgValueCandidates::new(get_completion_candidates))]
    /// The index of the mark to open
    index: usize,
}

#[derive(Debug, Args)]
#[group(required = true, multiple = false)]
pub struct MarksDeleteCommand {
    #[arg(add  = ArgValueCandidates::new(get_completion_candidates))]
    /// Index of mark to delete
    index: Option<usize>,
    #[arg(long, short)]
    /// Delete all items
    all: bool,
}

fn get_completion_candidates() -> Vec<CompletionCandidate> {
    let config = Config::new().unwrap_or_default();
    let marks = get_marks(&config).unwrap_or_default();
    marks
        .iter()
        .map(|(index, session)| {
            CompletionCandidate::new(index.to_string()).help(Some(session.name.clone().into()))
        })
        .collect::<Vec<_>>()
}

pub fn marks_command(args: &MarksCommand, config: Config, tmux: &Tmux) -> Result<()> {
    match (&args.cmd, args.index) {
        (None, None) => list(config),
        (_, Some(index)) => open(index, &config, tmux),
        (Some(MarksSubCommand::List), _) => list(config),
        (Some(MarksSubCommand::Set(args)), _) => set(args, config),
        (Some(MarksSubCommand::Open(args)), _) => open(args.index, &config, tmux),
        (Some(MarksSubCommand::Delete(args)), _) => delete(args, config),
    }
}

fn list(config: Config) -> Result<()> {
    let items = get_marks(&config).unwrap_or_default();
    items.iter().for_each(|(index, session)| {
        println!("{index}: {} ({})", session.name, session.path().display());
    });
    Ok(())
}

fn set(args: &MarksSetCommand, mut config: Config) -> Result<()> {
    let index = args.index.unwrap_or_else(|| {
        let items = get_marks(&config).unwrap_or_default();
        items
            .iter()
            .enumerate()
            .take_while(|(i, (index, _))| i == index)
            .count()
    });

    let path = if let Some(path) = &args.path {
        path.to_owned()
    } else {
        current_dir()
            .change_context(TmsError::IoError)?
            .to_string()
            .change_context(TmsError::IoError)?
    };
    config.add_mark(path, index);
    config.save().change_context(TmsError::ConfigError)
}

fn get_marks(config: &Config) -> Option<Vec<(usize, Session)>> {
    let items = config.marks.as_ref()?;
    let mut items = items
        .iter()
        .filter_map(|(index, item)| {
            let index = index.parse::<usize>().ok();
            let session = path_to_session(item).ok();
            index.zip(session)
        })
        .collect::<Vec<_>>();
    items.sort_by(|(a, _), (b, _)| a.cmp(b));
    Some(items)
}

fn open(index: usize, config: &Config, tmux: &Tmux) -> Result<()> {
    let path = config
        .marks
        .as_ref()
        .and_then(|items| items.get(&index.to_string()))
        .ok_or(TmsError::ConfigError)
        .attach_printable(format!("Session with index {} not found in marks", index))?;

    let session = path_to_session(path)?;

    session.switch_to(tmux, config)
}

fn path_to_session(path: &String) -> Result<Session> {
    let path = shellexpand::full(path)
        .change_context(TmsError::IoError)
        .and_then(|p| {
            PathBuf::from(p.to_string())
                .canonicalize()
                .change_context(TmsError::IoError)
        })?;

    let session_name = path
        .file_name()
        .expect("The file name doesn't end in `..`")
        .to_string()?;
    let session = Session::new(session_name, crate::session::SessionType::Path(path));
    Ok(session)
}

fn delete(args: &MarksDeleteCommand, mut config: Config) -> Result<()> {
    if args.all {
        config.clear_marks();
    } else if let Some(index) = args.index {
        config.delete_mark(index);
    } else {
        unreachable!("One of the args is required by clap");
    }
    config.save().change_context(TmsError::ConfigError)
}
