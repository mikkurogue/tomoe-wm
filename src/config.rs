use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

/// Main configuration struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// General settings
    #[serde(default)]
    pub general: GeneralConfig,

    /// Tiling configuration
    #[serde(default)]
    pub tiling: TilingConfig,

    /// Keybindings
    #[serde(default)]
    pub keybinds: HashMap<String, KeyAction>,

    /// Commands to run on startup
    #[serde(default)]
    pub on_start: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Gap between windows in pixels
    #[serde(default = "default_gap")]
    pub gap: i32,

    /// Outer margin around the workspace
    #[serde(default = "default_margin")]
    pub margin: i32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            gap: default_gap(),
            margin: default_margin(),
        }
    }
}

fn default_gap() -> i32 {
    8
}

fn default_margin() -> i32 {
    8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TilingConfig {
    /// Default window width as percentage of screen (0.0 - 1.0)
    #[serde(default = "default_window_width")]
    pub default_window_width: f64,

    /// Enable scrolling tiling (niri-style)
    #[serde(default = "default_true")]
    pub scrolling: bool,
}

impl Default for TilingConfig {
    fn default() -> Self {
        Self {
            default_window_width: default_window_width(),
            scrolling: true,
        }
    }
}

fn default_window_width() -> f64 {
    0.5
}

fn default_true() -> bool {
    true
}

/// Action to perform when a keybind is triggered
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum KeyAction {
    /// Spawn a command
    Spawn { command: String },
    /// Close the focused window
    Close,
    /// Focus the next window
    FocusNext,
    /// Focus the previous window
    FocusPrev,
    /// Scroll the view left
    ScrollLeft,
    /// Scroll the view right
    ScrollRight,
    /// Toggle fullscreen for focused window
    Fullscreen,
    /// Quit the compositor
    Quit,
}

impl Default for Config {
    fn default() -> Self {
        let mut keybinds = HashMap::new();

        // Use Super+Ctrl as base modifier for nested compositor compatibility
        keybinds.insert(
            "Super+Ctrl+Return".to_string(),
            KeyAction::Spawn {
                command: "alacritty".to_string(),
            },
        );
        keybinds.insert(
            "Super+Ctrl+t".to_string(),
            KeyAction::Spawn {
                command: "alacritty".to_string(),
            },
        );
        keybinds.insert(
            "Super+Ctrl+d".to_string(),
            KeyAction::Spawn {
                command: "wofi --show drun".to_string(),
            },
        );
        keybinds.insert("Super+Ctrl+q".to_string(), KeyAction::Close);
        keybinds.insert("Super+Ctrl+h".to_string(), KeyAction::FocusPrev);
        keybinds.insert("Super+Ctrl+l".to_string(), KeyAction::FocusNext);
        keybinds.insert("Super+Ctrl+Left".to_string(), KeyAction::ScrollLeft);
        keybinds.insert("Super+Ctrl+Right".to_string(), KeyAction::ScrollRight);
        keybinds.insert("Super+Ctrl+f".to_string(), KeyAction::Fullscreen);
        keybinds.insert("Super+Ctrl+Shift+e".to_string(), KeyAction::Quit);

        Self {
            general: GeneralConfig::default(),
            tiling: TilingConfig::default(),
            keybinds,
            on_start: vec![
                // Example startup commands (commented out in default)
                // "waybar".to_string(),
            ],
        }
    }
}

impl Config {
    /// Get the config directory path
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("tomoe")
    }

    /// Get the config file path
    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Load configuration from file, or create default if it doesn't exist
    pub fn load() -> Self {
        let config_path = Self::config_path();

        if config_path.exists() {
            info!("Loading config from {:?}", config_path);
            match fs::read_to_string(&config_path) {
                Ok(content) => match toml::from_str(&content) {
                    Ok(config) => {
                        info!("Config loaded successfully");
                        return config;
                    }
                    Err(e) => {
                        warn!("Failed to parse config: {}, using defaults", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read config file: {}, using defaults", e);
                }
            }
        } else {
            info!(
                "No config file found, creating default at {:?}",
                config_path
            );
            let config = Config::default();
            if let Err(e) = config.save() {
                warn!("Failed to save default config: {}", e);
            }
            return config;
        }

        Config::default()
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let config_dir = Self::config_dir();
        let config_path = Self::config_path();

        // Create config directory if it doesn't exist
        fs::create_dir_all(&config_dir)?;

        // Generate TOML with comments
        let toml_content = self.to_toml_with_comments();
        fs::write(&config_path, toml_content)?;

        info!("Config saved to {:?}", config_path);
        Ok(())
    }

    /// Generate TOML string with helpful comments
    fn to_toml_with_comments(&self) -> String {
        let mut output = String::new();

        output.push_str("# Tomoe Window Manager Configuration\n");
        output.push_str("# ==================================\n\n");

        // Root-level settings (before any section headers)
        output.push_str("# Commands to run on startup\n");
        output.push_str("# Example: on_start = [\"waybar\", \"mako\"]\n");
        if self.on_start.is_empty() {
            output.push_str("on_start = []\n\n");
        } else {
            output.push_str(&format!("on_start = {:?}\n\n", self.on_start));
        }

        output.push_str("# General settings\n");
        output.push_str("[general]\n");
        output.push_str(&format!("# Gap between windows in pixels\n"));
        output.push_str(&format!("gap = {}\n", self.general.gap));
        output.push_str(&format!("# Outer margin around the workspace\n"));
        output.push_str(&format!("margin = {}\n\n", self.general.margin));

        output.push_str("# Tiling configuration\n");
        output.push_str("[tiling]\n");
        output.push_str("# Default window width as percentage of screen (0.0 - 1.0)\n");
        output.push_str(&format!(
            "default_window_width = {}\n",
            self.tiling.default_window_width
        ));
        output.push_str("# Enable scrolling tiling (niri-style horizontal scrolling)\n");
        output.push_str(&format!("scrolling = {}\n\n", self.tiling.scrolling));

        output.push_str("# Keybindings\n");
        output.push_str("# Format: \"Modifier+Key\" = { action = \"action_name\", ... }\n");
        output.push_str("# Available modifiers: Super, Ctrl, Alt, Shift\n");
        output.push_str("# Available actions:\n");
        output.push_str("#   - spawn: { action = \"spawn\", command = \"program\" }\n");
        output.push_str("#   - close: { action = \"close\" }\n");
        output.push_str("#   - focus_next: { action = \"focus_next\" }\n");
        output.push_str("#   - focus_prev: { action = \"focus_prev\" }\n");
        output.push_str("#   - scroll_left: { action = \"scroll_left\" }\n");
        output.push_str("#   - scroll_right: { action = \"scroll_right\" }\n");
        output.push_str("#   - fullscreen: { action = \"fullscreen\" }\n");
        output.push_str("#   - quit: { action = \"quit\" }\n");
        output.push_str("#\n");
        output.push_str(
            "# Note: Using Super+Ctrl as base modifier for nested compositor compatibility\n",
        );
        output.push_str("[keybinds]\n");

        for (key, action) in &self.keybinds {
            let action_str = match action {
                KeyAction::Spawn { command } => {
                    format!("{{ action = \"spawn\", command = \"{}\" }}", command)
                }
                KeyAction::Close => "{ action = \"close\" }".to_string(),
                KeyAction::FocusNext => "{ action = \"focus_next\" }".to_string(),
                KeyAction::FocusPrev => "{ action = \"focus_prev\" }".to_string(),
                KeyAction::ScrollLeft => "{ action = \"scroll_left\" }".to_string(),
                KeyAction::ScrollRight => "{ action = \"scroll_right\" }".to_string(),
                KeyAction::Fullscreen => "{ action = \"fullscreen\" }".to_string(),
                KeyAction::Quit => "{ action = \"quit\" }".to_string(),
            };
            output.push_str(&format!("\"{}\" = {}\n", key, action_str));
        }

        output
    }
}

/// Parsed keybind for efficient matching
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ParsedKeybind {
    pub modifiers: Modifiers,
    pub key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub logo: bool, // Super/Meta key
}

impl ParsedKeybind {
    /// Parse a keybind string like "Super+Ctrl+t"
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('+').collect();
        if parts.is_empty() {
            return None;
        }

        let mut modifiers = Modifiers::default();
        let mut key = String::new();

        for (i, part) in parts.iter().enumerate() {
            let part_lower = part.to_lowercase();
            if i == parts.len() - 1 {
                // Last part is the key
                key = part.to_string();
            } else {
                // Modifier
                match part_lower.as_str() {
                    "super" | "logo" | "meta" | "mod4" => modifiers.logo = true,
                    "ctrl" | "control" => modifiers.ctrl = true,
                    "alt" | "mod1" => modifiers.alt = true,
                    "shift" => modifiers.shift = true,
                    _ => {
                        // Unknown modifier, might be part of the key name
                        key = parts[i..].join("+");
                        break;
                    }
                }
            }
        }

        if key.is_empty() {
            return None;
        }

        Some(Self { modifiers, key })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_keybind() {
        let kb = ParsedKeybind::parse("Super+Ctrl+t").unwrap();
        assert!(kb.modifiers.logo);
        assert!(kb.modifiers.ctrl);
        assert!(!kb.modifiers.alt);
        assert!(!kb.modifiers.shift);
        assert_eq!(kb.key, "t");

        let kb2 = ParsedKeybind::parse("Super+Ctrl+Shift+Return").unwrap();
        assert!(kb2.modifiers.logo);
        assert!(kb2.modifiers.ctrl);
        assert!(kb2.modifiers.shift);
        assert_eq!(kb2.key, "Return");
    }
}
