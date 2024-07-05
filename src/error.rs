use std::{error::Error, fmt::Display};

pub type Result<T> = error_stack::Result<T, TmsError>;

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

#[derive(Debug)]
pub struct Suggestion(pub &'static str);
impl Display for Suggestion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crossterm::style::Stylize;
        f.write_str(&format!("Suggestion: {}", self.0).green().bold().to_string())
    }
}
