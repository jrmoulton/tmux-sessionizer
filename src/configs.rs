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
}
