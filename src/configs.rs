use std::fmt::Display;

use serde_derive::{Deserialize, Serialize};

#[derive(Debug)]
pub(crate) enum ConfigError {
    NoDefaultSearchPath,
    WriteFailure,
    LoadError,
}
impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDefaultSearchPath => write!(f, "No default search path was found"),
            Self::WriteFailure => write!(f, "Failure writing the config file"),
            Self::LoadError => write!(f, "Error loading the config file"),
        }
    }
}
impl std::error::Error for ConfigError {}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Config {
    pub search_paths: Vec<String>, // old format, deprecated
    pub search_dirs: Vec<SearchDirectory>,
    pub excluded_dirs: Option<Vec<String>>,
    pub default_session: Option<String>,
    pub display_full_path: Option<bool>,
    pub sessions: Option<Vec<Session>>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SearchDirectory {
    pub path: String,
    pub depth: Option<usize>,
}

impl SearchDirectory {
    pub(crate) fn new(path: String, depth: Option<usize>) -> Self {
        SearchDirectory { path, depth }
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Session {
    pub name: Option<String>,
    pub path: Option<String>,
    pub windows: Option<Vec<Window>>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Window {
    pub name: Option<String>,
    pub path: Option<String>,
    pub panes: Option<Vec<Pane>>,
    pub command: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Pane {}
