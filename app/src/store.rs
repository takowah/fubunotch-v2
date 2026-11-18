// Where Kanmuri keeps its things: %APPDATA%\Kanmuri (settings.json
// and the webhook token). A save goes to a temporary file first, then replaces
// the old one, so a crash mid-write never leaves half a file.

use std::path::PathBuf;

use island_core::Settings;

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let dir = base.join("Kanmuri");
    // Renamed from FubuNotch v2 (18 Nov 2026): its folder is copied over once.
    let old = base.join("FubuNotch v2");
    if !dir.exists() && old.is_dir() && std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(entries) = std::fs::read_dir(&old) {
            for e in entries.flatten() {
                if e.path().is_file() {
                    let _ = std::fs::copy(e.path(), dir.join(e.file_name()));
                }
            }
        }
    }
    dir
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

/// One line in %APPDATA%\Kanmuri\kanmuri.log, with the local time.
pub fn log(line: &str) {
    use std::io::Write;
    let dir = data_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("kanmuri.log")) {
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
    /// The apps whose Windows banners Kanmuri turned off (AUMID → what
    /// they had): put back as they were.
    pub banners: std::collections::BTreeMap<String, crate::banners::Kept>,
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
