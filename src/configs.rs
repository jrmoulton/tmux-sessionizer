use error_stack::{IntoReport, Result, ResultExt};
use serde_derive::{Deserialize, Serialize};

use crate::{Suggestion, TmsError};

#[derive(Debug)]
pub(crate) enum ConfigError {
    NoDefaultSearchPath,
    WriteFailure,
    LoadError,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct OldConfig {
    pub search_path: String,
    pub excluded_dirs: Vec<String>,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct Config {
    pub search_paths: Vec<String>,
    pub excluded_dirs: Option<Vec<String>>,
    pub default_session: Option<String>,
    pub display_full_path: Option<bool>,
}

pub(crate) trait UpgradeConfig {
    /// Upgrade a configuration if necessary
    fn upgrade(self) -> Result<Config, TmsError>;
}
impl UpgradeConfig for std::result::Result<Config, confy::ConfyError> {
    fn upgrade(self) -> Result<Config, TmsError> {
        match self {
            Ok(defaults) => Ok(defaults),
            Err(_) => {
                let old_config = confy::load::<OldConfig>("tms")
                    .into_report()
                    .change_context(TmsError::ConfigError)
                    .attach_printable(
                        "The configuration file does not match any internal structure",
                    ).attach(Suggestion("Try using the `config` subcommand to configure options such as the search paths"))?;
                let path = vec![old_config.search_path];
                Ok(Config {
                    search_paths: path,
                    excluded_dirs: Some(old_config.excluded_dirs),
                    default_session: None,
                    display_full_path: None,
                })
            }
        }
    }
}
