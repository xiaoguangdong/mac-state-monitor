pub struct Config {
    pub poll_interval_secs: u64,
    pub menubar_temp_component: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: 1,
            menubar_temp_component: "CPU".to_string(),
        }
    }
}
