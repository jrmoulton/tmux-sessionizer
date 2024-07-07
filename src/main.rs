use clap::Parser;
use error_stack::Report;

use tms::{
    cli::{Cli, SubCommandGiven},
    error::{Result, Suggestion},
    get_single_selection,
    picker::Preview,
    session::{create_sessions, SessionContainer},
    tmux::Tmux,
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

    let sessions = create_sessions(&config)?;
    let session_strings = sessions.list();

    let selected_str =
        if let Some(str) = get_single_selection(&session_strings, Preview::None, &config, &tmux)? {
            str
        } else {
            return Ok(());
        };

    if let Some(session) = sessions.find_session(&selected_str) {
        session.switch_to(&tmux)?;
    }

    Ok(())
}
