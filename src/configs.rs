use clap::ValueEnum;
use error_stack::{Result, ResultExt};
use serde_derive::{Deserialize, Serialize};
use std::{env, fmt::Display, fs::canonicalize, io::Write, path::PathBuf};

use ratatui::style::{Color, Style};

use crate::{keymap::Keymap, Suggestion, TmsError};

#[derive(Debug)]
pub enum ConfigError {
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

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub default_session: Option<String>,
    pub display_full_path: Option<bool>,
    pub search_submodules: Option<bool>,
    pub recursive_submodules: Option<bool>,
    pub switch_filter_unknown: Option<bool>,
    pub session_sort_order: Option<SessionSortOrderConfig>,
    pub excluded_dirs: Option<Vec<String>>,
    pub search_paths: Option<Vec<String>>, // old format, deprecated
    pub search_dirs: Option<Vec<SearchDirectory>>,
    pub sessions: Option<Vec<Session>>,
    pub picker_colors: Option<PickerColorConfig>,
    pub shortcuts: Option<Keymap>,
}

impl Config {
    pub(crate) fn new() -> Result<Self, ConfigError> {
        let config_builder = match env::var("TMS_CONFIG_FILE") {
            Ok(path) => {
                config::Config::builder().add_source(config::File::with_name(&path).required(false))
            }
            Err(e) => match e {
                env::VarError::NotPresent => {
                    let mut builder = config::Config::builder();
                    let mut config_found = false; // Stores whether a valid config file was found
                    if let Some(home_path) = dirs::home_dir() {
                        config_found = true;
                        let path = home_path.as_path().join(".config/tms/config.toml");
                        builder = builder.add_source(config::File::from(path).required(false));
                    }
                    if let Some(config_path) = dirs::config_dir() {
                        config_found = true;
                        let path = config_path.as_path().join("tms/config.toml");
                        builder = builder.add_source(config::File::from(path).required(false));
                    }
                    if !config_found {
                        return Err(ConfigError::LoadError)
                            .attach_printable("Could not find a valid location for config file (both home and config dirs cannot be found)")
                            .attach(Suggestion("Try specifying a config file with the TMS_CONFIG_FILE environment variable."));
                    }
                    builder
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
        // The TMS_CONFIG_FILE envvar should be set, either by the user or when the config is
        // loaded. However, there is a possibility it becomes unset between loading and saving
        // the config. In this case, it will fall back to the platform-specific config folder, and
        // if that can't be found then it's good old ~/.config
        let path = match env::var("TMS_CONFIG_FILE") {
            Ok(path) => PathBuf::from(path),
            Err(_) => {
                if let Some(config_path) = dirs::config_dir() {
                    config_path.as_path().join("tms/config.toml")
                } else if let Some(home_path) = dirs::home_dir() {
                    home_path.as_path().join(".config/tms/config.toml")
                } else {
                    return Err(ConfigError::LoadError)
                        .attach_printable("Could not find a valid location to write config file (both home and config dirs cannot be found)")
                        .attach(Suggestion("Try specifying a config file with the TMS_CONFIG_FILE environment variable."));
                }
            }
        };
        let parent = path
            .parent()
            .ok_or(ConfigError::FileWriteError)
            .attach_printable(format!(
                "Unable to determine parent directory of specified tms config file: {}",
                path.to_str()
                    .unwrap_or("(path could not be converted to string)")
            ))?;
        std::fs::create_dir_all(parent)
            .change_context(ConfigError::FileWriteError)
            .attach_printable("Unable to create tms config folder")?;
        let mut file = std::fs::File::create(path).change_context(ConfigError::FileWriteError)?;
        file.write_all(&toml_pretty)
            .change_context(ConfigError::FileWriteError)?;
        Ok(())
    }

    pub(crate) fn search_dirs(&self) -> Result<Vec<SearchDirectory>, TmsError> {
        let mut search_dirs = if let Some(search_dirs) = self.search_dirs.as_ref() {
            search_dirs
                .iter()
                .map(|search_dir| {
                    let expanded_path = shellexpand::full(&search_dir.path.to_string_lossy())
                        .change_context(TmsError::IoError)
                        .unwrap()
                        .to_string();

                    let path = canonicalize(expanded_path)
                        .change_context(TmsError::IoError)
                        .unwrap();

                    SearchDirectory::new(path, search_dir.depth)
                })
                .collect()
        } else {
            Vec::new()
        };

        // merge old search paths with new search directories
        if let Some(search_paths) = self.search_paths.as_ref() {
            if !search_paths.is_empty() {
                search_dirs.extend(search_paths.iter().map(|path| {
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

        Ok(search_dirs)
    }
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchDirectory {
    pub path: PathBuf,
    pub depth: usize,
}

impl SearchDirectory {
    pub fn new(path: PathBuf, depth: usize) -> Self {
        SearchDirectory { path, depth }
    }
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub name: Option<String>,
    pub path: Option<String>,
    pub windows: Option<Vec<Window>>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Window {
    pub name: Option<String>,
    pub path: Option<String>,
    pub panes: Option<Vec<Pane>>,
    pub command: Option<String>,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Pane {}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PickerColorConfig {
    pub highlight_color: Option<String>,
    pub highlight_text_color: Option<String>,
    pub border_color: Option<String>,
    pub info_color: Option<String>,
    pub prompt_color: Option<String>,
}

impl PickerColorConfig {
    pub fn highlight_style(&self) -> Style {
        let mut style = Style::default().bg(Color::LightBlue).fg(Color::Black);

        if let Some(color) = &self.highlight_color {
            if let Some(color) = rgb_to_color(color) {
                style = style.bg(color);
            }
        }

        if let Some(color) = &self.highlight_text_color {
            if let Some(color) = rgb_to_color(color) {
                style = style.fg(color);
            }
        }

        style
    }

    pub fn border_color(&self) -> Option<Color> {
        if let Some(color) = &self.border_color {
            rgb_to_color(color)
        } else {
            None
        }
    }

    pub fn info_color(&self) -> Option<Color> {
        if let Some(color) = &self.info_color {
            rgb_to_color(color)
        } else {
            None
        }
    }

    pub fn prompt_color(&self) -> Option<Color> {
        if let Some(color) = &self.prompt_color {
            rgb_to_color(color)
        } else {
            None
        }
    }
}

fn rgb_to_color(color: &str) -> Option<Color> {
    if color.len() == 7 && color.starts_with('#') {
        let red = u8::from_str_radix(&color[1..3], 16).ok()?;
        let green = u8::from_str_radix(&color[3..5], 16).ok()?;
        let blue = u8::from_str_radix(&color[5..7], 16).ok()?;
        Some(Color::Rgb(red, green, blue))
    } else {
        None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub enum SessionSortOrderConfig {
    Alphabetical,
    LastAttached,
}

impl ValueEnum for SessionSortOrderConfig {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Alphabetical, Self::LastAttached]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            SessionSortOrderConfig::Alphabetical => {
                Some(clap::builder::PossibleValue::new("Alphabetical"))
            }
            SessionSortOrderConfig::LastAttached => {
                Some(clap::builder::PossibleValue::new("LastAttached"))
            }
        }
    }
}
