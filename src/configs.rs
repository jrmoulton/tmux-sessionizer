use error_stack::{Result, ResultExt};
use std::{env, fmt::Display, io::Write, path::PathBuf};

use serde_derive::{Deserialize, Serialize};

use crate::Suggestion;

#[derive(Debug)]
pub(crate) enum ConfigError {
    NoDefaultSearchPath,
    LoadError,
    TomlError,
    FileWriteError,
}

impl std::error::Error for ConfigError {}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDefaultSearchPath => write!(f, "No default search path was found"),
            Self::TomlError => write!(f, "Could not serialize config to TOML"),
            Self::FileWriteError => write!(f, "Could not write to config file"),
            Self::LoadError => write!(f, "Could not load configuration"),
        }
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Config {
    pub default_session: Option<String>,
    pub display_full_path: Option<bool>,
    pub search_submodules: Option<bool>,
    pub excluded_dirs: Option<Vec<String>>,
    pub search_paths: Option<Vec<String>>, // old format, deprecated
    pub search_dirs: Option<Vec<SearchDirectory>>,
    pub sessions: Option<Vec<Session>>,
}

impl Config {
    pub(crate) fn new() -> Result<Self, ConfigError> {
        let config_builder = match env::var("TMS_CONFIG_FILE") {
            Ok(path) => {
                config::Config::builder().add_source(config::File::with_name(&path).required(false))
            }
            Err(e) => match e {
                env::VarError::NotPresent => {
                    let mut path = home::home_dir()
                        .ok_or(ConfigError::LoadError)
                        .attach_printable("Could not locate home directory")
                        .attach(Suggestion(
                            "Try specifying a config file with the TMS_CONFIG_FILE environment variable.",
                        ))?;
                    path.push(".config/tms/config.toml");
                    config::Config::builder().add_source(config::File::from(path).required(false))
                }
                env::VarError::NotUnicode(_) => {
                    return Err(ConfigError::LoadError).attach_printable(
                        "Invalid non-unicode value for TMS_CONFIG_FILE env variable",
                    );
                }
            },
        };
        let config = config_builder
            .build()
            .change_context(ConfigError::LoadError)
            .attach_printable("Could not parse configuration")?;
        config
            .try_deserialize()
            .change_context(ConfigError::LoadError)
            .attach_printable("Could not deserialize configuration")
    }

    pub(crate) fn save(&self) -> Result<(), ConfigError> {
        let toml_pretty = toml::to_string_pretty(self)
            .change_context(ConfigError::TomlError)?
            .into_bytes();
        let path = match env::var("TMS_CONFIG_FILE") {
            Ok(path) => PathBuf::from(path),
            Err(_) => {
                let mut temp_path = home::home_dir()
                    .ok_or(ConfigError::LoadError)
                    .attach_printable("Could not locate home directory")
                    .attach(Suggestion(
                        "Try specifying a config file with the TMS_CONFIG_FILE environment variable.",
                    ))?;
                temp_path.push(".config/tms/config.toml");
                temp_path
            }
        };
        let mut file = std::fs::File::create(path).change_context(ConfigError::FileWriteError)?;
        file.write_all(&toml_pretty)
            .change_context(ConfigError::FileWriteError)?;
        Ok(())
    }
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct SearchDirectory {
    pub path: PathBuf,
    pub depth: usize,
}

impl SearchDirectory {
    pub(crate) fn new(path: PathBuf, depth: usize) -> Self {
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
