use std::{collections::HashSet, path::PathBuf};

#[derive(Clone)]
pub enum PickerItem {
    Project { name: String, path: PathBuf },
    TmuxSession(String),
}

impl PickerItem {
    pub fn name(&self) -> &str {
        match self {
            PickerItem::Project { name, .. } => name,
            PickerItem::TmuxSession(name) => name,
        }
    }

    pub fn display_name(&self, running_sessions: &HashSet<String>) -> String {
        let name = self.name();
        if running_sessions.contains(name) {
            format!("* {}", name)
        } else {
            name.to_string()
        }
    }

    pub fn is_running(&self, running_sessions: &HashSet<String>) -> bool {
        running_sessions.contains(self.name())
    }
}
