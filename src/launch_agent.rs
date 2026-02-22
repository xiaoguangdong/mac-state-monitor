use std::fs;
use std::path::PathBuf;

const PLIST_LABEL: &str = "com.mac-state-monitor.app";

fn plist_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", PLIST_LABEL))
}

pub fn is_enabled() -> bool {
    plist_path().exists()
}

pub fn set_enabled(enabled: bool) {
    let path = plist_path();
    if enabled {
        let exe = std::env::current_exe()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>"#,
            PLIST_LABEL, exe
        );
        let _ = fs::create_dir_all(path.parent().unwrap());
        let _ = fs::write(&path, plist);
    } else {
        let _ = fs::remove_file(&path);
    }
}
