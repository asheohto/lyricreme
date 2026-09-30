use std::fs;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

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
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window_x: None,
            window_y: 20,
            window_width: 1100,
            window_height: 80,
            font_family: "Segoe UI".to_string(),
            font_size_line1: 24.0,
            font_size_line2: 16.0,
            click_through: true,
            time_offset_ms: 0,
            tuna_port: 1608,
        }
    }
}

impl AppConfig {
    pub fn load_or_default() -> Self {
        if let Some(path) = config_file_path() {
            if path.exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(cfg) = serde_json::from_str::<AppConfig>(&content) {
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
