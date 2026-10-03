use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Screen anchor for the lyrics overlay window.
/// Horizontal: Left / Center / Right  ×  Vertical: Top / Middle / Bottom
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LyricsPosition {
    TopLeft,
    TopCenter,
    TopRight,
    MiddleLeft,
    MiddleCenter,
    MiddleRight,
    BottomLeft,
    BottomCenter,
    BottomRight,
}

impl Default for LyricsPosition {
    fn default() -> Self {
        LyricsPosition::TopCenter
    }
}

impl LyricsPosition {
    /// DirectWrite text alignment for this position.
    /// Left anchor → left-align, Center → center, Right → right-align.
    pub fn text_alignment(&self) -> TextAlign {
        match self {
            LyricsPosition::TopLeft | LyricsPosition::MiddleLeft | LyricsPosition::BottomLeft => TextAlign::Left,
            LyricsPosition::TopCenter | LyricsPosition::MiddleCenter | LyricsPosition::BottomCenter => TextAlign::Center,
            LyricsPosition::TopRight | LyricsPosition::MiddleRight | LyricsPosition::BottomRight => TextAlign::Right,
        }
    }

    /// Human-readable label shown in the tray menu.
    pub fn label(&self) -> &'static str {
        match self {
            LyricsPosition::TopLeft     => "Top Left",
            LyricsPosition::TopCenter   => "Top Center",
            LyricsPosition::TopRight    => "Top Right",
            LyricsPosition::MiddleLeft  => "Middle Left",
            LyricsPosition::MiddleCenter=> "Middle Center",
            LyricsPosition::MiddleRight => "Middle Right",
            LyricsPosition::BottomLeft  => "Bottom Left",
            LyricsPosition::BottomCenter=> "Bottom Center",
            LyricsPosition::BottomRight => "Bottom Right",
        }
    }

    /// Returns all variants in display order.
    pub const ALL: [LyricsPosition; 9] = [
        LyricsPosition::TopLeft,    LyricsPosition::TopCenter,    LyricsPosition::TopRight,
        LyricsPosition::MiddleLeft, LyricsPosition::MiddleCenter, LyricsPosition::MiddleRight,
        LyricsPosition::BottomLeft, LyricsPosition::BottomCenter, LyricsPosition::BottomRight,
    ];

    pub fn all() -> &'static [LyricsPosition] {
        &Self::ALL
    }
}

/// Text alignment hint passed to the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub window_x: Option<i32>,
    pub window_y: i32,
    pub window_width: i32,
    pub window_height: i32,
    pub font_family: String,
    pub font_size_line1: f32,
    pub font_size_line2: f32,
    pub click_through: bool,
    pub time_offset_ms: i64,
    pub tuna_port: u16,
    #[serde(default)]
    pub position: LyricsPosition,
    /// Overall overlay opacity (30–100). Applied via SetLayeredWindowAttributes LWA_ALPHA.
    #[serde(default = "default_window_alpha")]
    pub window_alpha: u8,
    /// Text colour as [r, g, b] 0–255.
    #[serde(default = "default_text_color")]
    pub text_color: [u8; 3],
    /// Outline / drop-shadow colour as [r, g, b] 0–255.
    #[serde(default = "default_outline_color")]
    pub outline_color: [u8; 3],
    /// Outline thickness in px (0 = no outline, max 6).
    #[serde(default = "default_outline_width")]
    pub outline_width: f32,
    /// Music-reactive visualizer art (`assets/vibe.png`) next to the lyrics.
    #[serde(default = "default_visualizer")]
    pub visualizer: bool,
    /// Display Japanese lyrics in Hiragana reading.
    #[serde(default)]
    pub show_hiragana: bool,
}

fn default_window_alpha() -> u8 { 100 }
fn default_text_color() -> [u8; 3] { [255, 255, 255] }
fn default_outline_color() -> [u8; 3] { [0, 0, 0] }
fn default_outline_width() -> f32 { 1.5 }
fn default_visualizer() -> bool { true }

/// Named colour presets offered in the tray "Text Color" submenu.
pub const COLOR_PRESETS: [(&str, [u8; 3]); 7] = [
    ("White",  [255, 255, 255]),
    ("Black",  [0, 0, 0]),
    ("Cream",  [255, 232, 190]),
    ("Sky",    [120, 200, 255]),
    ("Pink",   [255, 140, 190]),
    ("Mint",   [140, 240, 190]),
    ("Gold",   [255, 210, 90]),
];

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window_x: None,
            window_y: 0,
            window_width: 1100,
            window_height: 170,
            font_family: "Segoe UI".to_string(),
            font_size_line1: 24.0,
            font_size_line2: 16.0,
            click_through: true,
            time_offset_ms: 0,
            tuna_port: 1608,
            position: LyricsPosition::TopCenter,
            window_alpha: 100,
            text_color: [255, 255, 255],
            outline_color: [0, 0, 0],
            outline_width: 1.5,
            visualizer: true,
            show_hiragana: false,
        }
    }
}

impl AppConfig {
    pub fn load_or_default() -> Self {
        if let Some(path) = config_file_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&content) {
                        let mut changed = false;
                        if cfg.window_y > 4 {
                            cfg.window_y = 0;
                            changed = true;
                        }
                        if cfg.window_height < 140 || cfg.window_height > 220 {
                            cfg.window_height = 170;
                            changed = true;
                        }
                        if changed {
                            let _ = cfg.save();
                        }
                        return cfg;
                    }
                }
            }
        }
        let def = Self::default();
        let _ = def.save();
        def
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        if let Some(path) = config_file_path() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let data = serde_json::to_string_pretty(self)?;
            fs::write(path, data)?;
        }
        Ok(())
    }
}

fn config_file_path() -> Option<PathBuf> {
    if let Ok(appdata) = std::env::var("APPDATA") {
        Some(PathBuf::from(appdata).join("LyricReme").join("config.json"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Old configs predate the appearance fields; they must deserialize with sane
    /// defaults instead of resetting the whole file (AGENTS.md config-schema gotcha).
    #[test]
    fn test_config_back_compat_defaults() {
        let old = r#"{
            "window_x": 410, "window_y": 20, "window_width": 1100, "window_height": 90,
            "font_family": "Segoe UI", "font_size_line1": 24.0, "font_size_line2": 16.0,
            "click_through": true, "time_offset_ms": 800, "tuna_port": 1608,
            "position": "top_center"
        }"#;
        let cfg: AppConfig = serde_json::from_str(old).expect("legacy config must load");
        assert_eq!(cfg.window_alpha, 100);
        assert_eq!(cfg.text_color, [255, 255, 255]);
        assert_eq!(cfg.outline_color, [0, 0, 0]);
        assert_eq!(cfg.outline_width, 1.5);
        assert!(cfg.visualizer);
        assert!(!cfg.show_hiragana);
        assert_eq!(cfg.time_offset_ms, 800);
        assert_eq!(cfg.position, LyricsPosition::TopCenter);
    }

    /// Round-trip through JSON must preserve the new appearance fields.
    #[test]
    fn test_config_round_trip_appearance() {
        let mut cfg = AppConfig::default();
        cfg.window_alpha = 60;
        cfg.text_color = COLOR_PRESETS[4].1;
        cfg.outline_width = 3.5;
        cfg.font_size_line1 = 30.0;
        cfg.show_hiragana = true;

        let json = serde_json::to_string(&cfg).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.window_alpha, 60);
        assert_eq!(back.text_color, COLOR_PRESETS[4].1);
        assert_eq!(back.outline_width, 3.5);
        assert_eq!(back.font_size_line1, 30.0);
        assert!(back.show_hiragana);
    }
}
