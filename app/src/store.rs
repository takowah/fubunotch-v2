// Where FubuNotch v2 keeps its things: %APPDATA%\FubuNotch v2 (settings.json
// and the webhook token). A save goes to a temporary file first, then replaces
// the old one, so a crash mid-write never leaves half a file.

use std::path::PathBuf;

use island_core::Settings;

pub fn data_dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("FubuNotch v2")
}

fn settings_path() -> PathBuf {
    data_dir().join("settings.json")
}

pub fn load() -> Settings {
    std::fs::read(settings_path()).map(|b| Settings::parse(&b)).unwrap_or_default()
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir)?;
    let tmp = dir.join(format!("settings.json.{}.tmp", std::process::id()));
    std::fs::write(&tmp, settings.to_json())?;
    std::fs::rename(&tmp, settings_path())
}

/// One line in %APPDATA%\FubuNotch v2\fubunotch-v2.log, with the local time.
pub fn log(line: &str) {
    use std::io::Write;
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("fubunotch-v2.log")) {
        let _ = writeln!(f, "{} {line}", crate::local_time::stamp());
    }
}

/// What the daily modules keep: today's water and the to-do list.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Data {
    pub water: island_core::daily::WaterDay,
    pub todo: island_core::daily::Todo,
    pub shelf: Vec<crate::shelf::Item>,
    /// Songs the user liked (1) or disliked (-1), by `song_key`. Kept here
    /// only: Windows' media controls can't pass a like to the player.
    pub ratings: std::collections::BTreeMap<String, i8>,
    /// Apps seen sending notifications (name → AUMID): the per-app switches.
    pub notif_apps: std::collections::BTreeMap<String, String>,
    /// Focus rounds, oldest first.
    pub focus_log: Vec<island_core::daily::FocusRound>,
}

fn data_path() -> PathBuf {
    data_dir().join("data.json")
}

pub fn load_data() -> Data {
    std::fs::read(data_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn save_data(data: &Data) {
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join(format!("data.json.{}.tmp", std::process::id()));
    if let Ok(json) = serde_json::to_string_pretty(data) {
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, data_path());
        }
    }
}
