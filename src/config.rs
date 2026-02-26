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

    #[serde(default)]
    pub keyboard: KeyboardConfig,

    /// Tiling configuration
    #[serde(default)]
    pub tiling: TilingConfig,

    /// Output/monitor configuration
    #[serde(default)]
    pub outputs: Vec<OutputConfig>,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardConfig {
    /// XKB rules (empty = system default)
    #[serde(default)]
    pub rules: Option<String>,

    /// XKB model (empty = system default)
    #[serde(default)]
    pub model: Option<String>,

    /// Keyboard layout(s), comma-separated (e.g., "us,de")
    #[serde(default)]
    pub layout: Option<String>,

    /// Layout variant(s), comma-separated
    #[serde(default)]
    pub variant: Option<String>,

    /// XKB options (e.g., "grp:alt_shift_toggle,ctrl:nocaps")
    #[serde(default)]
    pub options: Option<String>,

    /// Key repeat delay in milliseconds
    #[serde(default = "default_repeat_delay")]
    pub repeat_delay: i32,

    /// Key repeat rate in milliseconds
    #[serde(default = "default_repeat_rate")]
    pub repeat_rate: i32,
}

impl Default for KeyboardConfig {
    fn default() -> Self {
        Self {
            rules: None,
            model: None,
            layout: None,
            variant: None,
            options: None,
            repeat_delay: default_repeat_delay(),
            repeat_rate: default_repeat_rate(),
        }
    }
}

fn default_repeat_delay() -> i32 {
    200
}

fn default_repeat_rate() -> i32 {
    25
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

/// Output/monitor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    /// Output name (e.g., "HDMI-A-1", "eDP-1", "DP-1")
    pub name: String,

    /// X position in pixels (optional, auto-arranged if not specified)
    #[serde(default)]
    pub x: Option<i32>,

    /// Y position in pixels (optional, auto-arranged if not specified)
    #[serde(default)]
    pub y: Option<i32>,

    /// Output scale factor (e.g., 1.0, 1.5, 2.0)
    #[serde(default)]
    pub scale: Option<f64>,

    /// Transform/rotation: "normal", "90", "180", "270", "flipped", "flipped-90", etc.
    #[serde(default)]
    pub transform: Option<String>,

    /// Preferred mode (e.g., "1920x1080@60" or "preferred")
    #[serde(default)]
    pub mode: Option<String>,

    /// Whether this output is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            x: None,
            y: None,
            scale: None,
            transform: None,
            mode: None,
            enabled: true,
        }
    }
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

    // Workspace actions
    /// Switch to a specific workspace (1-indexed for user convenience)
    SwitchToWorkspace { workspace: usize },
    /// Switch to the next workspace
    NextWorkspace,
    /// Switch to the previous workspace
    PrevWorkspace,
    /// Move focused window to a specific workspace (1-indexed)
    MoveToWorkspace { workspace: usize },
    /// Create a new workspace
    NewWorkspace,
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

        // Workspace navigation (Super+Ctrl+1-9 to switch)
        for i in 1..=9 {
            keybinds.insert(
                format!("Super+Ctrl+{}", i),
                KeyAction::SwitchToWorkspace { workspace: i },
            );
        }

        // Move window to workspace (Super+Ctrl+Shift+1-9)
        for i in 1..=9 {
            keybinds.insert(
                format!("Super+Ctrl+Shift+{}", i),
                KeyAction::MoveToWorkspace { workspace: i },
            );
        }

        // Next/prev workspace with Page_Up/Page_Down
        keybinds.insert("Super+Ctrl+Page_Up".to_string(), KeyAction::PrevWorkspace);
        keybinds.insert("Super+Ctrl+Page_Down".to_string(), KeyAction::NextWorkspace);

        // Create new workspace
        keybinds.insert("Super+Ctrl+n".to_string(), KeyAction::NewWorkspace);

        Self {
            general: GeneralConfig::default(),
            keyboard: KeyboardConfig::default(),
            tiling: TilingConfig::default(),
            outputs: Vec::new(), // Empty = auto-detect and auto-arrange
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

        output.push_str("# Keyboard/input configuration\n");
        output.push_str("# All fields are optional - empty values use system defaults\n");
        output.push_str("[keyboard]\n");
        output.push_str("# XKB layout(s), comma-separated for multiple (e.g., \"us,de,fr\")\n");
        if let Some(ref layout) = self.keyboard.layout {
            output.push_str(&format!("layout = \"{}\"\n", layout));
        } else {
            output.push_str("# layout = \"us\"\n");
        }
        output.push_str("# XKB variant(s), comma-separated to match layouts\n");
        if let Some(ref variant) = self.keyboard.variant {
            output.push_str(&format!("variant = \"{}\"\n", variant));
        } else {
            output.push_str("# variant = \"dvorak\"\n");
        }
        output.push_str("# XKB options for layout switching, etc.\n");
        output.push_str("# Common options:\n");
        output.push_str("#   grp:alt_shift_toggle  - Alt+Shift to switch layouts\n");
        output.push_str("#   grp:ctrl_shift_toggle - Ctrl+Shift to switch layouts\n");
        output.push_str("#   grp:win_space_toggle  - Super+Space to switch layouts\n");
        output.push_str("#   ctrl:nocaps           - Caps Lock as Ctrl\n");
        if let Some(ref options) = self.keyboard.options {
            output.push_str(&format!("options = \"{}\"\n", options));
        } else {
            output.push_str("# options = \"grp:alt_shift_toggle\"\n");
        }
        output.push_str(&format!("# Key repeat delay in milliseconds\n"));
        output.push_str(&format!("repeat_delay = {}\n", self.keyboard.repeat_delay));
        output.push_str(&format!("# Key repeat rate in milliseconds\n"));
        output.push_str(&format!("repeat_rate = {}\n\n", self.keyboard.repeat_rate));

        output.push_str("# Tiling configuration\n");
        output.push_str("[tiling]\n");
        output.push_str("# Default window width as percentage of screen (0.0 - 1.0)\n");
        output.push_str(&format!(
            "default_window_width = {}\n",
            self.tiling.default_window_width
        ));
        output.push_str("# Enable scrolling tiling (niri-style horizontal scrolling)\n");
        output.push_str(&format!("scrolling = {}\n\n", self.tiling.scrolling));

        output.push_str("# Output/monitor configuration (optional)\n");
        output.push_str(
            "# If not specified, outputs are auto-detected and auto-arranged left-to-right\n",
        );
        output.push_str("# Example:\n");
        output.push_str("# [[outputs]]\n");
        output.push_str("# name = \"HDMI-A-1\"\n");
        output.push_str("# x = 0\n");
        output.push_str("# y = 0\n");
        output.push_str("# scale = 1.0\n");
        output.push_str("# mode = \"1920x1080@60\"\n");
        output.push_str("#\n");
        output.push_str("# [[outputs]]\n");
        output.push_str("# name = \"eDP-1\"\n");
        output.push_str("# x = 1920\n");
        output.push_str("# y = 0\n");
        output.push_str("# scale = 1.5\n\n");

        for out_cfg in &self.outputs {
            output.push_str("[[outputs]]\n");
            output.push_str(&format!("name = \"{}\"\n", out_cfg.name));
            if let Some(x) = out_cfg.x {
                output.push_str(&format!("x = {}\n", x));
            }
            if let Some(y) = out_cfg.y {
                output.push_str(&format!("y = {}\n", y));
            }
            if let Some(scale) = out_cfg.scale {
                output.push_str(&format!("scale = {}\n", scale));
            }
            if let Some(ref transform) = out_cfg.transform {
                output.push_str(&format!("transform = \"{}\"\n", transform));
            }
            if let Some(ref mode) = out_cfg.mode {
                output.push_str(&format!("mode = \"{}\"\n", mode));
            }
            output.push_str(&format!("enabled = {}\n\n", out_cfg.enabled));
        }

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
        output.push_str(
            "#   - switch_to_workspace: { action = \"switch_to_workspace\", workspace = 1 }\n",
        );
        output.push_str("#   - next_workspace: { action = \"next_workspace\" }\n");
        output.push_str("#   - prev_workspace: { action = \"prev_workspace\" }\n");
        output.push_str(
            "#   - move_to_workspace: { action = \"move_to_workspace\", workspace = 1 }\n",
        );
        output.push_str("#   - new_workspace: { action = \"new_workspace\" }\n");
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
                KeyAction::SwitchToWorkspace { workspace } => {
                    format!(
                        "{{ action = \"switch_to_workspace\", workspace = {} }}",
                        workspace
                    )
                }
                KeyAction::NextWorkspace => "{ action = \"next_workspace\" }".to_string(),
                KeyAction::PrevWorkspace => "{ action = \"prev_workspace\" }".to_string(),
                KeyAction::MoveToWorkspace { workspace } => {
                    format!(
                        "{{ action = \"move_to_workspace\", workspace = {} }}",
                        workspace
                    )
                }
                KeyAction::NewWorkspace => "{ action = \"new_workspace\" }".to_string(),
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
