# Plan: Unify Projects and Running Sessions in the Default Picker

## Goal

Modify the default `tms` command to display a single, unified list of available projects (from disk) and currently running tmux sessions. This list will be deduplicated, so a project that is already running as a session appears only once, marked as "running". This provides a seamless experience for both creating new sessions and switching to existing ones from one interface.

## The `PickerItem` Approach

Instead of forcing tmux sessions into a `Repo` object, we will introduce a new, unified enum `PickerItem` that can cleanly represent all possible selectable items.

```rust
// In a new file, e.g., `src/picker_item.rs`
pub enum PickerItem {
    Project(Session),
    TmuxSession(String),
}

impl PickerItem {
    pub fn display_name(&self) -> String {
        // ... logic to get name from Session or the TmuxSession String
    }

    pub fn is_running(&self, running_sessions: &HashSet<String>) -> bool {
        // ... logic to check if the item is in the set of running sessions
    }
}
```

## Step-by-step Implementation

### 1. Configuration

-   **File to modify:** `src/configs.rs`
-   **Action:** Add a new `search_tmux_sessions: Option<bool>` field to the `Config` and `ConfigExport` structs. This will be `true` by default to enable the new unified view.
-   **File to modify:** `src/cli.rs`
-   **Action:** Add a `--search-tmux-sessions <true|false>` command-line argument to the `tms config` subcommand to allow users to toggle this feature.

### 2. Core Logic Modification

-   **File to modify:** `src/tmux.rs`
-   **Action:** Create a new public function, `get_running_sessions()`, that executes `tmux list-sessions` and returns a `Result<HashSet<String>>` containing the names of all active sessions.

-   **File to modify:** `src/repos.rs`
-   **Action:** Refactor the `find_repos` function (or create a new one, e.g., `get_picker_items`).
    1.  It will first call `tmux::get_running_sessions()` to get all active session names.
    2.  It will then scan the filesystem for projects (`Session` objects) as it does now.
    3.  It will iterate through the found projects, creating a `PickerItem::Project(session)` for each. It will use the set of running sessions to mark projects that are already active, preventing duplicates.
    4.  Finally, it will iterate through the remaining running sessions (those that did not correspond to a found project) and create a `PickerItem::TmuxSession(name)` for each.
    5.  The function will return a `Vec<PickerItem>`.

### 3. Picker Integration

-   **File to create:** `src/picker_item.rs`
-   **Action:** Define the `PickerItem` enum and its associated methods as described above. It will need a method to provide the string for the fuzzy finder.

-   **Files to modify:** `src/picker/mod.rs` and `src/lib.rs`
-   **Action:** The current picker logic in `get_single_selection` and the `Picker` struct is hardcoded to work with `String`. This needs to be adapted to work with `Vec<PickerItem>`.
    -   The `Picker` will be initialized with `Vec<PickerItem>`.
    -   It will use the `display_name()` method of each `PickerItem` to populate the fuzzy finder list.
    -   The `run` method will now return `Result<Option<PickerItem>>`.

### 4. Main Application Logic

-   **File to modify:** `src/main.rs`
-   **Action:**
    1.  Update the main execution flow to call the new `get_picker_items` function.
    2.  Pass the resulting `Vec<PickerItem>` to the `get_single_selection` function.
    3.  Inspect the returned `PickerItem`.
        -   If the item was a `PickerItem::Project`, use the inner `Session` to create a new tmux session as is currently done.
        -   If the item was a `PickerItem::TmuxSession` (or a project that was already running), execute the tmux `switch-client` command to attach to that session.

### 5. Testing

-   **File to modify:** `tests/cli.rs`
-   **Action:** Add new integration tests.
    -   **Test Unified List:** Create mock projects and mock running tmux sessions. Run `tms` and assert that the picker receives a correctly unified and deduplicated list of items.
    -   **Test Feature Flag:** Test that setting `search_tmux_sessions = false` results in only projects being shown.
    -   **Test Actions:** Mock the picker's selection and assert that selecting a running session results in a `switch-client` command, while selecting a non-running project results in a `new-session` command.
