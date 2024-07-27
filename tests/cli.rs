use assert_cmd::Command;
use pretty_assertions::assert_eq;
use std::fs;
use tempfile::tempdir;
use tms::configs::{Config, PickerColorConfig, SearchDirectory, SessionSortOrderConfig};

#[test]
fn tms_fails_with_missing_config() -> anyhow::Result<()> {
    let dir = tempdir()?;
    let file_path = dir.path().join("config.toml");

    let mut tms = Command::cargo_bin("tms")?;

    tms.env("TMS_CONFIG_FILE", file_path);

    tms.assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("Error"))
        .stderr(predicates::str::contains(
            "No default search path was found",
        ));

    Ok(())
}

#[test]
fn tms_config() -> anyhow::Result<()> {
    let directory = tempdir()?;
    let config_file_path = directory.path().join("config.toml");

    let depth = 1;
    let default_session = String::from("my_default_session");
    let excluded_dir = String::from("/exclude/this/directory");
    let picker_highlight_color = String::from("#aaaaaa");
    let picker_highlight_text_color = String::from("#bbbbbb");
    let picker_border_color = String::from("#cccccc");
    let picker_info_color = String::from("#dddddd");
    let picker_prompt_color = String::from("#eeeeee");

    let expected_config = Config {
        default_session: Some(default_session.clone()),
        display_full_path: Some(false),
        search_submodules: Some(false),
        recursive_submodules: Some(false),
        switch_filter_unknown: Some(false),
        session_sort_order: Some(SessionSortOrderConfig::Alphabetical),
        excluded_dirs: Some(vec![excluded_dir.clone()]),
        search_paths: None,
        search_dirs: Some(vec![SearchDirectory::new(
            fs::canonicalize(directory.path())?,
            depth,
        )]),
        sessions: None,
        picker_colors: Some(PickerColorConfig {
            highlight_color: Some(picker_highlight_color.clone()),
            highlight_text_color: Some(picker_highlight_text_color.clone()),
            border_color: Some(picker_border_color.clone()),
            info_color: Some(picker_info_color.clone()),
            prompt_color: Some(picker_prompt_color.clone()),
        }),
        shortcuts: None,
        bookmarks: None,
        session_configs: None,
    };

    let mut tms = Command::cargo_bin("tms")?;

    tms.env("TMS_CONFIG_FILE", &config_file_path)
        .arg("config")
        .args([
            "--paths",
            directory.path().to_str().unwrap(),
            "--max-depths",
            &depth.to_string(),
            "--session",
            &default_session,
            "--full-path",
            "false",
            "--search-submodules",
            "false",
            "--recursive-submodules",
            "false",
            "--switch-filter-unknown",
            "false",
            "--session-sort-order",
            "Alphabetical",
            "--excluded",
            &excluded_dir,
            "--picker-highlight-color",
            &picker_highlight_color,
            "--picker-highlight-text-color",
            &picker_highlight_text_color,
            "--picker-border-color",
            &picker_border_color,
            "--picker-info-color",
            &picker_info_color,
            "--picker-prompt-color",
            &picker_prompt_color,
        ]);

    tms.assert().success().code(0);

    let actual_config: Config = toml::from_str(&fs::read_to_string(&config_file_path).unwrap())?;

    assert_eq!(
        expected_config, actual_config,
        "tms config behaves as intended"
    );

    Ok(())
}
