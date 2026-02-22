use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const LAUNCH_AT_LOGIN_ID: &str = "launch_at_login";

fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/mac-state-monitor")
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub poll_interval_secs: u64,
    pub menubar_temp_component: String,
    #[serde(default)]
    pub launch_at_login: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: 1,
            menubar_temp_component: "CPU".to_string(),
            launch_at_login: false,
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = config_path();
        if let Ok(data) = fs::read_to_string(&path) {
            serde_json::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let dir = config_dir();
        let _ = fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(config_path(), json);
        }
    }
}
