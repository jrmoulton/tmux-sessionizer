use std::{collections::HashMap, fmt::Debug};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::de::Error as DeError;
use serde::ser::Error as SerError;
use serde::{Deserialize, Serialize};
use serde_derive::{Deserialize, Serialize};

use crate::error::TmsError;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, PartialOrd)]
pub struct Key {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let modifiers = self
            .modifiers
            .iter()
            .filter_map(modifier_to_string)
            .collect::<Vec<&str>>()
            .join("-");
        let code = keycode_to_string(self.code)
            .ok_or(TmsError::ConfigError)
            .map_err(S::Error::custom)?;
        let formatted = if modifiers.is_empty() {
            code
        } else {
            format!("{}-{}", modifiers, code)
        };
        serializer.serialize_str(&formatted)
    }
}

fn modifier_to_string<'a>(modifier: KeyModifiers) -> Option<&'a str> {
    match modifier {
        KeyModifiers::SHIFT => Some("shift"),
        KeyModifiers::CONTROL => Some("ctrl"),
        KeyModifiers::ALT => Some("alt"),
        KeyModifiers::SUPER => Some("super"),
        KeyModifiers::HYPER => Some("hyper"),
        KeyModifiers::META => Some("meta"),
        _ => None,
    }
}

fn keycode_to_string(code: KeyCode) -> Option<String> {
    match code {
        KeyCode::Esc => Some("esc".to_owned()),
        KeyCode::Enter => Some("enter".to_owned()),
        KeyCode::Left => Some("left".to_owned()),
        KeyCode::Right => Some("right".to_owned()),
        KeyCode::Up => Some("up".to_owned()),
        KeyCode::Down => Some("down".to_owned()),
        KeyCode::Home => Some("home".to_owned()),
        KeyCode::End => Some("end".to_owned()),
        KeyCode::PageUp => Some("pageup".to_owned()),
        KeyCode::PageDown => Some("pagedown".to_owned()),
        KeyCode::BackTab => Some("backtab".to_owned()),
        KeyCode::Backspace => Some("backspace".to_owned()),
        KeyCode::Delete => Some("delete".to_owned()),
        KeyCode::Insert => Some("insert".to_owned()),
        KeyCode::F(1) => Some("f1".to_owned()),
        KeyCode::F(2) => Some("f2".to_owned()),
        KeyCode::F(3) => Some("f3".to_owned()),
        KeyCode::F(4) => Some("f4".to_owned()),
        KeyCode::F(5) => Some("f5".to_owned()),
        KeyCode::F(6) => Some("f6".to_owned()),
        KeyCode::F(7) => Some("f7".to_owned()),
        KeyCode::F(8) => Some("f8".to_owned()),
        KeyCode::F(9) => Some("f9".to_owned()),
        KeyCode::F(10) => Some("f10".to_owned()),
        KeyCode::F(11) => Some("f11".to_owned()),
        KeyCode::F(12) => Some("f12".to_owned()),
        KeyCode::Char(' ') => Some("space".to_owned()),
        KeyCode::Tab => Some("tab".to_owned()),
        KeyCode::Char(c) => Some(String::from(c)),
        _ => None,
    }
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value: String = Deserialize::deserialize(deserializer)?;
        let tokens = value.split('-').collect::<Vec<&str>>();

        let mut modifiers = KeyModifiers::empty();

        for modifier in tokens.iter().take(tokens.len() - 1) {
            match modifier.to_ascii_lowercase().as_ref() {
                "shift" => modifiers.insert(KeyModifiers::SHIFT),
                "ctrl" => modifiers.insert(KeyModifiers::CONTROL),
                "alt" => modifiers.insert(KeyModifiers::ALT),
                "super" => modifiers.insert(KeyModifiers::SUPER),
                "hyper" => modifiers.insert(KeyModifiers::HYPER),
                "meta" => modifiers.insert(KeyModifiers::META),
                _ => {}
            };
        }

        let last = tokens
            .last()
            .ok_or(TmsError::ConfigError)
            .map_err(D::Error::custom)?;

        let code = match last.to_ascii_lowercase().as_ref() {
            "esc" => KeyCode::Esc,
            "enter" => KeyCode::Enter,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "backtab" => KeyCode::BackTab,
            "backspace" => KeyCode::Backspace,
            "del" => KeyCode::Delete,
            "delete" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "ins" => KeyCode::Insert,
            "f1" => KeyCode::F(1),
            "f2" => KeyCode::F(2),
            "f3" => KeyCode::F(3),
            "f4" => KeyCode::F(4),
            "f5" => KeyCode::F(5),
            "f6" => KeyCode::F(6),
            "f7" => KeyCode::F(7),
            "f8" => KeyCode::F(8),
            "f9" => KeyCode::F(9),
            "f10" => KeyCode::F(10),
            "f11" => KeyCode::F(11),
            "f12" => KeyCode::F(12),
            "space" => KeyCode::Char(' '),
            "tab" => KeyCode::Tab,
            c if c.len() == 1 => KeyCode::Char(c.chars().next().unwrap()),
            _ => {
                return Err(D::Error::custom(TmsError::ConfigError));
            }
        };
        Ok(Key { code, modifiers })
    }
}

impl From<KeyEvent> for Key {
    fn from(value: KeyEvent) -> Self {
        Self {
            code: value.code,
            modifiers: value.modifiers,
        }
    }
}

pub type Keymap = HashMap<Key, PickerAction>;

pub fn default_keymap() -> Keymap {
    HashMap::from([
        (
            Key {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::Cancel,
        ),
        (
            Key {
                code: KeyCode::Esc,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::Cancel,
        ),
        (
            Key {
                code: KeyCode::Enter,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::Confirm,
        ),
        (
            Key {
                code: KeyCode::Delete,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::Delete,
        ),
        (
            Key {
                code: KeyCode::Char('d'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::Delete,
        ),
        (
            Key {
                code: KeyCode::Backspace,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::Backspace,
        ),
        (
            Key {
                code: KeyCode::Down,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::MoveDown,
        ),
        (
            Key {
                code: KeyCode::Char('j'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::MoveDown,
        ),
        (
            Key {
                code: KeyCode::Char('n'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::MoveDown,
        ),
        (
            Key {
                code: KeyCode::Up,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::MoveUp,
        ),
        (
            Key {
                code: KeyCode::Char('k'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::MoveUp,
        ),
        (
            Key {
                code: KeyCode::Char('p'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::MoveUp,
        ),
        (
            Key {
                code: KeyCode::Left,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::CursorLeft,
        ),
        (
            Key {
                code: KeyCode::Right,
                modifiers: KeyModifiers::empty(),
            },
            PickerAction::CursorRight,
        ),
        (
            Key {
                code: KeyCode::Char('w'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::DeleteWord,
        ),
        (
            Key {
                code: KeyCode::Char('u'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::DeleteToLineStart,
        ),
        (
            Key {
                code: KeyCode::Char('a'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::MoveToLineStart,
        ),
        (
            Key {
                code: KeyCode::Char('e'),
                modifiers: KeyModifiers::CONTROL,
            },
            PickerAction::MoveToLineEnd,
        ),
    ])
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum PickerAction {
    #[serde(rename = "")]
    Noop,
    #[serde(rename = "cancel")]
    Cancel,
    #[serde(rename = "confirm")]
    Confirm,
    #[serde(rename = "backspace")]
    Backspace,
    #[serde(rename = "delete")]
    Delete,
    #[serde(rename = "move_up")]
    MoveUp,
    #[serde(rename = "move_down")]
    MoveDown,
    #[serde(rename = "cursor_left")]
    CursorLeft,
    #[serde(rename = "cursor_right")]
    CursorRight,
    #[serde(rename = "delete_word")]
    DeleteWord,
    #[serde(rename = "delete_to_line_start")]
    DeleteToLineStart,
    #[serde(rename = "delete_to_line_end")]
    DeleteToLineEnd,
    #[serde(rename = "move_to_line_start")]
    MoveToLineStart,
    #[serde(rename = "move_to_line_end")]
    MoveToLineEnd,
}
