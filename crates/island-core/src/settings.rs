// What the user chose: which modules run, in which order their tabs sit, and
// each module's own switches. Read leniently: a missing or wrong field takes
// its default, unknown modules are dropped and new ones added, so an old or
// hand-edited file never stops the island from starting.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Every module the island knows, in its default tab order.
pub const MODULES: &[ModuleInfo] = &[
    ModuleInfo { id: "music", name: "Music", has_tab: true, default_on: true },
    ModuleInfo { id: "notifications", name: "Notifications", has_tab: true, default_on: true },
    ModuleInfo { id: "system", name: "System", has_tab: false, default_on: true },
    ModuleInfo { id: "focus", name: "Focus", has_tab: true, default_on: true },
    ModuleInfo { id: "water", name: "Water", has_tab: true, default_on: true },
    ModuleInfo { id: "todo", name: "To-do", has_tab: true, default_on: true },
    ModuleInfo { id: "shelf", name: "Shelf", has_tab: true, default_on: true },
    ModuleInfo { id: "claude", name: "Claude Code", has_tab: false, default_on: true },
    ModuleInfo { id: "control", name: "Control", has_tab: true, default_on: true },
    ModuleInfo { id: "accounts", name: "Accounts", has_tab: true, default_on: true },
    ModuleInfo { id: "prayer", name: "Prayer", has_tab: true, default_on: true },
    ModuleInfo { id: "day", name: "Day Progress", has_tab: true, default_on: true },
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModuleInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// Has a page of its own (a tab in the expanded island).
    pub has_tab: bool,
    pub default_on: bool,
}

pub fn module_info(id: &str) -> Option<&'static ModuleInfo> {
    MODULES.iter().find(|m| m.id == id)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModuleEntry {
    pub id: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicSettings {
    /// Synced lyrics from LRCLIB (sends title, artist, album, length).
    pub lyrics: bool,
    /// When LRCLIB has none, YouTube Music (unofficial endpoints).
    pub lyrics_youtube: bool,
    /// Japanese lines in romaji.
    pub romaji: bool,
}

impl Default for MusicSettings {
    fn default() -> Self {
        MusicSettings { lyrics: false, lyrics_youtube: false, romaji: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NotificationSettings {
    /// Pop the island up for a new notification.
    pub popups: bool,
    /// Apps mirrored, by name or AUMID part; "*" = every app.
    pub apps: Vec<String>,
    /// Apps switched off one by one in Settings (their names), as in
    /// Windows' own list: every other app stays on.
    pub off: Vec<String>,
}

impl NotificationSettings {
    /// The list the watcher filters with: the apps, then "!Name" for each one off.
    pub fn filter(&self) -> Vec<String> {
        self.apps.iter().cloned().chain(self.off.iter().map(|a| format!("!{a}"))).collect()
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        // Every app, as v1 had it; the switches turn single apps off.
        NotificationSettings { popups: true, apps: vec!["*".into()], off: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemSettings {
    pub volume: bool,
    pub caps_lock: bool,
    pub keyboard_layout: bool,
    pub charger: bool,
    pub network: bool,
    pub audio_device: bool,
    /// A dot on the capsule while an app uses the microphone (orange) or the camera (green).
    pub mic_cam: bool,
}

impl Default for SystemSettings {
    fn default() -> Self {
        SystemSettings { volume: true, caps_lock: true, keyboard_layout: true, charger: true, network: true, audio_device: true, mic_cam: true }
    }
}

/// The island's look: a theme (Graphite, neutral, or a hololive member's
/// colours and pixel mascot) and, if the user chose one, a picture behind it.
pub const THEMES: &[&str] = &["graphite", "fubuki", "mio", "towa", "azki", "suisei"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookSettings {
    /// The pixel mascot on the island.
    pub mascot: bool,
    /// One of THEMES.
    pub theme: String,
    /// A copy of the user's picture in the app's data folder; "" = none.
    pub background: String,
    /// How much the island's colour covers the picture, 0..1 (text stays readable).
    pub background_dim: f32,
    /// The island's body (Settings → Appearance → Look): one of LOOK_STYLES.
    pub style: String,
    /// The body's colour for the "color" look, "#rrggbb".
    pub color: String,
    /// Liquid Glass' tint: "auto" (by what is behind), "dark" or "light".
    pub glass_tint: String,
    /// After sunset the island's colours ease warmer, back at sunrise.
    pub night_warm: bool,
    /// How warm at night, 0..100 %.
    pub night_warmth: u32,
    /// How solid the closed (compact) island is, 20..100 %: solid again while
    /// hovered, open or showing an alert.
    pub closed_opacity: u32,
    /// The idle capsule and the open island, in logical pixels (clamped by `sizes`).
    pub idle_w: u32,
    pub idle_h: u32,
    pub expanded_w: u32,
    /// The open island with a normal page, and with a tall one (Lyrics).
    pub expanded_h: u32,
    pub expanded_tall_h: u32,
}

impl Default for LookSettings {
    fn default() -> Self {
        LookSettings { glass_tint: "auto".into(),
            mascot: false,
            theme: "graphite".into(),
            background: String::new(),
            background_dim: 0.55,
            style: "black".into(),
            color: "#1f2a52".into(),
            night_warm: false,
            night_warmth: 50,
            closed_opacity: 100,
            idle_w: 180,
            idle_h: 45,
            expanded_w: 680,
            expanded_h: 196,
            expanded_tall_h: 268,
        }
    }
}

/// The island's body: solid black, a colour, the wallpaper's colour or
/// Liquid Glass (what is behind it, blurred, with a lens edge). The old
/// "glass" and "frosted" looks are read as Liquid Glass.
pub const LOOK_STYLES: &[&str] = &["black", "color", "wallpaper", "liquid"];

/// The island's sizes, each within what the window and the pages allow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LookSizes {
    pub idle_w: f32,
    pub idle_h: f32,
    pub expanded_w: f32,
    pub expanded_h: f32,
    pub expanded_tall_h: f32,
}

/// The sliders' ranges (Settings shows the same ones).
pub const IDLE_W: (u32, u32) = (140, 260);
pub const IDLE_H: (u32, u32) = (36, 56);
pub const EXPANDED_W: (u32, u32) = (600, 760);
pub const EXPANDED_H: (u32, u32) = (170, 260);
pub const EXPANDED_TALL_H: (u32, u32) = (220, 360);

impl LookSettings {
    /// The theme to draw: unknown names fall back to the first.
    pub fn theme(&self) -> &'static str {
        THEMES.iter().copied().find(|t| *t == self.theme).unwrap_or(THEMES[0])
    }

    /// The look to draw: unknown names fall back to black.
    pub fn style(&self) -> &'static str {
        if self.style == "glass" || self.style == "frosted" {
            return "liquid";
        }
        LOOK_STYLES.iter().copied().find(|t| *t == self.style).unwrap_or(LOOK_STYLES[0])
    }

    /// The colour look's colour; a broken value takes the default navy.
    pub fn color_rgb(&self) -> crate::look::Rgb {
        crate::look::parse_hex(&self.color).unwrap_or((0x1f, 0x2a, 0x52))
    }

    pub fn sizes(&self) -> LookSizes {
        let c = |v: u32, r: (u32, u32)| v.clamp(r.0, r.1) as f32;
        let expanded_h = c(self.expanded_h, EXPANDED_H);
        LookSizes {
            idle_w: c(self.idle_w, IDLE_W),
            idle_h: c(self.idle_h, IDLE_H),
            expanded_w: c(self.expanded_w, EXPANDED_W),
            expanded_h,
            // Never shorter than a normal page.
            expanded_tall_h: c(self.expanded_tall_h, EXPANDED_TALL_H).max(expanded_h),
        }
    }

    /// Closed-island opacity, 0.2..1.
    pub fn closed_alpha(&self) -> f32 {
        self.closed_opacity.clamp(20, 100) as f32 / 100.0
    }
}

/// Water, after v1: a glass at a time toward a daily goal, reminders after a
/// while without one, only inside the active hours.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WaterSettings {
    pub reminders: bool,
    /// 100..10000, step 50.
    pub goal_ml: u32,
    /// 10..2000, step 10.
    pub glass_ml: u32,
    /// Minutes without a glass before a reminder, 10..480, step 5.
    pub every_min: u32,
    /// Active hours, "HH:MM"; the end after the start (no crossing midnight).
    pub from: String,
    pub until: String,
}

impl Default for WaterSettings {
    fn default() -> Self {
        WaterSettings { reminders: true, goal_ml: 2000, glass_ml: 250, every_min: 60, from: "09:00".into(), until: "21:00".into() }
    }
}

/// "HH:MM" → minutes after midnight.
pub fn parse_hh_mm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

impl WaterSettings {
    /// The plan the reminders follow, every number clamped to its range;
    /// hours that don't make sense take the defaults (v1's rule).
    pub fn plan(&self) -> crate::daily::WaterPlan {
        let (from, until) = match (parse_hh_mm(&self.from), parse_hh_mm(&self.until)) {
            (Some(f), Some(u)) if u > f => (f, u),
            _ => (9 * 60, 21 * 60),
        };
        crate::daily::WaterPlan {
            goal_ml: self.goal_ml.clamp(100, 10_000),
            glass_ml: self.glass_ml.clamp(10, 2000),
            every_min: if self.reminders { self.every_min.clamp(10, 480) } else { 0 },
            from_min: from,
            until_min: until,
        }
    }
}

/// Focus (Pomodoro), after v1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusSettings {
    /// 1..180
    pub focus_min: u32,
    /// 1..60
    pub short_min: u32,
    /// 1..120
    pub long_min: u32,
    /// A long break every this many focus rounds, 1..12.
    pub rounds: u32,
}

impl Default for FocusSettings {
    fn default() -> Self {
        FocusSettings { focus_min: 25, short_min: 5, long_min: 15, rounds: 4 }
    }
}

impl FocusSettings {
    pub fn plan(&self) -> crate::daily::FocusPlan {
        crate::daily::FocusPlan {
            focus_min: self.focus_min.clamp(1, 180),
            short_min: self.short_min.clamp(1, 60),
            long_min: self.long_min.clamp(1, 120),
            rounds: self.rounds.clamp(1, 12),
        }
    }
}

/// Prayer times (Kemenag's method, worked out on the PC): where, and when to pop up.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrayerSettings {
    /// One of the app's preset places by name; an unknown one means the first.
    pub place: String,
    /// A reminder this many minutes before each prayer, 0..30; 0 = none.
    pub remind_before_min: u32,
    /// Pop up at each prayer's time.
    pub at_time: bool,
}

impl Default for PrayerSettings {
    fn default() -> Self {
        PrayerSettings { place: "Kab. Bekasi".into(), remind_before_min: 10, at_time: true }
    }
}

/// Day Progress: the waking day the bar runs over ("HH:MM").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaySettings {
    pub wake: String,
    pub bedtime: String,
}

impl Default for DaySettings {
    fn default() -> Self {
        DaySettings { wake: "07:00".into(), bedtime: "23:00".into() }
    }
}

impl DaySettings {
    /// (wake, bedtime) in minutes after midnight; bedtime after wake by at
    /// least 30 min (no crossing midnight), else the defaults.
    pub fn span(&self) -> (u32, u32) {
        match (parse_hh_mm(&self.wake), parse_hh_mm(&self.bedtime)) {
            (Some(w), Some(b)) if b >= w + 30 => (w, b),
            _ => (7 * 60, 23 * 60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralSettings {
    /// The compact island goes up this many seconds after the pointer left, 2..60.
    pub hide_after_s: u32,
    /// Pop the island up for activities (not asks: those always show).
    pub popups: bool,
    /// The screen the island sits on, by Windows' name ("\\.\DISPLAY1"); "" = the main one.
    pub screen: String,
    /// Moving to another screen on its own (Mini Island's "Move automatically"):
    /// "off", "fullscreen" (away from a fullscreen video or game), "unused" (always
    /// to a screen not in use), "follow" (to the screen in use). `screen` stays home.
    pub auto_screen: String,
    /// What the idle island shows left and right of the time:
    /// "" | "date" | "network" | "battery".
    pub idle_left: String,
    pub idle_right: String,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        GeneralSettings { hide_after_s: 3, popups: true, screen: String::new(), auto_screen: "off".into(), idle_left: "date".into(), idle_right: "network".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    /// Tab order is this order; every known module appears exactly once.
    pub modules: Vec<ModuleEntry>,
    pub music: MusicSettings,
    pub notifications: NotificationSettings,
    pub system: SystemSettings,
    pub look: LookSettings,
    pub water: WaterSettings,
    pub focus: FocusSettings,
    pub prayer: PrayerSettings,
    pub day: DaySettings,
    pub general: GeneralSettings,
    pub autostart: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            modules: MODULES.iter().map(|m| ModuleEntry { id: m.id.into(), enabled: m.default_on }).collect(),
            music: MusicSettings::default(),
            notifications: NotificationSettings::default(),
            system: SystemSettings::default(),
            look: LookSettings::default(),
            water: WaterSettings::default(),
            focus: FocusSettings::default(),
            prayer: PrayerSettings::default(),
            day: DaySettings::default(),
            general: GeneralSettings::default(),
            autostart: false,
        }
    }
}

/// One field of `v`, or the default when it is missing or of the wrong type.
fn field<T: for<'de> Deserialize<'de> + Default>(v: &Value, key: &str) -> T {
    v.get(key).and_then(|x| serde_json::from_value(x.clone()).ok()).unwrap_or_default()
}

/// Each switch on its own: one wrong field doesn't reset its siblings.
fn section<T: for<'de> Deserialize<'de> + Serialize + Default>(v: &Value, key: &str) -> T {
    let mut base = serde_json::to_value(T::default()).unwrap_or(Value::Null);
    if let (Some(given), Some(base_map)) = (v.get(key).and_then(Value::as_object), base.as_object_mut()) {
        for (k, default) in base_map.iter_mut() {
            if let Some(x) = given.get(k) {
                // Keep it only if it has the default's shape.
                let same_kind = std::mem::discriminant(x) == std::mem::discriminant(default)
                    || (default.is_array() && x.is_array());
                if same_kind {
                    *default = x.clone();
                }
            }
        }
    }
    serde_json::from_value(base).unwrap_or_default()
}

impl Settings {
    /// Settings from a file's bytes; never fails (defaults for what is wrong).
    pub fn parse(bytes: &[u8]) -> Settings {
        let Ok(v) = serde_json::from_slice::<Value>(bytes) else { return Settings::default() };
        let mut modules: Vec<ModuleEntry> = Vec::new();
        if let Some(list) = v.get("modules").and_then(Value::as_array) {
            for entry in list {
                let Some(id) = entry.get("id").and_then(Value::as_str) else { continue };
                let Some(info) = module_info(id) else { continue };
                if modules.iter().any(|m| m.id == id) {
                    continue;
                }
                let enabled = entry.get("enabled").and_then(Value::as_bool).unwrap_or(info.default_on);
                modules.push(ModuleEntry { id: id.into(), enabled });
            }
        }
        // Modules added since the file was written go at the end, with their default.
        for info in MODULES {
            if !modules.iter().any(|m| m.id == info.id) {
                modules.push(ModuleEntry { id: info.id.into(), enabled: info.default_on });
            }
        }
        Settings {
            modules,
            music: section(&v, "music"),
            notifications: section(&v, "notifications"),
            system: section(&v, "system"),
            look: section(&v, "look"),
            water: section(&v, "water"),
            focus: section(&v, "focus"),
            prayer: section(&v, "prayer"),
            day: section(&v, "day"),
            general: section(&v, "general"),
            autostart: field(&v, "autostart"),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    pub fn is_on(&self, id: &str) -> bool {
        self.modules.iter().any(|m| m.id == id && m.enabled)
    }

    /// Moves a module one place earlier (-1) or later (+1) in the tab order.
    pub fn move_module(&mut self, id: &str, step: i32) {
        let Some(i) = self.modules.iter().position(|m| m.id == id) else { return };
        let j = i as i64 + step as i64;
        if j < 0 || j >= self.modules.len() as i64 {
            return;
        }
        self.modules.swap(i, j as usize);
    }

    /// The tabs of the expanded island after Home: enabled modules with a page.
    pub fn tabs(&self) -> Vec<&'static ModuleInfo> {
        self.modules.iter().filter(|m| m.enabled).filter_map(|m| module_info(&m.id)).filter(|i| i.has_tab).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_or_garbage_gives_the_defaults() {
        assert_eq!(Settings::parse(b""), Settings::default());
        assert_eq!(Settings::parse(b"not json"), Settings::default());
        assert_eq!(Settings::parse(b"[]"), Settings::default());
    }

    #[test]
    fn a_saved_file_comes_back_the_same() {
        let mut s = Settings::default();
        s.modules[0].enabled = false;
        s.move_module("notifications", -1);
        s.music.lyrics = true;
        s.notifications.apps = vec!["*".into()];
        s.system.caps_lock = false;
        s.autostart = true;
        s.look.theme = "towa".into();
        s.look.background_dim = 0.3;
        s.water.from = "08:30".into();
        s.focus.rounds = 3;
        s.general.hide_after_s = 10;
        s.prayer = PrayerSettings { place: "Kota Bandung".into(), remind_before_min: 0, at_time: false };
        s.day = DaySettings { wake: "05:30".into(), bedtime: "22:00".into() };
        assert_eq!(Settings::parse(s.to_json().as_bytes()), s);
    }

    #[test]
    fn water_and_focus_plans_stay_in_range() {
        let w = WaterSettings { goal_ml: 5, glass_ml: 99_999, every_min: 1, from: "08:30".into(), until: "22:00".into(), ..Default::default() };
        let p = w.plan();
        assert_eq!((p.goal_ml, p.glass_ml, p.every_min, p.from_min, p.until_min), (100, 2000, 10, 510, 1320));
        let backwards = WaterSettings { from: "22:00".into(), until: "08:00".into(), ..Default::default() };
        assert_eq!((backwards.plan().from_min, backwards.plan().until_min), (540, 1260), "no crossing midnight: the defaults");
        let off = WaterSettings { reminders: false, ..Default::default() };
        assert_eq!(off.plan().every_min, 0, "reminders off");
        assert_eq!(parse_hh_mm("7:05"), Some(425));
        assert_eq!(parse_hh_mm("24:00"), None);
        let f = FocusSettings { focus_min: 0, short_min: 999, long_min: 15, rounds: 40 }.plan();
        assert_eq!((f.focus_min, f.short_min, f.rounds), (1, 60, 12));
        assert_eq!(LookSettings { theme: "pekora".into(), ..Default::default() }.theme(), "graphite");
        assert_eq!(DaySettings { wake: "23:00".into(), bedtime: "07:00".into() }.span(), (420, 1380), "backwards: the defaults");
        assert_eq!(DaySettings { wake: "06:15".into(), bedtime: "00:00".into() }.span(), (420, 1380));
        assert_eq!(DaySettings { wake: "06:15".into(), bedtime: "22:45".into() }.span(), (375, 1365));
    }

    #[test]
    fn an_old_look_keeps_todays_appearance_and_sizes_stay_in_range() {
        let s = Settings::parse(br#"{"look":{"theme":"towa","background_dim":0.4}}"#);
        assert_eq!(s.look.theme(), "towa");
        assert_eq!(s.look.style(), "black");
        assert_eq!(s.look.closed_alpha(), 1.0);
        assert!(!s.look.night_warm);
        let z = s.look.sizes();
        assert_eq!((z.idle_w, z.idle_h, z.expanded_w, z.expanded_h, z.expanded_tall_h), (180.0, 45.0, 680.0, 196.0, 268.0));
        let wild = LookSettings { style: "neon".into(), color: "nope".into(), idle_w: 9999, idle_h: 1, expanded_w: 100, expanded_h: 255, expanded_tall_h: 230, closed_opacity: 3, ..Default::default() };
        assert_eq!(wild.style(), "black");
        for old in ["glass", "frosted", "liquid"] {
            assert_eq!(LookSettings { style: old.into(), ..Default::default() }.style(), "liquid");
        }
        assert_eq!(wild.color_rgb(), (0x1f, 0x2a, 0x52));
        assert_eq!(wild.closed_alpha(), 0.2);
        let z = wild.sizes();
        assert_eq!((z.idle_w, z.idle_h, z.expanded_w, z.expanded_tall_h), (260.0, 36.0, 600.0, 255.0), "tall never under normal");
    }

    #[test]
    fn unknown_modules_go_new_ones_come_order_is_kept() {
        let s = Settings::parse(br#"{"modules":[{"id":"system","enabled":false},{"id":"weather","enabled":true},{"id":"music","enabled":true},{"id":"music","enabled":false}]}"#);
        let ids: Vec<&str> = s.modules.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["system", "music", "notifications", "focus", "water", "todo", "shelf", "claude", "accounts", "prayer", "day"], "weather dropped, duplicate ignored, the rest added");
        assert!(!s.is_on("system"));
        assert!(s.is_on("music"));
    }

    #[test]
    fn a_wrong_field_takes_its_default_and_keeps_its_siblings() {
        let s = Settings::parse(br#"{"music":{"lyrics":"yes","romaji":false},"system":{"volume":false},"autostart":"no"}"#);
        assert!(!s.music.lyrics, "wrong type = default");
        assert!(!s.music.romaji, "sibling kept");
        assert!(!s.system.volume);
        assert!(s.system.caps_lock, "missing = default");
        assert_eq!(s.prayer, PrayerSettings::default(), "an old file gains Prayer's defaults");
        assert!(s.is_on("prayer"), "and the module, on");
        assert_eq!(s.day.span(), (420, 1380), "Day Progress: 07:00 to 23:00");
        assert!(s.is_on("day"));
        assert!(!s.autostart);
    }

    #[test]
    fn tabs_follow_order_and_switches() {
        let mut s = Settings::default();
        assert_eq!(s.tabs().iter().map(|t| t.id).collect::<Vec<_>>(), ["music", "notifications", "focus", "water", "todo", "shelf", "accounts", "prayer", "day"], "system has no tab");
        s.move_module("notifications", -1);
        assert_eq!(s.tabs()[..2].iter().map(|t| t.id).collect::<Vec<_>>(), ["notifications", "music"]);
        s.modules.iter_mut().find(|m| m.id == "music").unwrap().enabled = false;
        assert_eq!(s.tabs()[0].id, "notifications");
        assert!(!s.tabs().iter().any(|t| t.id == "music"));
        s.move_module("notifications", -1);
        assert_eq!(s.modules[0].id, "notifications", "can't move past the start");
    }
}
