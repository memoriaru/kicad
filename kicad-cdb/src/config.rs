use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Deserialize, Serialize)]
pub struct AppConfig {
    #[serde(default = "default_kicad_cli_path")]
    pub kicad_cli_path: String,
}

fn default_kicad_cli_path() -> String {
    if cfg!(target_os = "macos") {
        "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli".into()
    } else if cfg!(target_os = "windows") {
        r"C:\Program Files\KiCad\bin\kicad-cli.exe".into()
    } else {
        "/usr/bin/kicad-cli".into()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            kicad_cli_path: default_kicad_cli_path(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        // 1. Project-level: ./cdb.toml
        if Path::new("cdb.toml").exists() {
            if let Ok(cfg) = Self::load_from(Path::new("cdb.toml")) {
                return Ok(cfg);
            }
        }

        // 2. User-level: ~/.config/cdb/config.toml
        if let Some(dir) = dirs::config_dir() {
            let user_path = dir.join("cdb").join("config.toml");
            if user_path.exists() {
                if let Ok(cfg) = Self::load_from(&user_path) {
                    return Ok(cfg);
                }
            }
        }

        Ok(Self::default())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&content)?;
        Ok(cfg)
    }
}
