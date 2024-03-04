use std::{error::Error, fmt::Display};

#[derive(Debug)]
pub enum TmsError {
    GitError,
    NonUtf8Path,
    TuiError(String),
    IoError,
    ConfigError,
}

impl Display for TmsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConfigError => write!(f, "Config Error"),
            Self::GitError => write!(f, "Git Error"),
            Self::NonUtf8Path => write!(f, "Non Utf-8 Path"),
            Self::IoError => write!(f, "IO Error"),
            Self::TuiError(inner) => write!(f, "TUI error: {inner}"),
        }
    }
}

impl Error for TmsError {}
