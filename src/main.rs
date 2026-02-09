use std::{collections::HashSet, env, path::PathBuf, process::Command};

use clap::{CommandFactory, Parser};
use clap_complete::CompleteEnv;
use error_stack::{Report, ResultExt};

use tms::{
    cli::{Cli, SubCommandGiven},
    configs::SessionSortOrderConfig,
    error::{Result, Suggestion, TmsError},
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

    let sessions = create_sessions(&config)?;
    let (session_strings, active_sessions) = get_session_list(&sessions, &config, &tmux);

    // Create picker with active session styling
    let mut picker = tms::picker::Picker::new(
        &session_strings,
        None,
        config.shortcuts.as_ref(),
        config.input_position.unwrap_or_default(),
        &tmux,
    )
    .set_colors(config.picker_colors.as_ref());

    if let Some(active) = active_sessions {
        picker = picker.set_active_sessions(active);
    }

    let selected_str = if let Some(str) = picker.run()? {
        str
    } else {
        return Ok(());
    };

    // Check if user wants to create a new directory
    if let Some(name) = selected_str.strip_prefix("__TMS_CREATE_NEW__:") {
        create_new_directory(name, &config, &tmux)?;
        return Ok(());
    }

    if let Some(session) = sessions.find_session(&selected_str) {
        session.switch_to(&tmux, &config)?;
    }

    Ok(())
}

/// Get the session list, optionally sorted with active sessions first
/// Returns (session_list, active_sessions_set)
fn get_session_list(
    sessions: &impl SessionContainer,
    config: &tms::configs::Config,
    tmux: &Tmux,
) -> (Vec<String>, Option<HashSet<String>>) {
    let all_sessions = sessions.list();

    // If LastAttached is configured, prioritize active tmux sessions
    if matches!(
        config.session_sort_order,
        Some(SessionSortOrderConfig::LastAttached)
    ) {
        // Get active sessions from tmux with timestamps, excluding the currently attached one
        let active_sessions_raw =
            tmux.list_sessions("'#{?session_attached,,#{session_name}#,#{session_last_attached}}'");

        // Parse into (name, timestamp) pairs
        let active_sessions: Vec<(&str, i64)> = active_sessions_raw
            .trim()
            .split('\n')
            .filter_map(|line| {
                let line = line.trim_matches('\'');
                let (name, timestamp) = line.split_once(',')?;
                let timestamp = timestamp.parse::<i64>().ok()?;
                Some((name, timestamp))
            })
            .collect();

        // Build a set of active session names for fast lookup
        let active_names: HashSet<&str> = active_sessions.iter().map(|(name, _)| *name).collect();
        let active_names_owned: HashSet<String> =
            active_names.iter().map(|s| s.to_string()).collect();

        // Partition sessions into active and inactive
        let (mut active_list, mut inactive_list): (Vec<String>, Vec<String>) =
            all_sessions.into_iter().partition(|session_name| {
                // Check if this session name (or its normalized form) is active
                // Tmux normalizes both dots and hyphens to underscores in session names
                let normalized = session_name.replace(['.', '-'], "_");
                active_names.contains(session_name.as_str())
                    || active_names.contains(&normalized.as_str())
            });

        // Sort active sessions by timestamp (most recent first)
        active_list.sort_by_cached_key(|name| {
            // Find the timestamp for this session
            // Tmux normalizes both dots and hyphens to underscores
            let normalized = name.replace(['.', '-'], "_");
            active_sessions
                .iter()
                .find(|(active_name, _)| *active_name == name || *active_name == normalized)
                .map(|(_, timestamp)| -timestamp) // Negative for descending order
                .unwrap_or(0)
        });

        // Sort inactive sessions alphabetically
        inactive_list.sort();

        // Combine: active first, then inactive
        active_list.extend(inactive_list);
        (active_list, Some(active_names_owned))
    } else {
        // Default behavior: alphabetically sorted
        (all_sessions, None)
    }
}

/// Default create hook template embedded in binary
const DEFAULT_HOOK: &str = r#"#!/usr/bin/env bash
# tms create hook
#
# Called when you type a non-existent name in the tms picker.
# Example: create-hook "my-app" "/home/user/code" "/home/user/work"
#
# Parameters:
#   $1 = name you typed
#   $2, $3... = search directories from your config
#
# Output (optional):
#   Print directory name to stdout to override session name
#   If no output, session name defaults to $1
#
# Must: Create a directory that tms can discover, exit 0 on success

set -e

NAME="$1"
FIRST_DIR="$2"

# Detect Git URL (GitHub, GitLab, etc.) and clone it
# HTTPS: https://github.com/user/repo.git
# SSH:   git@github.com:user/repo.git
if [[ "$NAME" =~ ^https?://[^/]+/([^/]+)/([^/]+)(\.git)?$ ]]; then
    REPO_NAME="${BASH_REMATCH[2]%.git}"
elif [[ "$NAME" =~ ^git@[^:]+:([^/]+)/([^/]+)(\.git)?$ ]]; then
    REPO_NAME="${BASH_REMATCH[2]%.git}"
else
    REPO_NAME=""
fi

if [[ -n "$REPO_NAME" ]]; then
    TARGET_DIR="$FIRST_DIR/$REPO_NAME"

    # Only clone if directory doesn't exist
    if [[ ! -d "$TARGET_DIR" ]]; then
        echo "Cloning $REPO_NAME..." >&2
        git clone "$NAME" "$TARGET_DIR" 2>&1 | sed 's/^/  /' >&2
        echo "Done!" >&2
    else
        echo "Directory $REPO_NAME already exists, opening..." >&2
    fi

    echo "$REPO_NAME"  # Tell tms the actual directory name
    exit 0
fi

# Default: create new directory with git init
DIR="$FIRST_DIR/$NAME"
mkdir -p "$DIR"
cd "$DIR"
git init -q

# No output = use original name from picker

# Example: customize based on name pattern
# if [[ "$NAME" == *-rs ]]; then
#     cargo init --name "${NAME%-rs}"
#     echo "${NAME%-rs}"  # Return the actual directory name
# fi
"#;

/// Ensure the create hook exists at the conventional location
fn ensure_hook_exists(hook_path: &PathBuf) -> Result<()> {
    if hook_path.exists() {
        return Ok(());
    }

    // Create parent directory if needed
    if let Some(parent) = hook_path.parent() {
        std::fs::create_dir_all(parent)
            .change_context(TmsError::IoError)
            .attach_printable(format!("Failed to create directory: {}", parent.display()))?;
    }

    // Write default hook
    std::fs::write(hook_path, DEFAULT_HOOK)
        .change_context(TmsError::IoError)
        .attach_printable(format!("Failed to write hook: {}", hook_path.display()))?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(hook_path)
            .change_context(TmsError::IoError)?
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(hook_path, perms)
            .change_context(TmsError::IoError)
            .attach_printable("Failed to set hook permissions")?;
    }

    eprintln!("✓ Created default hook at {}", hook_path.display());
    eprintln!("  Customize it: nvim {}", hook_path.display());

    Ok(())
}

/// Check if a file is executable
#[cfg(unix)]
fn is_executable(path: &PathBuf) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &PathBuf) -> bool {
    true
}

/// Handle creation of a new directory via the create hook
fn create_new_directory(name: &str, config: &tms::configs::Config, tmux: &Tmux) -> Result<()> {
    // Check ~/.config/tms/create-hook first, then fall back to platform config dir
    // (matches how config.toml loading works)
    let hook_path = dirs::home_dir()
        .map(|h| h.join(".config/tms/create-hook"))
        .filter(|p| p.exists())
        .or_else(|| dirs::config_dir().map(|c| c.join("tms/create-hook")))
        .ok_or(TmsError::ConfigError)
        .attach_printable("Could not determine config directory")?;

    // Ensure hook exists (create from template if needed)
    ensure_hook_exists(&hook_path)?;

    // Check if executable
    if !is_executable(&hook_path) {
        return Err(TmsError::ConfigError)
            .attach_printable(format!("Hook is not executable: {}", hook_path.display()))
            .attach_printable(format!("Run: chmod +x {}", hook_path.display()));
    }

    // Get search directories from config
    let search_dirs = config
        .search_dirs
        .as_ref()
        .ok_or(TmsError::ConfigError)
        .attach_printable("No search directories configured in config.toml")?;

    let search_paths: Vec<String> = search_dirs
        .iter()
        .filter_map(|d| {
            shellexpand::full(&d.path.to_string_lossy())
                .ok()
                .map(|p| p.to_string())
        })
        .collect();

    if search_paths.is_empty() {
        return Err(TmsError::ConfigError).attach_printable("search_dirs is empty in config.toml");
    }

    // Execute hook: create-hook "name" "/path1" "/path2" ...
    // Inherit stderr so user sees progress messages, but capture stdout for directory name
    let output = Command::new(&hook_path)
        .arg(name)
        .args(&search_paths)
        .stderr(std::process::Stdio::inherit())
        .output()
        .change_context(TmsError::IoError)
        .attach_printable("Failed to execute create hook")?;

    // Check exit status
    if !output.status.success() {
        return Err(TmsError::IoError)
            .attach_printable(format!(
                "Hook failed with exit code: {}",
                output.status.code().unwrap_or(-1)
            ))
            .attach_printable("Check hook output for details");
    }

    // Get session name from hook's stdout, or fall back to the typed name
    let hook_output = String::from_utf8_lossy(&output.stdout);
    let session_name = hook_output.trim();
    let session_name = if session_name.is_empty() {
        name
    } else {
        session_name
    };
    // Re-discover sessions to find the one we just created
    let sessions = create_sessions(config)?;
    let session = sessions
        .find_session(session_name)
        .ok_or(TmsError::IoError)
        .attach_printable("Hook did not create a discoverable directory")
        .attach_printable(format!(
            "Expected to find a directory matching: {}",
            session_name
        ))?;

    // Open it using normal session flow
    session.switch_to(tmux, config)?;

    Ok(())
}
