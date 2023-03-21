#![allow(unused)]
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;

use once_cell::sync::Lazy;

static TMUX_CONFIG_PATH: Lazy<String> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_tmux.conf")
        .to_str()
        .unwrap()
        .to_string()
});
