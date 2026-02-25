use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const LAUNCH_AT_LOGIN_ID: &str = "launch_at_login";

pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/mac-state-monitor")
}

fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

fn default_runner_id() -> String {
    "runcat:cat".to_string()
}

fn default_runner_frame_ms() -> u64 {
    100
}

fn default_runner_display_secs() -> u64 {
    600
}

fn default_runner_icon_mode() -> RunnerIconMode {
    RunnerIconMode::White
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RunnerIconMode {
    Original,
    White,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct CustomRunnerSet {
    pub id: String,
    pub name: String,
    pub frame_paths: Vec<String>,
}

impl CustomRunnerSet {
    pub fn generate_id() -> String {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("set-{}", ts)
    }

    pub fn new(id: String, name: String, frame_paths: Vec<String>) -> Self {
        Self {
            id,
            name,
            frame_paths,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub poll_interval_secs: u64,
    pub menubar_temp_component: String,
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default = "default_runner_id")]
    pub runner_id: String,
    #[serde(default = "default_runner_frame_ms")]
    pub runner_frame_ms: u64,
    #[serde(default = "default_runner_display_secs")]
    pub runner_display_secs: u64,
    #[serde(default)]
    pub runner_rotation_ids: Vec<String>,
    #[serde(default)]
    pub custom_runner_sets: Vec<CustomRunnerSet>,
    #[serde(default = "default_runner_icon_mode")]
    pub runner_icon_mode: RunnerIconMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: 1,
            menubar_temp_component: "CPU".to_string(),
            launch_at_login: false,
            runner_id: default_runner_id(),
            runner_frame_ms: default_runner_frame_ms(),
            runner_display_secs: default_runner_display_secs(),
            runner_rotation_ids: vec![default_runner_id()],
            custom_runner_sets: Vec::new(),
            runner_icon_mode: default_runner_icon_mode(),
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
