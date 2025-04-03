use clap::ValueEnum;
use error_stack::ResultExt;
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, env, fmt::Display, fs::canonicalize, io::Write, path::PathBuf};

use ratatui::style::{Color, Style, Stylize};

use crate::{error::Suggestion, keymap::Keymap};

type Result<T> = error_stack::Result<T, ConfigError>;

#[derive(Debug)]
pub enum ConfigError {
    NoDefaultSearchPath,
    NoValidSearchPath,
    LoadError,
    TomlError,
    FileWriteError,
    IoError,
}

impl std::error::Error for ConfigError {}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDefaultSearchPath => write!(f, "No default search path was found"),
            Self::NoValidSearchPath => write!(f, "No valid search path was found"),
            Self::TomlError => write!(f, "Could not serialize config to TOML"),
            Self::FileWriteError => write!(f, "Could not write to config file"),
            Self::LoadError => write!(f, "Could not load configuration"),
            Self::IoError => write!(f, "IO error"),
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
    pub bookmarks: Option<Vec<String>>,
    pub session_configs: Option<HashMap<String, SessionConfig>>,
    pub marks: Option<HashMap<String, String>>,
    pub clone_repo_switch: Option<CloneRepoSwitchConfig>,
    pub vcs_providers: Option<Vec<VcsProviders>>,
}

pub const DEFAULT_VCS_PROVIDERS: &[VcsProviders] = &[VcsProviders::Git];

#[derive(Copy, Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VcsProviders {
    Git,
    #[serde(alias = "jj")]
    Jujutsu,
}

#[derive(Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConfigExport {
    pub default_session: Option<String>,
    pub display_full_path: bool,
    pub search_submodules: bool,
    pub recursive_submodules: bool,
    pub switch_filter_unknown: bool,
    pub session_sort_order: SessionSortOrderConfig,
    pub excluded_dirs: Vec<String>,
    pub search_dirs: Vec<SearchDirectory>,
    pub sessions: Vec<Session>,
    pub picker_colors: PickerColorConfig,
    pub shortcuts: Keymap,
    pub bookmarks: Vec<String>,
    pub session_configs: HashMap<String, SessionConfig>,
    pub marks: HashMap<String, String>,
    pub clone_repo_switch: CloneRepoSwitchConfig,
    pub vcs_providers: Vec<VcsProviders>,
}

impl From<Config> for ConfigExport {
    fn from(value: Config) -> Self {
        Self {
            default_session: value.default_session,
            display_full_path: value.display_full_path.unwrap_or_default(),
            search_submodules: value.search_submodules.unwrap_or_default(),
            recursive_submodules: value.recursive_submodules.unwrap_or_default(),
            switch_filter_unknown: value.switch_filter_unknown.unwrap_or_default(),
            session_sort_order: value.session_sort_order.unwrap_or_default(),
            excluded_dirs: value.excluded_dirs.unwrap_or_default(),
            search_dirs: value.search_dirs.unwrap_or_default(),
            sessions: value.sessions.unwrap_or_default(),
            picker_colors: PickerColorConfig::with_defaults(
                value.picker_colors.unwrap_or_default(),
            ),
            shortcuts: value
                .shortcuts
                .as_ref()
                .map(Keymap::with_defaults)
                .unwrap_or_default(),
            bookmarks: value.bookmarks.unwrap_or_default(),
            session_configs: value.session_configs.unwrap_or_default(),
            marks: value.marks.unwrap_or_default(),
            clone_repo_switch: value.clone_repo_switch.unwrap_or_default(),
            vcs_providers: value.vcs_providers.unwrap_or(DEFAULT_VCS_PROVIDERS.into()),
        }
    }
}

impl Config {
    pub(crate) fn new() -> Result<Self> {
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

    pub(crate) fn save(&self) -> Result<()> {
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

    pub fn search_dirs(&self) -> Result<Vec<SearchDirectory>> {
        if self.search_dirs.as_ref().is_none_or(Vec::is_empty)
            && self.search_paths.as_ref().is_none_or(Vec::is_empty)
        {
            return Err(ConfigError::NoDefaultSearchPath)
            .attach_printable(
                "You must configure at least one default search path with the `config` subcommand. E.g `tms config` ",
            );
        }

        let mut search_dirs = if let Some(search_dirs) = self.search_dirs.as_ref() {
            search_dirs
                .iter()
                .filter_map(|search_dir| {
                    let expanded_path = shellexpand::full(&search_dir.path.to_string_lossy())
                        .ok()?
                        .to_string();

                    let path = canonicalize(expanded_path).ok()?;

                    Some(SearchDirectory::new(path, search_dir.depth))
                })
                .collect()
        } else {
            Vec::new()
        };

        // merge old search paths with new search directories
        if let Some(search_paths) = self.search_paths.as_ref() {
            if !search_paths.is_empty() {
                search_dirs.extend(search_paths.iter().filter_map(|path| {
                    let expanded_path = shellexpand::full(&path).ok()?.to_string();
                    let path = canonicalize(expanded_path).ok()?;

                    Some(SearchDirectory::new(path, 10))
                }));
            }
        }

        if search_dirs.is_empty() {
            return Err(ConfigError::NoValidSearchPath)
            .attach_printable(
                "You must configure at least one valid search path with the `config` subcommand. E.g `tms config` "
            );
        }

        Ok(search_dirs)
    }

    pub fn add_bookmark(&mut self, path: String) {
        let bookmarks = &mut self.bookmarks;
        match bookmarks {
            Some(ref mut bookmarks) => {
                if !bookmarks.contains(&path) {
                    bookmarks.push(path);
                }
            }
            None => {
                self.bookmarks = Some(vec![path]);
            }
        }
    }

    pub fn delete_bookmark(&mut self, path: String) {
        if let Some(ref mut bookmarks) = self.bookmarks {
            if let Some(idx) = bookmarks.iter().position(|bookmark| *bookmark == path) {
                bookmarks.remove(idx);
            }
        }
    }

    pub fn bookmark_paths(&self) -> Vec<PathBuf> {
        if let Some(bookmarks) = &self.bookmarks {
            bookmarks
                .iter()
                .filter_map(|b| {
                    if let Ok(expanded) = shellexpand::full(b) {
                        if let Ok(path) = PathBuf::from(expanded.to_string()).canonicalize() {
                            Some(path)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn add_mark(&mut self, path: String, index: usize) {
        let marks = &mut self.marks;
        match marks {
            Some(ref mut marks) => {
                marks.insert(index.to_string(), path);
            }
            None => {
                self.marks = Some(HashMap::from([(index.to_string(), path)]));
            }
        }
    }

    pub fn delete_mark(&mut self, index: usize) {
        if let Some(ref mut marks) = self.marks {
            marks.remove(&index.to_string());
        }
    }

    pub fn clear_marks(&mut self) {
        self.marks = None;
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

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PickerColorConfig {
    pub highlight_color: Option<Color>,
    pub highlight_text_color: Option<Color>,
    pub border_color: Option<Color>,
    pub info_color: Option<Color>,
    pub prompt_color: Option<Color>,
}

const HIGHLIGHT_COLOR_DEFAULT: Color = Color::LightBlue;
const HIGHLIGHT_TEXT_COLOR_DEFAULT: Color = Color::Black;
const BORDER_COLOR_DEFAULT: Color = Color::DarkGray;
const INFO_COLOR_DEFAULT: Color = Color::LightYellow;
const PROMPT_COLOR_DEFAULT: Color = Color::LightGreen;

impl PickerColorConfig {
    pub fn default_colors() -> Self {
        PickerColorConfig {
            highlight_color: Some(HIGHLIGHT_COLOR_DEFAULT),
            highlight_text_color: Some(HIGHLIGHT_TEXT_COLOR_DEFAULT),
            border_color: Some(BORDER_COLOR_DEFAULT),
            info_color: Some(INFO_COLOR_DEFAULT),
            prompt_color: Some(PROMPT_COLOR_DEFAULT),
        }
    }

    pub fn with_defaults(self) -> Self {
        PickerColorConfig {
            highlight_color: self.highlight_color.or(Some(HIGHLIGHT_COLOR_DEFAULT)),
            highlight_text_color: self
                .highlight_text_color
                .or(Some(HIGHLIGHT_TEXT_COLOR_DEFAULT)),
            border_color: self.border_color.or(Some(BORDER_COLOR_DEFAULT)),
            info_color: self.info_color.or(Some(INFO_COLOR_DEFAULT)),
            prompt_color: self.prompt_color.or(Some(PROMPT_COLOR_DEFAULT)),
        }
    }

    pub fn highlight_style(&self) -> Style {
        let mut style = Style::default()
            .bg(HIGHLIGHT_COLOR_DEFAULT)
            .fg(HIGHLIGHT_TEXT_COLOR_DEFAULT)
            .bold();

        if let Some(color) = self.highlight_color {
            style = style.bg(color);
        }

        if let Some(color) = self.highlight_text_color {
            style = style.fg(color);
        }

        style
    }

    pub fn border_color(&self) -> Color {
        if let Some(color) = self.border_color {
            color
        } else {
            BORDER_COLOR_DEFAULT
        }
    }

    pub fn info_color(&self) -> Color {
        if let Some(color) = self.info_color {
            color
        } else {
            INFO_COLOR_DEFAULT
        }
    }

    pub fn prompt_color(&self) -> Color {
        if let Some(color) = self.prompt_color {
            color
        } else {
            PROMPT_COLOR_DEFAULT
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub enum SessionSortOrderConfig {
    #[default]
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

#[derive(Debug, Default, Serialize, Deserialize, Copy, Clone, PartialEq, Eq)]
pub enum CloneRepoSwitchConfig {
    #[default]
    Always,
    Never,
    Foreground,
}

impl ValueEnum for CloneRepoSwitchConfig {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Always, Self::Never, Self::Foreground]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            CloneRepoSwitchConfig::Always => Some(clap::builder::PossibleValue::new("Always")),
            CloneRepoSwitchConfig::Never => Some(clap::builder::PossibleValue::new("Never")),
            CloneRepoSwitchConfig::Foreground => {
                Some(clap::builder::PossibleValue::new("Foreground"))
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct SessionConfig {
    pub create_script: Option<PathBuf>,
}
