# Plan: Implement searching for non-Git directories

## Goal

Modify `tmux-sessionizer` to optionally scan for all directories, not just Git repositories. This feature will be controlled by a new flag in the `config.toml` file, ensuring it's opt-in and doesn't alter the default behavior for existing users.

## Step-by-step Implementation

### 1. Configuration Update

- **File to modify:** `src/configs.rs`
- **Action:** Add a new boolean field to the `Config` struct.
  - **Field name:** `search_non_git_dirs`
  - **Type:** `bool`
  - **Default:** `false`
- This ensures the feature is disabled by default.

- **File to modify:** `src/cli.rs`
- **Action:** Add a new command-line argument to the `tms config` subcommand.
  - **Argument:** `--search-non-git-dirs <true|false>`
  - This will allow users to enable or disable the feature from the CLI, which will update the `config.toml` file.

### 2. Core Logic Modification

- **File to modify:** `src/repos.rs`
- **Action:** Update the directory scanning logic (likely in a function like `find_repos` or similar).
  - The current implementation probably iterates through directories and checks for the existence of a `.git` subdirectory.
  - The logic will be wrapped in a conditional check:
    - **If `config.search_non_git_dirs` is `false`:** Keep the existing behavior (search for Git repositories only).
    - **If `config.search_non_git_dirs` is `true`:** Modify the search to include all directories that are not otherwise excluded by the user's configuration (`excluded_dirs`). The search must continue to respect the `max_depths` setting.

### 3. Display and Integration

- **File to consider:** `src/session.rs` (or wherever the `Repo` / `Project` struct is defined)
- **Action:** Ensure that the struct used to represent a selectable item can handle non-Git directories gracefully.
  - Plain directories won't have Git-specific information (like worktrees or a `.git` folder). The existing struct should be reviewed to ensure this doesn't cause issues. It's expected that functions for Git-specific features (like worktree discovery) will simply do nothing for these new directory types.
  - No changes are anticipated for the fuzzy picker display. It should display the directory name, which is the desired behavior.

### 4. Testing

- **File to modify:** `tests/cli.rs`
- **Action:** Add new integration tests to validate the feature.
  - **Test Case 1 (Flag Off):**
    - Create a temporary directory structure containing both Git and non-Git subdirectories.
    - Run the scanner with `search_non_git_dirs` set to `false`.
    - Assert that only the Git repositories are found.
  - **Test Case 2 (Flag On):**
    - Use the same directory structure.
    - Run the scanner with `search_non_git_dirs` set to `true`.
    - Assert that all directories (both Git and non-Git) are found, respecting any exclusion rules.

### 5. Documentation

- **File to modify:** `README.md`
- **Action:** Update the documentation to reflect the new feature.
  - Explain the `search_non_git_dirs` option in the "Configuring defaults" section.
  - Briefly describe the new capability in the main "Usage" section.
- The help text for the `tms config` command will be automatically updated by the changes in `src/cli.rs`.
