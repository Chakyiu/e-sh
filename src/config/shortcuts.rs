use std::path::Path;
use std::{fmt, fs};

use anyhow::{Context, Result};
use egui::{Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};

const SHORTCUTS_FILE: &str = "shortcuts.toml";

/// User-configurable keyboard shortcuts (stored as `modifier+key` strings).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppShortcuts {
    /// Fuzzy command palette.
    pub command_palette: String,
    pub toggle_sidebar: String,
    pub close_tab: String,
    pub quit: String,
    /// Find in terminal scrollback (terminal tab, focused).
    pub find_in_terminal: String,
    /// Clear terminal scrollback / viewport (terminal tab, focused).
    pub clear_terminal: String,
}

impl Default for AppShortcuts {
    fn default() -> Self {
        Self {
            command_palette: "cmd+shift+p".into(),
            toggle_sidebar: "cmd+b".into(),
            close_tab: "cmd+w".into(),
            quit: "cmd+q".into(),
            find_in_terminal: "cmd+f".into(),
            clear_terminal: default_clear_terminal_binding(),
        }
    }
}

fn default_clear_terminal_binding() -> String {
    if cfg!(target_os = "macos") {
        "cmd+k".into()
    } else {
        "ctrl+k".into()
    }
}

impl AppShortcuts {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(SHORTCUTS_FILE);
        if !path.exists() {
            return Self::default();
        }
        match fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "failed parsing shortcuts.toml, using defaults");
                Self::default()
            }),
            Err(e) => {
                tracing::warn!(error = %e, "failed reading shortcuts.toml, using defaults");
                Self::default()
            }
        }
    }

    pub fn save(&self, config_dir: &Path) -> Result<()> {
        fs::create_dir_all(config_dir)
            .with_context(|| format!("creating {}", config_dir.display()))?;
        let path = config_dir.join(SHORTCUTS_FILE);
        let text = toml::to_string_pretty(self).context("serializing shortcuts")?;
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn command_palette_shortcut(&self) -> KeyboardShortcut {
        self.parse_binding(&self.command_palette)
            .unwrap_or_else(|| Self::default().command_palette_shortcut())
    }

    pub fn toggle_sidebar_shortcut(&self) -> KeyboardShortcut {
        self.parse_binding(&self.toggle_sidebar)
            .unwrap_or_else(|| Self::default().toggle_sidebar_shortcut())
    }

    pub fn close_tab_shortcut(&self) -> KeyboardShortcut {
        self.parse_binding(&self.close_tab)
            .unwrap_or_else(|| Self::default().close_tab_shortcut())
    }

    pub fn quit_shortcut(&self) -> KeyboardShortcut {
        self.parse_binding(&self.quit)
            .unwrap_or_else(|| Self::default().quit_shortcut())
    }

    pub fn find_in_terminal_shortcut(&self) -> KeyboardShortcut {
        self.parse_binding(&self.find_in_terminal)
            .unwrap_or_else(|| Self::default().find_in_terminal_shortcut())
    }

    pub fn clear_terminal_shortcut(&self) -> KeyboardShortcut {
        self.parse_binding(&self.clear_terminal)
            .unwrap_or_else(|| Self::default().clear_terminal_shortcut())
    }

    /// Human-readable label for UI (palette hints, settings).
    pub fn display_label(binding: &str) -> String {
        let parts: Vec<&str> = binding.split('+').map(str::trim).collect();
        let mut out = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i + 1 == parts.len() {
                out.push_str(&part.to_uppercase());
            } else {
                out.push_str(&display_modifier(part));
            }
        }
        out
    }

    pub fn parse_binding(&self, binding: &str) -> Option<KeyboardShortcut> {
        parse_binding(binding)
    }

    pub fn validate_binding(binding: &str) -> Result<(), ShortcutParseError> {
        parse_binding(binding).ok_or_else(|| ShortcutParseError(binding.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ShortcutParseError(pub String);

impl fmt::Display for ShortcutParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid shortcut {:?}", self.0)
    }
}

impl std::error::Error for ShortcutParseError {}

fn display_modifier(part: &str) -> String {
    match part.to_ascii_lowercase().as_str() {
        "cmd" | "command" | "meta" | "super" | "win" => {
            if cfg!(target_os = "macos") {
                "\u{2318}".into()
            } else {
                "Ctrl+".into()
            }
        }
        "ctrl" | "control" => "Ctrl+".into(),
        "shift" => {
            if cfg!(target_os = "macos") {
                "\u{21e7}".into()
            } else {
                "Shift+".into()
            }
        }
        "alt" | "option" => {
            if cfg!(target_os = "macos") {
                "\u{2325}".into()
            } else {
                "Alt+".into()
            }
        }
        other => format!("{other}+"),
    }
}

fn parse_binding(binding: &str) -> Option<KeyboardShortcut> {
    let binding = binding.trim();
    if binding.is_empty() {
        return None;
    }
    let parts: Vec<&str> = binding.split('+').map(str::trim).collect();
    if parts.is_empty() {
        return None;
    }
    let key_name = parts.last()?;
    let key = parse_key(key_name)?;
    let mut mods = Modifiers::NONE;
    for part in &parts[..parts.len() - 1] {
        mods |= parse_modifier(part)?;
    }
    Some(KeyboardShortcut::new(mods, key))
}

fn parse_modifier(part: &str) -> Option<Modifiers> {
    match part.to_ascii_lowercase().as_str() {
        "cmd" | "command" | "meta" | "super" | "win" => Some(Modifiers::COMMAND),
        "ctrl" | "control" => Some(Modifiers::CTRL),
        "shift" => Some(Modifiers::SHIFT),
        "alt" | "option" => Some(Modifiers::ALT),
        _ => None,
    }
}

fn parse_key(name: &str) -> Option<Key> {
    use Key::*;
    match name.to_ascii_lowercase().as_str() {
        "a" => Some(A),
        "b" => Some(B),
        "c" => Some(C),
        "d" => Some(D),
        "e" => Some(E),
        "f" => Some(F),
        "g" => Some(G),
        "h" => Some(H),
        "i" => Some(I),
        "j" => Some(J),
        "k" => Some(K),
        "l" => Some(L),
        "m" => Some(M),
        "n" => Some(N),
        "o" => Some(O),
        "p" => Some(P),
        "q" => Some(Q),
        "r" => Some(R),
        "s" => Some(S),
        "t" => Some(T),
        "u" => Some(U),
        "v" => Some(V),
        "w" => Some(W),
        "x" => Some(X),
        "y" => Some(Y),
        "z" => Some(Z),
        "0" => Some(Num0),
        "1" => Some(Num1),
        "2" => Some(Num2),
        "3" => Some(Num3),
        "4" => Some(Num4),
        "5" => Some(Num5),
        "6" => Some(Num6),
        "7" => Some(Num7),
        "8" => Some(Num8),
        "9" => Some(Num9),
        "f1" => Some(F1),
        "f2" => Some(F2),
        "f3" => Some(F3),
        "f4" => Some(F4),
        "f5" => Some(F5),
        "f6" => Some(F6),
        "f7" => Some(F7),
        "f8" => Some(F8),
        "f9" => Some(F9),
        "f10" => Some(F10),
        "f11" => Some(F11),
        "f12" => Some(F12),
        "space" => Some(Space),
        "tab" => Some(Tab),
        "enter" | "return" => Some(Enter),
        "escape" | "esc" => Some(Escape),
        "backspace" => Some(Backspace),
        "delete" | "del" => Some(Delete),
        "home" => Some(Home),
        "end" => Some(End),
        "pageup" => Some(PageUp),
        "pagedown" => Some(PageDown),
        "arrowup" | "up" => Some(ArrowUp),
        "arrowdown" | "down" => Some(ArrowDown),
        "arrowleft" | "left" => Some(ArrowLeft),
        "arrowright" | "right" => Some(ArrowRight),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_command_palette_default() {
        let sc = AppShortcuts::default();
        assert!(sc.parse_binding("cmd+shift+p").is_some());
        assert!(sc.parse_binding(&sc.command_palette).is_some());
    }
}
