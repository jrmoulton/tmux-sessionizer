use anyhow::{Context, Result};
use serde_derive::{Deserialize, Serialize};

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct OldConfig {
    pub search_path: String,
    pub excluded_dirs: Vec<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Config {
    pub search_paths: Vec<String>,
    pub excluded_dirs: Vec<String>,
    pub default_session: Option<String>,
    pub display_full_path: Option<bool>,
}

pub trait UpgradeConfig {
    /// Upgrade a configuration if necessary
    fn upgrade(self) -> Result<Config>;
}
impl UpgradeConfig for Result<Config, confy::ConfyError> {
    fn upgrade(self) -> Result<Config> {
        match self {
            Ok(defaults) => Ok(defaults),
            Err(_) => {
                let old_config = confy::load::<OldConfig>("tms")
                    .context("The configuration file does not match any internal format")?;
                let path = vec![old_config.search_path];
                Ok(Config {
                    search_paths: path,
                    excluded_dirs: old_config.excluded_dirs,
                    default_session: None,
                    display_full_path: None,
                })
            }
        }
    }
}
