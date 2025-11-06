use assert_cmd::Command;
use pretty_assertions::assert_eq;
use ratatui::style::Color;
use std::{fs, str::FromStr};
use tempfile::tempdir;
use tms::configs::{
    CloneRepoSwitchConfig, Config, PickerColorConfig, SearchDirectory, SessionSortOrderConfig,
};

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
    let picker_highlight_color = Color::from_str("#aaaaaa")?;
    let picker_highlight_text_color = Color::from_str("#bbbbbb")?;
    let picker_border_color = Color::from_str("#cccccc")?;
    let picker_info_color = Color::from_str("green")?;
    let picker_prompt_color = Color::from_str("#eeeeee")?;

    let expected_config = Config {
        default_session: Some(default_session.clone()),
        display_full_path: Some(false),
        search_submodules: Some(false),
        recursive_submodules: Some(false),
        switch_filter_unknown: Some(false),
        search_non_git_dirs: Some(false),
        session_sort_order: Some(SessionSortOrderConfig::Alphabetical),
        excluded_dirs: Some(vec![excluded_dir.clone()]),
        search_paths: None,
        search_dirs: Some(vec![SearchDirectory::new(
            fs::canonicalize(directory.path())?,
            depth,
        )]),
        sessions: None,
        picker_colors: Some(PickerColorConfig {
            highlight_color: Some(picker_highlight_color),
            highlight_text_color: Some(picker_highlight_text_color),
            border_color: Some(picker_border_color),
            info_color: Some(picker_info_color),
            prompt_color: Some(picker_prompt_color),
        }),
        shortcuts: None,
        bookmarks: None,
        session_configs: None,
        marks: None,
        clone_repo_switch: Some(CloneRepoSwitchConfig::Always),
        vcs_providers: None,
        input_position: None,
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
            "--search-non-git-dirs",
            "false",
            "--session-sort-order",
            "Alphabetical",
            "--excluded",
            &excluded_dir,
            "--picker-highlight-color",
            &picker_highlight_color.to_string(),
            "--picker-highlight-text-color",
            &picker_highlight_text_color.to_string(),
            "--picker-border-color",
            &picker_border_color.to_string(),
            "--picker-info-color",
            &picker_info_color.to_string(),
            "--picker-prompt-color",
            &picker_prompt_color.to_string(),
            "--clone-repo-switch",
            "Always",
        ]);

    tms.assert().success().code(0);

    let actual_config: Config = toml::from_str(&fs::read_to_string(&config_file_path).unwrap())?;

    assert_eq!(
        expected_config, actual_config,
        "tms config behaves as intended"
    );

    Ok(())
}

#[test]
fn tms_list_dirs_without_non_git_dirs() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let temp_dir_path = temp_dir.path();

    // Create a git repo
    let git_repo_path = temp_dir_path.join("git_repo");
    fs::create_dir(&git_repo_path)?;
    gix::init(&git_repo_path)?;

    // Create a non-git directory
    let non_git_dir_path = temp_dir_path.join("non_git_dir");
    fs::create_dir(&non_git_dir_path)?;

    let config = Config {
        search_non_git_dirs: Some(false),
        search_dirs: Some(vec![SearchDirectory::new(
            fs::canonicalize(temp_dir_path)?,
            1,
        )]),
        ..Default::default()
    };

    let config_file_path = temp_dir_path.join("config.toml");
    fs::write(&config_file_path, toml::to_string(&config)?)?;

    let mut tms = Command::cargo_bin("tms")?;
    tms.env("TMS_CONFIG_FILE", &config_file_path);
    tms.arg("--just-print"); // a fake argument to just print the list and not launch fzf
    let output = tms.assert().success().get_output().stdout.clone();
    let output = String::from_utf8(output)?;

    assert!(output.contains("git_repo"));
    assert!(!output.contains("non_git_dir"));

    Ok(())
}

#[test]
fn tms_list_dirs_with_non_git_dirs() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let temp_dir_path = temp_dir.path();

    // Create a git repo
    let git_repo_path = temp_dir_path.join("git_repo");
    fs::create_dir(&git_repo_path)?;
    gix::init(&git_repo_path)?;

    // Create a non-git directory
    let non_git_dir_path = temp_dir_path.join("non_git_dir");
    fs::create_dir(&non_git_dir_path)?;

    let config = Config {
        search_non_git_dirs: Some(true),
        search_dirs: Some(vec![SearchDirectory::new(
            fs::canonicalize(temp_dir_path)?,
            1,
        )]),
        ..Default::default()
    };

    let config_file_path = temp_dir_path.join("config.toml");
    fs::write(&config_file_path, toml::to_string(&config)?)?;

    let mut tms = Command::cargo_bin("tms")?;
    tms.env("TMS_CONFIG_FILE", &config_file_path);
    tms.arg("--just-print"); // a fake argument to just print the list and not launch fzf
    let output = tms.assert().success().get_output().stdout.clone();
    let output = String::from_utf8(output)?;

    assert!(output.contains("git_repo"));
    assert!(output.contains("non_git_dir"));

    Ok(())
}

#[test]
fn tms_list_dirs_with_override_flags() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let temp_dir_path = temp_dir.path();

    // Create a git repo
    let git_repo_path = temp_dir_path.join("git_repo");
    fs::create_dir(&git_repo_path)?;
    gix::init(&git_repo_path)?;

    // Create a non-git directory
    let non_git_dir_path = temp_dir_path.join("non_git_dir");
    fs::create_dir(&non_git_dir_path)?;

    // Test case 1: config says search non-git, but override with --search-git-dirs
    let config = Config {
        search_non_git_dirs: Some(true),
        search_dirs: Some(vec![SearchDirectory::new(
            fs::canonicalize(temp_dir_path)?,
            1,
        )]),
        ..Default::default()
    };
    let config_file_path = temp_dir_path.join("config.toml");
    fs::write(&config_file_path, toml::to_string(&config)?)?;

    let mut tms = Command::cargo_bin("tms")?;
    tms.env("TMS_CONFIG_FILE", &config_file_path);
    tms.args(["--just-print", "--search-git-dirs"]);
    let output = tms.assert().success().get_output().stdout.clone();
    let output = String::from_utf8(output)?;

    assert!(output.contains("git_repo"));
    assert!(!output.contains("non_git_dir"));

    // Test case 2: config says search only git, but override with --search-non-git-dirs
    let config = Config {
        search_non_git_dirs: Some(false),
        search_dirs: Some(vec![SearchDirectory::new(
            fs::canonicalize(temp_dir_path)?,
            1,
        )]),
        ..Default::default()
    };
    let config_file_path = temp_dir_path.join("config2.toml");
    fs::write(&config_file_path, toml::to_string(&config)?)?;

    let mut tms = Command::cargo_bin("tms")?;
    tms.env("TMS_CONFIG_FILE", &config_file_path);
    tms.args(["--just-print", "--search-non-git-dirs"]);
    let output = tms.assert().success().get_output().stdout.clone();
    let output = String::from_utf8(output)?;

    assert!(output.contains("git_repo"));
    assert!(output.contains("non_git_dir"));

    Ok(())
}
