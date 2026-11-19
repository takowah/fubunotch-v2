// Kanmuri: the island. Activities come in through the local webhook
// (island-webhook) and the modules; the arbiter (island-core) picks the front
// one; the user's settings decide which modules run and which tabs the
// expanded island shows. One timer: the arbiter's next wake, or the clock's
// next minute. Nothing else runs while nothing changes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod prayer;
mod day;
mod watch;
mod banners;
mod mascot;
mod bg;
mod claude;
mod local_time;
mod look;
mod glass;
mod music;
mod accent;
mod notif;
mod flash;
mod sensors;
mod system;
mod shelf;
mod snapshot;
mod accounts;
mod setup;
mod chats;
mod weather;
mod control;
mod pixels;
mod stats;
mod usage;
mod store;
mod theme;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::Instant;

use island_core::daily::{self, Focus, FocusPlan, WaterPlan};
#[allow(unused_imports)]
use island_core::settings::parse_hh_mm;
use island_core::{module_info, Activity, Arbiter, Kind, Settings};
use island_webhook::Event;
use slint::winit_030::{winit, WinitWindowAccessor};
use std::path::{Path, PathBuf};
use slint::{ComponentHandle, LogicalSize, ModelRc, PhysicalPosition, SharedString, Timer, TimerMode, VecModel};

slint::include_modules!();

/// Glow margin around the island (Ice.pad in island.slint).
/// The gap between the screen's top edge and the island (Ice.top).
// Sizes after Mini Island (island.ps1): the capsule at its "Extra large"
// setting (x 1.5), the notification card and the open list at normal size.
const TOP: f32 = 8.0;
/// Room around the island for its glow.
const GLOW: f32 = 6.0;
/// The hidden island: Mini Island's handle.
const HIDDEN: (f32, f32) = (64.0, 4.0);
/// The hidden island's hover strip (wider than its bar, easy to point at).
const HIDDEN_HOVER: (f32, f32) = (200.0, 18.0);
/// Nothing going on: the idle pill (120 x 30 at x 1.5).
const COMPACT_IDLE: (f32, f32) = (180.0, 45.0);
/// Something else on one line (a system notice, a progress).
const COMPACT_BUSY: (f32, f32) = (300.0, 42.0);
/// The song while it plays: Mini Island's small pill (148 x 28 at x 1.5).
const COMPACT_SONG: (f32, f32) = (222.0, 42.0);
/// A notification: Mini Island's popup ($WIDE_W x $NOTIF_H), a full pill.
const COMPACT_NOTICE: (f32, f32) = (340.0, 56.0);
const NOTICE_RADIUS: f32 = 28.0;
/// A multiple-choice question from Claude Code: the question, its options under it.
const COMPACT_QUESTION: (f32, f32) = (420.0, 88.0);
const QUESTION_RADIUS: f32 = 24.0;
/// The music's notice (a new song, play, pause): the song's popup card.
const COMPACT_MEDIA: (f32, f32) = (340.0, 88.0);
const MEDIA_RADIUS: f32 = 24.0;
/// The music's notices start this long after launch (ms), past the first snapshot.
const MUSIC_EVENTS_AFTER: u64 = 4_000;
/// Line's capsule: as wide as its line (plus the cover and the bars, $MINI_PAD),
/// between the small pill and $MINI_MAX_W; a longer line is drawn smaller.
const COMPACT_LYRIC: (f32, f32) = (240.0, 56.0);
const LYRIC_PAD: f32 = 106.0;
const LYRIC_MAX_W: f32 = 570.0;
/// A lyric's width per Latin character at 17.25 px (kana and kanji count twice).
const LYRIC_CHAR_W: f32 = 8.6;
/// The row under Line's lyric: a notice with a message, and without.
const LINE_NOTICE_H: f32 = 54.0;
/// The capsule's Line with furigana: the readings' row over the line.
const CAPSULE_RUBY_H: f32 = 10.0;
const LINE_NOTICE_SHORT_H: f32 = 30.0;
/// About as many characters as fit on Line's widest capsule.
const LYRIC_ONE_LINE: usize = 56;
/// The open island, laid out like Poni's: a wide panel, one top bar (tabs,
/// time, quick buttons) and the page under it across the full width, no
/// footer. Its chrome (edge, margins, the bar) plus the page; by default
/// (Settings → Appearance → Size, LookSettings::sizes) 680 px wide, a normal
/// page makes a 196 px panel, a tall one (lyrics) 268 px, and a page with
/// more in it (notifications, accounts) grows to fit.
const EXPANDED_CHROME: f32 = 74.0;
/// Notifications: a header and up to six rows in two columns ($ITEM_H + 2 each).
const NOTIF_HEADER_H: f32 = 28.0;
const NOTIF_ROW_PITCH: f32 = 50.0;
const NOTIF_ROWS_MAX: usize = 6;
/// Claude Code's questions shown at the top of the Notifications page.
const ASK_ROWS_MAX: usize = 3;
/// Accounts: one column per app, its title and its chips stacked.
const ACCOUNT_PAD: f32 = 8.0;
/// On a Claude Desktop chip after the first click: the switch restarts Claude.
const CONFIRM_DESKTOP: &str = "Click again to restart Claude";
const ACCOUNT_TITLE_H: f32 = 20.0;
const ACCOUNT_CHIP_H: f32 = 34.0;
const ACCOUNT_SPACING: f32 = 8.0;
/// Under a chip: its usage rows (label and resets, then the bar and "32% used")
/// and a note line ("Updated 18 min ago"), $USAGE_GAP apart (AccountBlock).
const USAGE_ROW_H: f32 = 28.0;
const USAGE_NOTE_LINE_H: f32 = 13.0;
const USAGE_GAP: f32 = 5.0;
/// Prayer: the timeline's width (the right column) and the room at its
/// ends: half a label (wide enough for "Maghrib" at 11.5 px), so the end
/// labels sit right under their dots too.
const PRAYER_TRACK_W: f32 = 420.0;
const PRAYER_TRACK_INSET: f32 = 28.0;
/// A prayer pop-up still comes this many minutes late (the PC was asleep
/// at the time); later than that it is skipped.
const PRAYER_LATE_MIN: u32 = 5;
/// Mini Island's $MAX_ISLAND_H.
const EXPANDED_MAX_H: f32 = 440.0;
/// Finished tasks shown in the log (more are kept).
const TODO_LOG_SHOWN: usize = 100;
/// How long a Windows notification floats on the capsule, at least.
const NOTIF_HOLD_MS: u64 = 5_000;

/// The compact island goes up this long after the pointer left (or a notice
/// came): Settings → General.
/// Now, as the to-do keeps it.
fn stamp_now() -> island_core::daily::Stamp {
    island_core::daily::Stamp { date: local_time::today(), min: local_time::minute_of_day() }
}

/// The to-do log, newest first, the day named on the first row of each.
fn todo_log_rows(log: &[island_core::daily::LoggedTask], today: &str) -> Vec<DoneRow> {
    let mut last_day = None;
    log.iter()
        .rev()
        .take(TODO_LOG_SHOWN)
        .map(|l| {
            let first = last_day != Some(l.done.date.as_str());
            last_day = Some(l.done.date.as_str());
            DoneRow {
                day: if first { island_core::daily::day_label(&l.done.date, today).to_uppercase().into() } else { SharedString::new() },
                time: local_time::hh_mm(l.done.min).into(),
                text: l.text.as_str().into(),
            }
        })
        .collect()
}

fn hide_after(s: &Settings) -> u64 {
    s.general.hide_after_s.clamp(2, 60) as u64 * 1000
}
/// The island springs between sizes over this long (Ice.morph); the window
/// region follows once it is done.
const MORPH_MS: u64 = 450;
/// The window never changes size: the largest island and its glow.
/// The widest open island Settings allows (Appearance → Size).
const EXPANDED_W_MAX: f32 = island_core::settings::EXPANDED_W.1 as f32;
const WINDOW: (f32, f32) = (if EXPANDED_W_MAX > LYRIC_MAX_W { EXPANDED_W_MAX } else { LYRIC_MAX_W } + 2.0 * GLOW + 8.0, TOP + EXPANDED_MAX_H + GLOW + 4.0);

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Hidden,
    Compact,
    Expanded,
}
/// Rows of waiting activities on the Home page.
const OTHERS_MAX: usize = 3;

struct State {
    arbiter: Arbiter,
    started: Instant,
    mode: Mode,
    /// When the compact island goes up again (None: it stays).
    hide_at: Option<u64>,
    /// The front activity last shown, to pop the island up for a new one.
    last_front: Option<String>,
    /// Keeps the island expanded (KANMURI_EXPANDED): screenshots without the mouse.
    pinned: bool,
    /// Music's Hold: the island stays open when the pointer leaves (let go
    /// on leaving the Music tab, so it can't stay stuck open).
    held: bool,
    /// Music's Line: the island stays a capsule showing the line being sung.
    line_held: bool,
    /// Counts the music's notices (song, play, pause, stop), one id each.
    music_events: u64,
    /// Do not disturb: the island stays down for notifications (calls excepted).
    dnd: bool,
    /// "Watch in island": the docked video, and whether it is kept shown.
    watching: Option<Watching>,
    watch_held: bool,
    /// The pointer left the island for the video: the island stays open
    /// until it leaves the video too.
    watch_hover: bool,
    /// A PiP window the user dragged away from the island: theirs now, not
    /// docked again while it is open.
    watch_freed: Option<isize>,
    /// Whether a Focus round ran at the last refresh: DND follows it.
    last_focus_running: bool,
    /// Until when the front shows under Line's lyric (ms).
    line_notice_until: u64,
    /// What the compact island showed last (see show_changes).
    last_shown: String,
    /// The mascot: its mood, and the timers that blink it and end a mood.
    /// The mascot's life and its clock (running only while the island shows).
    life: mascot::Life,
    life_timer: Timer,
    /// The island's size for its mode, and the timer that narrows the window
    /// region to it once the spring has finished.
    size_to: (f32, f32),
    region_timer: Timer,
    active_tab: String,
    /// The account being switched to (shown as busy).
    account_pending: String,
    /// A line shown on one account's chip in place of its name: "Click again to
    /// restart Claude", or how the last switch went. (account id, text)
    account_hint: (String, String),
    /// The Accounts page was open at the last refresh (its usage is read when it opens).
    accounts_open: bool,
    /// The Control page: what it shows, a line after a failed change, the
    /// page open at the last refresh, and the worker that reads and changes.
    control: control::Snapshot,
    control_note: String,
    control_open: bool,
    control_tx: Option<std::sync::mpsc::Sender<ControlJob>>,
    /// The Control page: the last weather and when it came (ms since start), a
    /// reading on its way, the last one failed; keep awake on.
    weather: Option<(weather::Weather, u64)>,
    weather_busy: bool,
    weather_failed: bool,
    keep_awake: bool,
    /// Settings > Accounts > Send setup: the two accounts, the parts picked
    /// (None: not yet, all but the model), what the last send did.
    setup_from: String,
    setup_to: String,
    setup_parts: Option<Vec<String>>,
    setup_report: String,
    /// Settings > Accounts > MCP servers: the ones picked, what the last send did.
    mcp_picked: Vec<String>,
    mcp_report: String,
    /// Settings > Accounts > Chats: the two accounts, the chats picked, the
    /// folders open, the list read (for which account), what the last copy did.
    chats_from: String,
    chats_to: String,
    chats_picked: Vec<String>,
    chats_open: Vec<String>,
    chats_list: Option<(String, Vec<chats::Folder>)>,
    chats_loading: bool,
    chats_report: String,
    settings: Settings,
    /// What is playing (the Music module), and its artwork as an image.
    music: music::Snapshot,
    music_art: Option<Art>,
    /// The lyrics card's text width (logical px), as the card measured it:
    /// the furigana rows are wrapped to it.
    lyric_width: f32,
    /// The Music page's big cover, logical px, as the page measured it (0 = not yet).
    cover_big: f32,
    /// The cover's colour (accent.rs), the island's while the song plays.
    music_accent: Option<(u8, u8, u8)>,
    /// A browser's song: its cover from YouTube Music (kept over the browser's
    /// own icon, which is all a browser hands over), by the lyrics key.
    cover_for: String,
    /// The lyrics of `lyrics_key` (a song), or where the search is.
    lyrics_key: String,
    lyrics: Option<island_lyrics::Lyrics>,
    /// The lyric lines as the UI's list, made once a song (and kept, so the
    /// list slides instead of being rebuilt every tick), and what it was made from.
    lyrics_model: Option<(String, ModelRc<LyricLine>)>,
    /// YouTube Music's shuffle/repeat from the browser extension, and when it said so.
    ytm: Option<(island_webhook::YtmState, u64)>,
    /// The user's nudge of the lyrics for this song, seconds (later: positive).
    lyrics_nudge: f64,
    /// The Notifications module: its watcher while on, and the newest first.
    notif_watcher: Option<notif::Watcher>,
    notifications: VecDeque<notif::Notif>,
    /// Who uses the microphone and the camera now (sensors.rs).
    sensors: sensors::InUse,
    /// The next id for a notification of an app without Windows ones (flash.rs).
    flash_id: u32,
    /// The daily modules.
    focus: Focus,
    focus_plan: FocusPlan,
    water_plan: WaterPlan,
    /// When the last water reminder went (minute of the day).
    water_reminded: Option<u32>,
    /// When the focus round under way was first started (minutes after midnight).
    focus_started_min: Option<u32>,
    /// Prayer: the day's times, for which date, place and time zone.
    prayer_day: Option<(String, prayer::Day)>,
    /// The last minute whose prayer pop-ups were looked at, and its date
    /// ("2026-11-18"); pop-ups up to it are done.
    prayer_seen: (String, i64),
    data: store::Data,
    /// Claude Code permission requests waiting for a click: activity id → answer.
    pending: HashMap<String, island_webhook::Reply>,
    /// AskUserQuestion's input per question activity, to answer it with the option clicked.
    ask_inputs: HashMap<String, serde_json::Value>,
    ask_seq: u64,
    /// Apps' logos as images (by AUMID), made once on the UI thread from
    /// notif's pixels: refreshes reuse them.
    logos: HashMap<String, slint::Image>,
    confirm: Option<ConfirmWindow>,
    /// A file is being dragged over the island.
    drop_hover: bool,
    timer: Timer,
    settings_window: Option<SettingsWindow>,
    /// The island's background picture: for which file, size and look, and the picture.
    bg_key: String,
    bg_image: Option<slint::Image>,
    bg_preview: Option<slint::Image>,
    /// The wallpaper's colour, darkened (the Wallpaper color look).
    wallpaper: Option<island_core::look::Rgb>,
    /// The Look last put on the island.
    look_shown: Option<look::Colors>,
    /// Liquid Glass (glass.rs): its ~10 fps clock (only while shown), the
    /// backdrop is light (dark ink), the last frame's hash and the shape the
    /// shown picture was cut for, a frame being made, unchanged frames in a
    /// row, and whether the island is left out of screen capture.
    glass_timer: Timer,
    /// Re-reads the idle island's word sides (CPU / RAM / disk every
    /// STATS_MS, the next prayer every half minute), only while they show.
    stats_timer: Timer,
    stats_every: u64,
    glass_light: bool,
    glass_hash: u64,
    glass_key: String,
    glass_busy: bool,
    glass_still: u32,
    glass_excluded: Option<bool>,
}

static ISLAND_MADE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    /// The island, for the file-drop handler (which runs before Slint sees the event).
    static ISLAND: RefCell<Option<slint::Weak<Island>>> = const { RefCell::new(None) };
}

/// Files dropped on (or dragged over) the island, queued by shelf::DropWatch.
fn take_drops() {
    let drops: Vec<shelf::Drop> = std::mem::take(&mut *shelf::QUEUE.lock().unwrap_or_else(|e| e.into_inner()));
    let Some(ui) = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade())) else { return };
    with_state(|state| {
        if !state.settings.is_on("shelf") {
            return;
        }
        for d in drops {
            match d {
                shelf::Drop::Hover => {
                    state.drop_hover = true;
                    state.mode = Mode::Expanded;
                    state.hide_at = None;
                    state.active_tab = "shelf".into();
                }
                shelf::Drop::Cancel => state.drop_hover = false,
                shelf::Drop::File(path) => {
                    state.drop_hover = false;
                    state.active_tab = "shelf".into();
                    shelf::add(&mut state.data.shelf, &path);
                    store::save_data(&state.data);
                }
            }
        }
    });
    refresh(&ui);
}

/// Puts the island back where it belongs: top centre of its monitor, above
/// every window, its region matching what it shows. After a monitor came or
/// went (screen off after idle, a dock, a resolution change), a wake from
/// sleep, or Kanmuri started again while running.
fn recover_island(ui: &Island) {
    place(ui);
    if let Some(hwnd) = island_hwnd(ui) {
        popup_style(hwnd);
    }
    keep_on_top();
    if let Some((size, hidden)) = with_state(|state| (state.size_to, state.mode == Mode::Hidden)) {
        clip_window(ui, size, hidden);
    }
    show_changes(ui);
}

/// Windows said the screens or the power changed: recover once things settle
/// (Windows moves windows about for a moment after a display change).
fn recover_soon() {
    Timer::single_shot(std::time::Duration::from_millis(800), || {
        if let Some(ui) = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade())) {
            store::log("island: screens or power changed, placed again");
            recover_island(&ui);
            refresh(&ui);
        }
    });
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

fn now_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

/// The source's colour: a few icy tones, the same one for the same source;
/// an `ask` is always amber (someone waits for you).
fn accent(a: &Activity) -> slint::Color {
    accent_of(&a.source, a.kind)
}

fn accent_of(source: &str, kind: Kind) -> slint::Color {
    if kind == Kind::Ask {
        return slint::Color::from_rgb_u8(0xf5, 0xb5, 0x42);
    }
    // Mini Island takes an app's colour from its icon; without icons, a few
    // known apps get theirs, the rest one of Mini Island's spare colours.
    let known = match source.to_lowercase().as_str() {
        s if s.contains("claude") => Some((0xd9, 0x77, 0x57)),
        s if s.contains("discord") => Some((0x58, 0x65, 0xf2)),
        s if s.contains("whatsapp") => Some((0x25, 0xd3, 0x66)),
        s if s.contains("telegram") => Some((0x2a, 0xab, 0xee)),
        s if s.contains("wechat") => Some((0x07, 0xc1, 0x60)),
        s if s.contains("slack") => Some((0xe0, 0x1e, 0x5a)),
        "icon:prayer" => Some((0x30, 0xd1, 0x58)),
        _ => None,
    };
    if let Some((r, g, b)) = known {
        return slint::Color::from_rgb_u8(r, g, b);
    }
    const TONES: [(u8, u8, u8); 7] = [(0x0a, 0x84, 0xff), (0xbf, 0x5a, 0xf2), (0xff, 0x9f, 0x0a), (0xff, 0x37, 0x5f), (0x64, 0xd2, 0xff), (0x30, 0xd1, 0x58), (0x5e, 0x5c, 0xe6)];
    let h = source.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let (r, g, b) = TONES[(h as usize) % TONES.len()];
    slint::Color::from_rgb_u8(r, g, b)
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Alert => "alert",
        Kind::Ask => "ask",
        Kind::Progress => "progress",
        Kind::Status => "status",
        Kind::Ambient => "ambient",
    }
}

/// A multiple-choice question from Claude Code: an ask whose buttons are its
/// options (claude::question_plan), not Allow / Deny.
fn is_question(a: &Activity) -> bool {
    a.kind == Kind::Ask && a.actions.first().is_some_and(|x| x.id.starts_with("answer:"))
}

/// `logo` at LOGO_NOTICE, `logo_small` at LOGO_CAPSULE (each drawn 1:1).
fn item(a: &Activity, logo: Option<slint::Image>, logo_small: Option<slint::Image>) -> Item {
    let actions: Vec<ActionInfo> = a.actions.iter().map(|x| ActionInfo { id: x.id.as_str().into(), label: x.label.as_str().into() }).collect();
    Item {
        actions: ModelRc::new(VecModel::from(actions)),
        // System notices travel as "icon:<name>": the capsule draws that icon.
        source: if a.source.starts_with("icon:") { "system".into() } else { a.source.as_str().into() },
        icon: match a.source.strip_prefix("icon:") {
            Some(name) => name.into(),
            // The island's own modules: a drawn icon (the owner: never a letter in a circle).
            None if logo.is_none() => module_icon(&a.source).into(),
            None => "".into(),
        },
        title: a.title.as_str().into(),
        body: a.body.as_str().into(),
        kind: kind_name(a.kind).into(),
        progress: a.progress.unwrap_or(0.0),
        has_progress: a.progress.is_some(),
        accent: accent(a),
        initial: initial(&a.source).into(),
        has_logo: logo.is_some(),
        logo: logo.unwrap_or_default(),
        logo_small: logo_small.unwrap_or_default(),
        question: is_question(a),
    }
}

/// The app behind an activity, by AUMID: Claude Code's events wear the Claude
/// app's logo, a notification its own app's; "" when unknown.
fn activity_aumid(notifications: &VecDeque<notif::Notif>, a: &Activity) -> String {
    if a.id.starts_with("claude:") || a.source.to_lowercase().contains("claude") {
        return notif::CLAUDE_AUMID.into();
    }
    if let Some(n) = a.id.strip_prefix("notif:").and_then(|x| x.parse::<u32>().ok()).and_then(|x| notifications.iter().find(|n| n.id == x)) {
        return n.aumid.clone();
    }
    notifications.iter().find(|n| n.app == a.source).map(|n| n.aumid.clone()).or_else(|| notif::seen_apps().get(&a.source).cloned()).unwrap_or_default()
}

/// The stack a notification belongs to: its app (by AUMID, else its name).
fn notif_key(n: &notif::Notif) -> &str {
    if n.aumid.is_empty() {
        &n.app
    } else {
        &n.aumid
    }
}

/// The most rows one app gets on the Notifications page, so a chatty app
/// can't fill it (the owner, 18 Nov 2026).
const NOTIF_PER_APP: usize = 2;

/// The Notifications page's rows (indexes in the list, newest first): as
/// before, one a notification, but at most NOTIF_PER_APP of each app.
fn notif_view(list: &std::collections::VecDeque<notif::Notif>) -> Vec<usize> {
    let mut seen: Vec<(&str, usize)> = Vec::new();
    let mut rows = Vec::new();
    for (i, n) in list.iter().enumerate() {
        let k = notif_key(n);
        let c = match seen.iter_mut().find(|(x, _)| *x == k) {
            Some(e) => {
                e.1 += 1;
                e.1
            }
            None => {
                seen.push((k, 1));
                1
            }
        };
        if c <= NOTIF_PER_APP {
            rows.push(i);
        }
    }
    rows
}

/// An app's logo as an image with rounded corners, `px` physical pixels a
/// side (exactly as drawn: Slint's software renderer scales jaggedly), made
/// once per app and size on the UI thread; None (the app glyph is drawn)
/// while notif has no pixels for it.
fn logo_image(cache: &mut HashMap<String, slint::Image>, aumid: &str, px: u32) -> Option<slint::Image> {
    if aumid.is_empty() {
        return None;
    }
    let key = format!("{aumid}@{px}");
    if let Some(img) = cache.get(&key) {
        return Some(img.clone());
    }
    let src = notif::logo(aumid)?;
    let n = notif::LOGO_PX;
    if src.len() != (n * n * 4) as usize {
        return None;
    }
    // A square tile gets rounded corners; a logo with its own shape (its
    // corners already transparent) is left as drawn.
    let corner_alpha = |x: u32, y: u32| src[((y * n + x) * 4 + 3) as usize];
    let tile = [(0, 0), (n - 1, 0), (0, n - 1), (n - 1, n - 1)].iter().any(|&(x, y)| corner_alpha(x, y) > 128);
    let mut out = pixels::resize_square(&src, n, px);
    if tile {
        round_cover(&mut out, px);
    }
    let img = pixels::image(&out, px);
    cache.insert(key, img.clone());
    Some(img)
}

/// The logo sizes drawn, logical px: rows on the Notifications page (and the
/// open island's notice), the capsule's.
const LOGO_ROW: f32 = 30.0;
const LOGO_NOTICE: f32 = 34.0;
const LOGO_CAPSULE: f32 = 26.0;

/// Claude Code's open questions (asks still live), newest first, for the
/// top of the Notifications page.
fn ask_rows(arbiter: &Arbiter, logos: &mut HashMap<String, slint::Image>, scale: f32) -> Vec<AskRow> {
    let logo = logo_image(logos, notif::CLAUDE_AUMID, pixels::physical(LOGO_ROW, scale));
    let mut asks: Vec<&Activity> = arbiter.ranked().into_iter().filter(|a| a.kind == Kind::Ask).collect();
    asks.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    asks.iter()
        .map(|a| {
            // The body is "project\ncommand" (claude::plan).
            let (project, command) = a.body.split_once('\n').unwrap_or((a.body.as_str(), ""));
            AskRow {
                id: a.id.as_str().into(),
                title: a.title.as_str().into(),
                project: project.trim().into(),
                command: command.trim().into(),
                accent: accent(a),
                initial: initial(&a.source).into(),
                has_logo: logo.is_some(),
                logo: logo.clone().unwrap_or_default(),
                // Allow / Deny, or a question's options.
                actions: ModelRc::new(VecModel::from(a.actions.iter().map(|x| ActionInfo { id: x.id.as_str().into(), label: x.label.as_str().into() }).collect::<Vec<_>>())),
            }
        })
        .collect()
}

/// Answers a question (from the capsule or the Notifications page): the
/// user's "allow" / "deny" goes back to Claude Code, and the question ends.
fn answer(state: &mut State, id: &str, action: &str, now: u64) {
    let input = state.ask_inputs.remove(id);
    if let Some(reply) = state.pending.remove(id) {
        // Only what the user clicked goes back: "allow", "deny", or a
        // question's option as its answer ("input:" + the answered input).
        if action == "allow" || action == "deny" {
            let _ = reply.0.send(action.to_string());
        } else if let Some(i) = action.strip_prefix("answer:").and_then(|i| i.parse::<usize>().ok()) {
            if let Some(answered) = input.as_ref().and_then(|v| claude::answered_input(v, i)) {
                let _ = reply.0.send(format!("input:{answered}"));
            }
        }
    }
    state.arbiter.end(id, now);
}

/// The demo notifications' apps and their AUMIDs (KANMURI_DEMO_NOTIFS).
const DEMO_APPS: [(&str, &str); 3] = [
    ("Claude", notif::CLAUDE_AUMID),
    ("Discord", "com.squirrel.Discord.Discord"),
    ("WhatsApp", "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"),
];

/// A made-up logo for a demo app Windows doesn't have: a tile in its colour
/// with a pale diagonal band, LOGO_PX square RGBA.
fn demo_logo(c: slint::Color) -> Vec<u8> {
    let n = notif::LOGO_PX;
    let mut px = Vec::with_capacity((n * n * 4) as usize);
    for y in 0..n {
        for x in 0..n {
            let band = (x as i32 - y as i32).abs() < (n as i32 / 6);
            let mix = |v: u8| if band { ((v as u32 + 255 * 2) / 3) as u8 } else { v };
            px.extend([mix(c.red()), mix(c.green()), mix(c.blue()), 255]);
        }
    }
    px
}

/// An app's first letter, for its round icon.
/// The drawn icon (SysIcon) for an activity of the island's own modules and
/// anything else without a logo; "" leaves the app's logo or the app glyph.
fn module_icon(source: &str) -> &'static str {
    match source {
        s if s.starts_with("water") => "water",
        s if s.starts_with("focus") => "timer",
        s if s.starts_with("todo") => "check",
        s if s.starts_with("shelf") => "tray",
        s if s.starts_with("prayer") => "prayer",
        s if s.starts_with("music") => "note",
        s if s.starts_with("claude") => "",
        _ => "",
    }
}

fn initial(app: &str) -> String {
    app.trim().chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().to_string()).unwrap_or_default()
}

/// Rounds a square cover's corners (radius a quarter of its side) in place:
/// the software renderer clips square, so the picture carries its own corners.
fn round_cover(rgba: &mut [u8], n: u32) {
    let r = n as f32 / 4.0;
    for y in 0..n {
        for x in 0..n {
            // Distance into the corner's circle, from the nearest corner's centre.
            let cx = if (x as f32) < r { r } else if (x as f32) > n as f32 - r { n as f32 - r } else { continue };
            let cy = if (y as f32) < r { r } else if (y as f32) > n as f32 - r { n as f32 - r } else { continue };
            let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
            let cover = (r - d + 0.5).clamp(0.0, 1.0);
            let i = ((y * n + x) * 4) as usize;
            // Straight (not premultiplied) RGBA: only the alpha changes.
            if let Some(a) = rgba.get_mut(i + 3) {
                *a = (*a as f32 * cover) as u8;
            }
        }
    }
}

/// The screen picked in Settings > General (Windows' name); "" = the main one.
static SCREEN: Mutex<String> = Mutex::new(String::new());
/// Settings > General > Move automatically ("off" | "fullscreen" | "unused" | "follow").
static AUTO_MODE: Mutex<String> = Mutex::new(String::new());
/// The screen the island moved to on its own; "" = home (SCREEN).
static AUTO_AT: Mutex<String> = Mutex::new(String::new());

/// Where the island wants to be, the moment it first wanted it, and a fullscreen
/// window remembered until it is no longer fullscreen (so the island doesn't come
/// back over a video still playing while the owner works on the other screen).
struct AutoMove {
    want: Option<String>,
    since: std::time::Instant,
    fullscreen: isize,
    timer: Timer,
}

thread_local! {
    static AUTO: RefCell<AutoMove> = RefCell::new(AutoMove { want: None, since: std::time::Instant::now(), fullscreen: 0, timer: Timer::default() });
}

/// A window's screen (Windows' name) and that screen's rectangle.
fn screen_of(hwnd: windows::Win32::Foundation::HWND) -> Option<(String, windows::Win32::Foundation::RECT)> {
    use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONULL};
    if hwnd.0.is_null() {
        return None;
    }
    unsafe {
        let m = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL);
        if m.is_invalid() {
            return None;
        }
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(m, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
            return None;
        }
        let n = info.szDevice.iter().position(|&c| c == 0).unwrap_or(info.szDevice.len());
        Some((String::from_utf16_lossy(&info.szDevice[..n]), info.monitorInfo.rcMonitor))
    }
}

/// A window covering its whole screen (a video, a game), not the desktop or the taskbar.
fn is_fullscreen(hwnd: windows::Win32::Foundation::HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowRect, IsWindow, IsWindowVisible};
    unsafe {
        if hwnd.0.is_null() || !IsWindow(Some(hwnd)).as_bool() || !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class) as usize;
        let class = String::from_utf16_lossy(&class[..n]);
        if matches!(class.as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
            return false;
        }
        let mut r = windows::Win32::Foundation::RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() {
            return false;
        }
        screen_of(hwnd).is_some_and(|(_, m)| r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom)
    }
}

/// Mini Island's "Move automatically": decides which screen the island should be
/// on now and moves it once that has held for 2 s (a quick alt-tab doesn't make it
/// jump), never while the island is open. Run when another window comes forward,
/// and every 2 s only while something is pending or a fullscreen window is
/// remembered. Nothing runs while it is off.
fn auto_screen_check() {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let Some(ui) = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade())) else { return };
    let mode = AUTO_MODE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let home_pick = SCREEN.lock().unwrap_or_else(|e| e.into_inner()).clone();
    // The screens: (name, is main), the main one first.
    let mut screens: Vec<(String, bool)> = Vec::new();
    ui.window().with_winit_window(|w: &winit::window::Window| {
        let main = w.primary_monitor().and_then(|m| m.name());
        for m in w.available_monitors() {
            if let Some(n) = m.name() {
                let is_main = main.as_deref() == Some(n.as_str());
                screens.push((n, is_main));
            }
        }
    });
    screens.sort_by_key(|(n, main)| (!main, n.clone()));
    let off = !matches!(mode.as_str(), "fullscreen" | "unused" | "follow") || screens.len() < 2;
    let at = AUTO_AT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if off {
        AUTO.with(|a| {
            let mut a = a.borrow_mut();
            a.want = None;
            a.fullscreen = 0;
            a.timer.stop();
        });
        if !at.is_empty() {
            AUTO_AT.lock().unwrap_or_else(|e| e.into_inner()).clear();
            recover_island(&ui);
        }
        return;
    }
    let home = screens.iter().find(|(n, _)| !home_pick.is_empty() && *n == home_pick).or_else(|| screens.first()).map(|(n, _)| n.clone()).unwrap_or_default();
    let current = if !at.is_empty() && screens.iter().any(|(n, _)| *n == at) { at.clone() } else { home.clone() };
    let busy = with_state(|s| s.mode == Mode::Expanded).unwrap_or(false);
    let fg = unsafe { GetForegroundWindow() };
    // Kanmuri's own windows (the island, Settings) say nothing about the screen in use:
    // with Follow me, Settings on one screen dragged the island back after a pick.
    let ours = {
        let mut pid = 0u32;
        unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId(fg, Some(&mut pid)) };
        pid == std::process::id()
    };
    let fg_screen = if ours { None } else { screen_of(fg).map(|(n, _)| n) };
    let move_to = AUTO.with(|a| {
        let mut a = a.borrow_mut();
        if !ours && is_fullscreen(fg) {
            a.fullscreen = fg.0 as isize;
        }
        let mut blocked: Vec<String> = Vec::new();
        if a.fullscreen != 0 {
            let fs = windows::Win32::Foundation::HWND(a.fullscreen as *mut core::ffi::c_void);
            match is_fullscreen(fs).then(|| screen_of(fs)).flatten() {
                Some((n, _)) => blocked.push(n),
                None => a.fullscreen = 0,
            }
        }
        if mode == "unused" {
            blocked.extend(fg_screen.clone());
        }
        let want = match (mode.as_str(), &fg_screen) {
            ("follow", Some(f)) if !blocked.contains(f) => f.clone(),
            ("follow", None) => current.clone(),
            _ if !blocked.contains(&home) => home.clone(),
            // Every screen in use: stay home.
            _ => screens.iter().map(|(n, _)| n).find(|n| !blocked.contains(n)).cloned().unwrap_or_else(|| home.clone()),
        };
        let mut move_to = None;
        if want == current {
            a.want = None;
        } else if a.want.as_deref() != Some(want.as_str()) {
            store::log(&format!("island: wants {want} (on {current}, in use {})", fg_screen.as_deref().unwrap_or("-")));
            a.want = Some(want);
            a.since = std::time::Instant::now();
        } else if a.since.elapsed() >= std::time::Duration::from_millis(1900) && !busy {
            a.want = None;
            move_to = Some(want);
        } else if busy {
            store::log("island: move waits, the island is open");
        }
        // Checked again every second while Move automatically is on, as Mini
        // Island does (its Update-Position runs on a timer): Windows' foreground
        // event alone missed switches (Chrome on the other screen for 20 s,
        // 19 Nov 2026). A check is two Win32 calls; nothing runs while it is off.
        a.timer.start(slint::TimerMode::SingleShot, std::time::Duration::from_millis(1000), auto_screen_check);
        move_to
    });
    if let Some(to) = move_to {
        store::log(&format!("island: moved on its own to {to} ({mode})"));
        *AUTO_AT.lock().unwrap_or_else(|e| e.into_inner()) = if to == home { String::new() } else { to };
        recover_island(&ui);
    }
}

/// Puts the (fixed-size) window at the top centre of its monitor: the screen
/// picked in Settings when it is connected, else the one it is on or the main one.
fn place(ui: &Island) {
    ui.window().set_size(LogicalSize::new(WINDOW.0, WINDOW.1));
    let at = AUTO_AT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let want = if at.is_empty() { SCREEN.lock().unwrap_or_else(|e| e.into_inner()).clone() } else { at };
    ui.window().with_winit_window(|win: &winit::window::Window| {
        let picked = if want.is_empty() { None } else { win.available_monitors().find(|m| m.name().as_deref() == Some(want.as_str())) };
        // "" is the main screen (Mini Island's Get-HomeScreen): not the one it
        // happens to be on, which left a pick of "Screen 2 (main)" doing nothing.
        let fallback = win.primary_monitor().or_else(|| win.current_monitor());
        if let Some(monitor) = picked.or(fallback) {
            let scale = monitor.scale_factor();
            let mw = monitor.size().width as f64;
            let x = monitor.position().x as f64 + (mw - WINDOW.0 as f64 * scale) / 2.0;
            ui.window().set_position(PhysicalPosition::new(x.round() as i32, monitor.position().y));
        }
    });
}

/// Clips the window to `size` (an island of that size, with its glow; the
/// hidden island's hover strip): the rest of the window lets the mouse
/// through to whatever is under it.
fn clip_window(ui: &Island, size: (f32, f32), hidden: bool) {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Graphics::Gdi::{CreateRectRgn, SetWindowRgn};
    let scale = ui.window().scale_factor();
    let (w, h) = if hidden { HIDDEN_HOVER } else { (size.0 + 2.0 * GLOW, TOP + size.1 + GLOW) };
    let x0 = ((WINDOW.0 - w) / 2.0 * scale).floor() as i32;
    let x1 = ((WINDOW.0 + w) / 2.0 * scale).ceil() as i32;
    let y1 = (h * scale).ceil() as i32;
    ui.window().with_winit_window(|win: &winit::window::Window| {
        let Ok(handle) = win.window_handle() else { return };
        if let RawWindowHandle::Win32(h) = handle.as_raw() {
            let hwnd = windows::Win32::Foundation::HWND(h.hwnd.get() as *mut core::ffi::c_void);
            unsafe {
                let region = CreateRectRgn(x0, 0, x1, y1);
                // The window owns the region from here on.
                SetWindowRgn(hwnd, Some(region), true);
            }
            popup_style(hwnd);
        }
    });
}

/// What a frame is made of: the island has none.
const FRAME_STYLES: u32 = 0x00C0_0000 /* CAPTION */ | 0x0004_0000 /* THICKFRAME */ | 0x0008_0000 /* SYSMENU */ | 0x0002_0000 /* MINIMIZEBOX */ | 0x0001_0000 /* MAXIMIZEBOX */;
const FRAME_EX_STYLES: u32 = 0x0004_0000 /* APPWINDOW */ | 0x0000_0100 /* WINDOWEDGE */ | 0x0000_0001 /* DLGMODALFRAME */;

/// The island's window for `keep_on_top` (0 until the event loop made it).
static ISLAND_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

/// Puts the island back above every window. Being always-on-top is not
/// enough: other always-on-top windows (a Discord pop-out, a meeting app)
/// go above it when they come forward, and now and then even ordinary
/// windows ended up over it.
fn keep_on_top() {
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE};
    let h = ISLAND_HWND.load(std::sync::atomic::Ordering::Relaxed);
    if h == 0 {
        return;
    }
    unsafe {
        let _ = SetWindowPos(
            windows::Win32::Foundation::HWND(h as *mut core::ffi::c_void),
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

unsafe extern "system" fn on_foreground(
    _: windows::Win32::UI::Accessibility::HWINEVENTHOOK,
    _: u32,
    _: windows::Win32::Foundation::HWND,
    _: i32,
    _: i32,
    _: u32,
    _: u32,
) {
    keep_on_top();
    auto_screen_check();
}

/// Keeps the island on top from now on: each time another app's window comes
/// forward, Windows tells us and the island goes back above it. Nothing is
/// polled. Once, on the UI thread (its message loop delivers the events).
fn guard_on_top(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::Accessibility::SetWinEventHook;
    use windows::Win32::UI::WindowsAndMessaging::{EVENT_SYSTEM_FOREGROUND, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS};
    let first = ISLAND_HWND.swap(hwnd.0 as isize, std::sync::atomic::Ordering::Relaxed) == 0;
    if first {
        unsafe {
            SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(on_foreground), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS);
        }
    }
    keep_on_top();
}

/// The island's native window, once the event loop has made it.
fn island_hwnd(ui: &Island) -> Option<windows::Win32::Foundation::HWND> {
    hwnd_of(ui.window())
}

/// A Slint window's native window, once the event loop has made it.
/// Has the island drawn again in full. A change in the compact island (Line's
/// lyric, the mascot) otherwise stayed off screen until its size changed: the
/// software renderer did not count the new text as changed there.
fn show_changes(ui: &Island) {
    ui.set_repaint(!ui.get_repaint());
}

fn hwnd_of(window: &slint::Window) -> Option<windows::Win32::Foundation::HWND> {
    use slint::winit_030::winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    window
        .with_winit_window(|win: &winit::window::Window| match win.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => Some(windows::Win32::Foundation::HWND(h.hwnd.get() as *mut core::ffi::c_void)),
            _ => None,
        })
        .flatten()
}

thread_local! {
    /// What to flip on each window when Windows asks to see it again.
    static REPAINT: RefCell<HashMap<isize, Box<dyn Fn()>>> = RefCell::new(HashMap::new());
}

/// Slint's software renderer only presents what changed. When another window
/// uncovers one of ours and Windows hasn't kept its picture (some PCs don't),
/// nothing changed, nothing is presented, and the other window's pixels stay
/// in ours. On WM_PAINT `flip` marks the whole window changed, so it is all
/// drawn and presented again. Called once the window is up.
fn repaint_on_expose(window: &slint::Window, flip: impl Fn() + 'static) {
    let Some(hwnd) = hwnd_of(window) else { return };
    REPAINT.with(|m| m.borrow_mut().insert(hwnd.0 as isize, Box::new(flip)));
    unsafe {
        let _ = windows::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(on_expose), 0x5245_5041, 0);
    }
}

unsafe extern "system" fn on_expose(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{WM_NCDESTROY, WM_PAINT};
    // Only a real uncovering: Slint's own redraws come as an internal paint
    // with nothing invalid, and flipping on those would loop forever.
    let exposed = msg == WM_PAINT && unsafe { windows::Win32::Graphics::Gdi::GetUpdateRect(hwnd, None, false) }.as_bool();
    if exposed {
        // Take it out while it runs: flipping may reach Slint, which may paint.
        let flip = REPAINT.with(|m| m.borrow_mut().remove(&(hwnd.0 as isize)));
        if let Some(f) = flip {
            f();
            REPAINT.with(|m| m.borrow_mut().insert(hwnd.0 as isize, f));
        }
    } else if msg == WM_NCDESTROY {
        REPAINT.with(|m| m.borrow_mut().remove(&(hwnd.0 as isize)));
    }
    unsafe { windows::Win32::UI::Shell::DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Lets title changes and activation through without the caption painting
/// that comes with them: the island has no frame to paint.
unsafe extern "system" fn no_caption_paint(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{LPARAM, LRESULT};
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::*;
    match msg {
        // Hidden for the moment of the call, the default handler stores the
        // text without drawing it (Chromium does the same).
        WM_SETTEXT | WM_SETICON => unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style & !(WS_VISIBLE.0 as isize));
            let r = DefSubclassProc(hwnd, msg, wparam, lparam);
            SetWindowLongPtrW(hwnd, GWL_STYLE, style);
            r
        },
        // -1: change the activation state without repainting the frame.
        WM_NCACTIVATE => unsafe { DefSubclassProc(hwnd, msg, wparam, LPARAM(-1)) },
        WM_NCPAINT => LRESULT(0),
        // A monitor came or went, or the PC woke: the island may have been
        // moved off its place or under other windows.
        // A new wallpaper: the Wallpaper color look reads it again.
        WM_SETTINGCHANGE if wparam.0 as u32 == SPI_SETDESKWALLPAPER.0 => unsafe {
            let _ = slint::invoke_from_event_loop(wallpaper_changed);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        WM_DISPLAYCHANGE => unsafe {
            let _ = slint::invoke_from_event_loop(recover_soon);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        WM_POWERBROADCAST if matches!(wparam.0 as u32, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) => unsafe {
            let _ = slint::invoke_from_event_loop(recover_soon);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        // winit rewrites the styles from its own flags now and then (the
        // caption comes back with them): keep them a popup's, whoever asks.
        WM_STYLECHANGING => unsafe {
            let change = &mut *(lparam.0 as *mut STYLESTRUCT);
            if wparam.0 as i32 == GWL_STYLE.0 {
                change.styleNew = (change.styleNew & !FRAME_STYLES) | WS_POPUP.0;
            } else if wparam.0 as i32 == GWL_EXSTYLE.0 {
                change.styleNew = (change.styleNew & !FRAME_EX_STYLES) | WS_EX_TOOLWINDOW.0;
            }
            DefSubclassProc(hwnd, msg, wparam, lparam)
        },
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

/// winit's undecorated window still carries WS_CAPTION and WS_EX_APPWINDOW
/// and only hides the frame through WM_NCCALCSIZE: when the island is
/// activated (a click on a tab) Windows may paint the native title bar for a
/// frame. Make it a plain popup tool window. Idempotent, so it is safe to
/// repeat whenever winit might have rewritten the styles.
fn popup_style(hwnd: windows::Win32::Foundation::HWND) {
    use windows::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let new_style = (style & !FRAME_STYLES) | WS_POPUP.0;
        let new_ex = (ex & !FRAME_EX_STYLES) | WS_EX_TOOLWINDOW.0;
        // Slint sets the title on every window update (a tab change is one):
        // DefWindowProc then paints a classic caption straight over the
        // island, popup or not. The subclass keeps it from painting.
        if !windows::Win32::UI::Shell::SetWindowSubclass(hwnd, Some(no_caption_paint), 0x4655_4255, 0).as_bool() {
            store::log("island: SetWindowSubclass failed");
        }
        if new_style == style && new_ex == ex {
            return;
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, new_style as isize);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

/// Springs the island to `target`. The window region grows at once (so the
/// growing island is never cut) and shrinks once the spring is done.
fn morph_to(ui: &Island, state: &mut State, target: (f32, f32)) {
    let hidden = state.mode == Mode::Hidden;
    if target == state.size_to {
        return;
    }
    let before = state.size_to;
    state.size_to = target;
    ui.set_target_w(target.0);
    ui.set_target_h(target.1);
    let grows = target.0 >= before.0 && target.1 >= before.1;
    if grows {
        clip_window(ui, target, hidden);
        state.region_timer.stop();
    } else {
        // Keep the larger region while it shrinks, then narrow it.
        clip_window(ui, (before.0.max(target.0), before.1.max(target.1)), false);
        let weak = ui.as_weak();
        state.region_timer.start(TimerMode::SingleShot, std::time::Duration::from_millis(MORPH_MS), move || {
            if let Some(ui) = weak.upgrade() {
                let now = with_state(|state| (state.size_to, state.mode == Mode::Hidden));
                if let Some((size, hidden)) = now {
                    clip_window(&ui, size, hidden);
                }
            }
        });
    }
}

/// Shows what the arbiter and the modules say, sizes the island, sets the one timer.
/// The Accounts tab: the Claude Desktop, Claude Code and Codex accounts, one column per app.
/// Returns the page's height: the tallest column (its title, its chips stacked).
fn set_accounts(ui: &Island, pending: &str, hint: &(String, String)) -> f32 {
    ui.set_account_hint_id(hint.0.as_str().into());
    ui.set_account_hint(hint.1.as_str().into());
    let list = accounts::load();
    let mut groups: Vec<AccountGroup> = Vec::new();
    let mut tallest = 0.0f32;
    for (service, title) in accounts::SERVICES.iter() {
        let mut h = ACCOUNT_TITLE_H;
        let rows: Vec<AccountRow> = list
            .iter()
            .filter(|a| a.service == *service)
            .map(|a| {
                let v = usage::view(a);
                let note_lines = if v.note.is_empty() { 0 } else { v.note.lines().count() };
                h += ACCOUNT_SPACING
                    + ACCOUNT_CHIP_H
                    + v.rows.len() as f32 * (USAGE_GAP + USAGE_ROW_H)
                    + if note_lines > 0 { USAGE_GAP + note_lines as f32 * USAGE_NOTE_LINE_H } else { 0.0 };
                let windows: Vec<UsageRow> = v
                    .rows
                    .iter()
                    .map(|r| UsageRow { label: r.label.as_str().into(), used: r.used, used_text: r.used_text.as_str().into(), resets: r.resets.as_str().into() })
                    .collect();
                AccountRow {
                    id: a.id.as_str().into(),
                    name: a.name.as_str().into(),
                    initial: a.name.trim().chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into(),
                    // Once switched, the pending one is the active one.
                    active: a.active,
                    plan: v.plan.as_str().into(),
                    windows: ModelRc::new(VecModel::from(windows)),
                    note: v.note.as_str().into(),
                    note_lines: note_lines as i32,
                }
            })
            .collect();
        if rows.is_empty() {
            continue;
        }
        tallest = tallest.max(h);
        groups.push(AccountGroup { title: (*title).into(), accounts: ModelRc::new(VecModel::from(rows)) });
    }
    let pending = if list.iter().any(|a| a.id == pending && a.active) { "" } else { pending };
    ui.set_account_pending(pending.into());
    if groups.is_empty() {
        ui.set_account_note("No accounts set up yet".into());
    }
    ui.set_account_groups(ModelRc::new(VecModel::from(groups)));
    2.0 * ACCOUNT_PAD + tallest
}

/// Work for the Control worker: read (with the brightness or not), or a change.
enum ControlJob {
    Read(bool),
    Do(control::Command),
}

/// Hands a job to the Control worker; in a snapshot, reads right here.
fn control_send(tx: &Option<std::sync::mpsc::Sender<ControlJob>>, shown: &mut control::Snapshot, job: ControlJob) {
    if snapshot_mode() {
        if let ControlJob::Read(_) = job {
            *shown = control::read();
        }
        return;
    }
    if let Some(tx) = tx {
        let _ = tx.send(job);
    }
}

/// The Control worker: one thread, jobs taken in batches (a volume drag sends
/// many: only the last is applied), the page read again after each batch.
fn start_control_worker(ui: &Island) -> std::sync::mpsc::Sender<ControlJob> {
    let (tx, rx) = std::sync::mpsc::channel::<ControlJob>();
    let weak = ui.as_weak();
    let _ = std::thread::Builder::new().name("kanmuri-control".into()).spawn(move || {
        while let Ok(first) = rx.recv() {
            let mut jobs = vec![first];
            while let Ok(j) = rx.try_recv() {
                jobs.push(j);
            }
            let last_volume = jobs.iter().rposition(|j| matches!(j, ControlJob::Do(control::Command::Volume(_))));
            let mut note = String::new();
            let mut brightness = false;
            for (i, j) in jobs.iter().enumerate() {
                match j {
                    ControlJob::Read(b) => brightness |= *b,
                    ControlJob::Do(control::Command::Volume(_)) if Some(i) != last_volume => {}
                    ControlJob::Do(c) => {
                        if matches!(c, control::Command::Brightness(_)) {
                            brightness = true;
                        }
                        if let Err(e) = control::apply(c) {
                            store::log(&format!("control: {c:?} failed: {e}"));
                            note = format!("Couldn't change it: {e}");
                        }
                    }
                }
            }
            let snap = control::read_with(brightness);
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                with_state(|state| {
                    let keep = state.control.brightness;
                    state.control = snap;
                    if !brightness {
                        state.control.brightness = keep;
                    }
                    state.control_note = note;
                });
                refresh(&ui);
            });
        }
    });
    tx
}

/// Asks Open-Meteo again when the Weather module is on and the last reading
/// is older than weather::REFRESH_MS (or there is none), on a thread.
fn weather_due(ui: &Island, state: &mut State, now: u64) {
    if !state.settings.is_on("weather") || state.weather_busy || snapshot_mode() {
        return;
    }
    if state.weather.as_ref().is_some_and(|(_, at)| now.saturating_sub(*at) < weather::REFRESH_MS) {
        return;
    }
    // After a failure, not again for a minute.
    if state.weather_failed && state.weather.as_ref().is_some_and(|(_, at)| now.saturating_sub(*at) < 60_000) {
        return;
    }
    state.weather_busy = true;
    let (_, lat, lon) = prayer_place(&state.settings);
    let weak = ui.as_weak();
    let _ = std::thread::Builder::new().name("kanmuri-weather".into()).spawn(move || {
        let got = weather::fetch(lat, lon);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            with_state(|state| {
                state.weather_busy = false;
                let now = now_ms(state.started);
                match got {
                    Ok(w) => {
                        state.weather = Some((w, now));
                        state.weather_failed = false;
                    }
                    Err(e) => {
                        store::log(&format!("weather: {e}"));
                        state.weather_failed = true;
                        if let Some((_, at)) = state.weather.as_mut() {
                            *at = now.saturating_sub(weather::REFRESH_MS - 60_000);
                        }
                    }
                }
            });
            refresh(&ui);
        });
    });
}

/// The Control page's weather and its switches that aren't the radios', for Slint.
fn dash_view(state: &State) -> DashView {
    let (place, _, _) = prayer_place(&state.settings);
    let (weather, glyph, temp, desc, high_low) = match (&state.weather, state.settings.is_on("weather")) {
        (_, false) => ("off", "", String::new(), "", String::new()),
        (Some((w, _)), true) => ("ok", weather::glyph(w.code, w.day), format!("{:.0}°", w.now), weather::describe(w.code), format!("H {:.0}°  L {:.0}°", w.high, w.low)),
        (None, true) if state.weather_failed => ("error", "", String::new(), "", String::new()),
        (None, true) => ("loading", "", String::new(), "", String::new()),
    };
    DashView {
        weather: weather.into(),
        glyph: glyph.into(),
        temp: temp.into(),
        desc: desc.into(),
        high_low: high_low.into(),
        place: place.into(),
        keep_awake: state.keep_awake,
        has_dark: state.control.dark.is_some(),
        dark: state.control.dark.unwrap_or(false),
    }
}

/// Keeps the PC and the screen awake (on) or lets them sleep again (off).
/// The flag stays with the UI thread, which lives as long as Kanmuri.
fn set_keep_awake(on: bool) {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED};
    unsafe {
        SetThreadExecutionState(if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED } else { ES_CONTINUOUS });
    }
}

/// The Control page's data for Slint.
fn control_view(c: &control::Snapshot, note: &str) -> ControlView {
    let devices = |list: &[control::Device]| {
        ModelRc::new(VecModel::from(list.iter().take(CONTROL_LIST_MAX).map(|d| ControlDevice { id: d.id.as_str().into(), name: d.name.as_str().into(), on: d.default }).collect::<Vec<_>>()))
    };
    ControlView {
        has_volume: c.volume.is_some(),
        volume: c.volume.map_or(0.0, |v| v.0),
        muted: c.volume.is_some_and(|v| v.1),
        has_mic: c.mic_muted.is_some(),
        mic_muted: c.mic_muted.unwrap_or(false),
        has_brightness: c.brightness.is_some(),
        brightness: c.brightness.map_or(0.0, |b| b as f32 / 100.0),
        has_wifi: c.wifi.is_some(),
        wifi: c.wifi.unwrap_or(false),
        has_bluetooth: c.bluetooth.is_some(),
        bluetooth: c.bluetooth.unwrap_or(false),
        outputs: devices(&c.outputs),
        mics: devices(&c.mics),
        note: note.into(),
    }
}

/// The most outputs / microphones listed.
const CONTROL_LIST_MAX: usize = 4;

/// The Control page's height: the device lists' column (the sliders' one is
/// shorter): padding, two headings, a row each, the gap, a note.
fn control_height(c: &control::Snapshot) -> f32 {
    let rows = c.outputs.len().min(CONTROL_LIST_MAX) + c.mics.len().min(CONTROL_LIST_MAX);
    // Right: padding, the weather row, two headings, a row each, the gap, a note.
    let right = 20.0 + 37.0 + 2.0 * 21.0 + rows as f32 * 29.0 + 9.0 + 14.0;
    // Left: padding, two sliders, two rows of switches (a 40 px disc and its name).
    let left = 24.0 + 2.0 * 28.0 + 3.0 * 10.0 + 4.0 + 2.0 * 59.0;
    right.max(left)
}

/// The Accounts tab just opened: read the plans and usage again (a snapshot
/// inline and offline; otherwise on a thread, then refresh).
fn accounts_opened(ui: &Island) {
    if snapshot_mode() {
        usage::sweep(&accounts::load(), false);
        return;
    }
    let weak = ui.as_weak();
    usage::sweep_in_background(move || {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
    });
}

fn refresh(ui: &Island) {
    with_state(|state| {
        let now = now_ms(state.started);
        daily_tick(state, now);
        // The weather on the Control page: asked for when it is open and the reading is old.
        if state.mode == Mode::Expanded && state.active_tab == "control" {
            weather_due(ui, state, now);
        }
        // Before anything borrows the state: the lyric list is made (once a song) here.
        let lines = lyric_lines(state);
        state.arbiter.tick(now);
        let live: Vec<String> = state.arbiter.ranked().iter().map(|a| a.id.clone()).collect();
        state.pending.retain(|id, _| live.contains(id));
        let view = state.arbiter.view();
        let front_id = view.front.map(|a| a.id.clone());
        let front_sig = view.front.map(|a| format!("{}|{}|{}|{:?}", a.id, a.title, a.body, a.progress)).unwrap_or_default();
        ui.set_has_front(view.front.is_some());
        let scale = ui.window().scale_factor();
        if let Some(front) = view.front {
            let aumid = activity_aumid(&state.notifications, front);
            let logo = logo_image(&mut state.logos, &aumid, pixels::physical(LOGO_NOTICE, scale));
            let small = logo_image(&mut state.logos, &aumid, pixels::physical(LOGO_CAPSULE, scale));
            ui.set_front(item(front, logo, small));
        }
        // A Windows notification with a message shows it (v1's taller chip).
        // So does the music's notice (song, play, pause, stop).
        let notice = view.front.is_some_and(|a| (a.id.starts_with("notif:") || a.id.starts_with("music:event")) && !a.body.trim().is_empty());
        ui.set_notice(notice);
        ui.set_waiting(view.waiting as i32);
        let others: Vec<Item> = state
            .arbiter
            .ranked()
            .into_iter()
            .filter(|a| Some(&a.id) != front_id.as_ref())
            .take(OTHERS_MAX)
            .map(|a| {
                let aumid = activity_aumid(&state.notifications, a);
                item(a, logo_image(&mut state.logos, &aumid, pixels::physical(LOGO_NOTICE, scale)), logo_image(&mut state.logos, &aumid, pixels::physical(LOGO_CAPSULE, scale)))
            })
            .collect();
        ui.set_others(ModelRc::new(VecModel::from(others)));
        // KANMURI_DEMO_PRAYER: the header's clock says the pretend time too.
        // So does KANMURI_DEMO_DAY.
        let clock = if std::env::var_os("KANMURI_DEMO_DAY").is_some() {
            prayer::hh_mm(day_now().3 / 60)
        } else if std::env::var_os("KANMURI_DEMO_PRAYER").is_some() {
            prayer::hh_mm(prayer_now().3 / 60)
        } else {
            local_time::clock_text()
        };
        ui.set_clock(SharedString::from(clock));

        // Tabs, as Mini Island's list: Notifications (when there are some, or
        // no song), Music and Lyrics (while a song plays, Lyrics when it has
        // them), Accounts (once accounts are set up).
        let has_lyrics = state.lyrics.as_ref().is_some_and(|l| l.status == island_lyrics::Status::Found && !l.lines.is_empty());
        let mut tab_ids: Vec<(&str, &str)> = Vec::new();
        let asks = ask_rows(&state.arbiter, &mut state.logos, scale);
        if !state.notifications.is_empty() || !asks.is_empty() || !state.music.has_session {
            tab_ids.push(("notifications", "Notifications"));
        }
        if state.music.has_session {
            // One tab: the player, with the lyrics beside it when the song has some.
            tab_ids.push(("music", "Music"));
        }
        if state.settings.is_on("prayer") {
            tab_ids.push(("prayer", "Prayer"));
        }
        if state.settings.is_on("day") {
            tab_ids.push(("day", "Day Progress"));
        }
        if state.settings.is_on("control") {
            tab_ids.push(("control", "Control"));
        }
        if accounts::installed() {
            tab_ids.push(("accounts", "Accounts"));
        }
        let tabs: Vec<TabInfo> = tab_ids.iter().map(|(id, name)| TabInfo { id: (*id).into(), label: (*name).into() }).collect();
        if !tabs.iter().any(|t| t.id == state.active_tab.as_str()) {
            state.active_tab = tab_ids.first().map_or("notifications", |t| t.0).into();
        }
        ui.set_music_notice(view.front.is_some_and(|a| a.id.starts_with("music:event")));
        ui.set_lyrics_japanese(state.lyrics.as_ref().is_some_and(|l| l.lines.iter().any(|x| x.text.chars().any(|c| matches!(c as u32, 0x3040..=0x30ff | 0x4e00..=0x9fff)))));
        ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
        ui.set_active_tab(state.active_tab.as_str().into());
        let lyrics_on = state.settings.music.lyrics || state.settings.music.lyrics_youtube;
        ui.set_notif_total(state.notifications.len() as i32);
        let view_rows = notif_view(&state.notifications);
        let logos = &mut state.logos;
        let rows: Vec<NotifRow> = view_rows
            .iter()
            .filter_map(|&i| state.notifications.get(i).map(|n| (i, n)))
            .map(|(i, n)| {
                let logo = logo_image(logos, &n.aumid, pixels::physical(LOGO_ROW, scale));
                NotifRow {
                    index: i as i32,
                    app: n.app.as_str().into(),
                    title: n.title.as_str().into(),
                    body: n.body.as_str().into(),
                    time: n.time.as_str().into(),
                    accent: accent_of(&n.app, Kind::Alert),
                    initial: initial(&n.app).into(),
                    has_logo: logo.is_some(),
                    logo: logo.unwrap_or_default(),
                }
            })
            .collect();
        ui.set_notifications(ModelRc::new(VecModel::from(rows)));
        let ask_count = asks.len();
        ui.set_asks(ModelRc::new(VecModel::from(asks)));

        let left = state.focus.left(now);
        let length = state.focus_plan.length_ms(state.focus.phase);
        ui.set_pomodoro(FocusView {
            phase: state.focus.phase.label().into(),
            left: daily::mm_ss(left).into(),
            rounds: format!("Round {} of {}", (state.focus.done_rounds + 1).min(state.focus_plan.rounds), state.focus_plan.rounds).into(),
            running: state.focus.running(),
            on_break: state.focus.phase != daily::Phase::Focus,
            today: {
                let (n, secs) = daily::day_total(&state.data.focus_log, &local_time::today());
                if n == 0 { "No rounds yet today".into() } else { format!("{n} round{} · {}", if n == 1 { "" } else { "s" }, daily::h_m(secs)).into() }
            },
            log: ModelRc::new(VecModel::from(
                state.data.focus_log.iter().rev().filter(|r| r.date == local_time::today()).take(4)
                    .map(|r| RoundRow {
                        time: format!("{}–{}", local_time::hh_mm(r.from_min), local_time::hh_mm(r.to_min)).into(),
                        length: daily::h_m(r.focused_s).into(),
                        full: r.full,
                    })
                    .collect::<Vec<_>>(),
            )),
            fraction: 1.0 - left as f32 / length as f32,
        });
        state.data.water.for_date(&local_time::today());
        ui.set_water(water_view(state));
        if state.active_tab == "prayer" {
            ui.set_prayer(prayer_view(&mut state.prayer_day, &state.settings));
        }
        // Day Progress moves with the clock's per-minute wake (no timer of its own).
        if state.active_tab == "day" {
            ui.set_day(day_view(&mut state.prayer_day, &state.settings));
        }
        let tasks: Vec<TaskRow> = state.data.todo.ordered().into_iter().map(|t| TaskRow { id: t.id as i32, text: t.text.as_str().into(), done: t.done }).collect();
        ui.set_tasks(ModelRc::new(VecModel::from(tasks)));
        ui.set_todo_log(ModelRc::new(VecModel::from(todo_log_rows(&state.data.todo.log, &local_time::today()))));
        ui.set_todo_open(state.data.todo.open_count() as i32);
        let shelf_rows: Vec<ShelfRow> = state.data.shelf.iter().map(|f| ShelfRow { name: f.name().into(), detail: f.detail().into() }).collect();
        ui.set_shelf(ModelRc::new(VecModel::from(shelf_rows)));
        let accounts_open = state.mode == Mode::Expanded && state.active_tab == "accounts";
        if accounts_open && !state.accounts_open {
            accounts_opened(ui);
        }
        state.accounts_open = accounts_open;
        let accounts_h = if state.active_tab == "accounts" { set_accounts(ui, &state.account_pending, &state.account_hint) } else { 0.0 };
        // Control: read again each time the page opens (and after each change).
        let control_open = state.mode == Mode::Expanded && state.active_tab == "control";
        if control_open && !state.control_open {
            control_send(&state.control_tx, &mut state.control, ControlJob::Read(true));
        }
        state.control_open = control_open;
        ui.set_dash(dash_view(state));
        ui.set_control(control_view(&state.control, &state.control_note));
        let control_h = control_height(&state.control);
        ui.set_drop_hover(state.drop_hover);
        *NOTIF_APPS.lock().unwrap_or_else(|e| e.into_inner()) = state.settings.notifications.filter();
        let rating = state.data.ratings.get(&song_key(&state.music)).copied().unwrap_or(0);
        // The pin lives on the Lyrics page: leaving it lets the list close again.
        if state.active_tab == "lyrics" {
            state.active_tab = "music".into();
        }
        if state.active_tab != "music" {
            state.held = false;
        }
        ui.set_held(state.held);
        // Line stays switched on, but shows only while there is music (a
        // browser's session drops out for a moment between songs).
        // Like Mini Island: while a song plays the lyric shows by itself; Line keeps it when paused too.
        // A question from Claude Code takes the capsule (with its Allow / Deny) over the lyric.
        let line = (state.line_held || state.music.playing) && state.music.has_session && !view.front.is_some_and(|a| a.kind == Kind::Ask);
        ui.set_line_held(line);
        let ytm = ytm_live(state, now);
        let capsule_line = if line { capsule_lyric(state) } else { String::new() };
        ui.set_capsule_line(capsule_line.as_str().into());
        let (sweep_at, sweep_len) = if line { capsule_sweep(state) } else { (0, 0) };
        ui.set_sweep_len(sweep_len);
        ui.set_sweep_playing(state.music.playing);
        // Last: setting it restarts the sweep's clock in the UI.
        ui.set_sweep_at(sweep_at);
        let capsule_ro = if line { capsule_lyric_line(state).map(|l| l.romaji.clone()).unwrap_or_default() } else { String::new() };
        ui.set_capsule_romaji(capsule_ro.as_str().into());
        // The line's words with furigana (the あ switch shows them over its kanji).
        let capsule_ruby: Vec<RubySeg> = if line {
            capsule_lyric_line(state)
                .filter(|l| l.ruby.iter().any(|w| !w.ruby.is_empty()))
                .map(|l| l.ruby.iter().map(|w| RubySeg { text: w.text.as_str().into(), ruby: w.ruby.as_str().into() }).collect())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let capsule_has_ruby = !capsule_ruby.is_empty() && ui.get_furigana();
        ui.set_capsule_ruby(ModelRc::new(VecModel::from(capsule_ruby)));
        let capsule_long = lyric_width(&capsule_line) > LYRIC_ONE_LINE;
        ui.set_capsule_long(capsule_long);
        ui.set_music(music_view(&state.music, &state.music_art, ui.window().scale_factor(), state.cover_big, state.music_accent, &state.lyrics, lines, lyrics_on, state.lyrics_nudge, rating, ytm.as_ref()));

        // Modes: a new notice pops the hidden island up; the compact island
        // goes up hide_after() later unless someone waits for an answer.
        let front_kind = view.front.map(|a| a.kind);
        // A multiple-choice question (its options as answers): two rows, question over options.
        let front_question = view.front.is_some_and(is_question);
        // Claude Code busy running a tool doesn't hide the song: the lyric stays,
        // its count shows as "+N". Questions and "finished" notices still come first.
        let song_over = state.music.has_session
            && state.music.playing
            && view.front.is_some_and(|a| matches!(a.kind, Kind::Status | Kind::Progress) && a.source.starts_with("claude"));
        ui.set_song_over(song_over);
        // Do not disturb comes on with a Focus round and goes with its pause or stop.
        let focus_running = state.focus.running();
        if focus_running != state.last_focus_running {
            state.last_focus_running = focus_running;
            state.dnd = focus_running;
        }
        ui.set_dnd(state.dnd);
        let dots = state.settings.system.mic_cam && state.settings.is_on("system");
        ui.set_mic_on(dots && !state.sensors.mic.is_empty());
        ui.set_cam_on(dots && !state.sensors.cam.is_empty());
        // Quiet: nothing pops the island but a question or a call.
        let quiet = state.dnd && front_kind != Some(Kind::Ask) && !view.front.is_some_and(|a| a.id.starts_with("notif:") && is_call(&a.title, &a.body));
        let front_changed = front_id.is_some() && front_id != state.last_front;
        state.last_front = front_id.clone();
        if state.pinned {
            // KANMURI_EXPANDED=compact pins the capsule instead (checks).
            state.mode = if std::env::var("KANMURI_EXPANDED").is_ok_and(|v| v == "compact") { Mode::Compact } else { Mode::Expanded };
            state.hide_at = None;
        } else if front_changed
            && !quiet
            && front_kind != Some(Kind::Ambient)
            && state.mode == Mode::Hidden
            // Settings → General → Pop up for activities; water reminders still come (v1).
            // Prayer's pop-ups too: the user asked for them on the Prayer page.
            && (state.settings.general.popups || matches!(front_id.as_deref(), Some("water:remind" | "prayer:remind")))
        {
            state.mode = Mode::Compact;
            state.hide_at = Some(now + hide_after(&state.settings));
        }
        // A notification floats at least NOTIF_HOLD_MS, a new one again in full
        // even when the capsule is already down.
        if front_changed && !quiet && state.mode == Mode::Compact && front_id.as_deref().is_some_and(|id| id.starts_with("notif:") || id.starts_with("music:event")) {
            let hold = now + hide_after(&state.settings).max(NOTIF_HOLD_MS);
            state.hide_at = Some(state.hide_at.map_or(hold, |t| t.max(hold)));
        }
        let asking = front_kind == Some(Kind::Ask);
        // While Line holds, a new front (a notification, the volume) shows under
        // the lyric instead of over it, for NOTIF_HOLD_MS; a question until answered.
        if front_changed {
            state.line_notice_until = now + NOTIF_HOLD_MS;
        }
        // While a song plays the capsule is the song alone (as Poni's): other news
        // only counts as "+N", and a question takes the whole capsule. A row of
        // news under the lyric kept pushing the lyric about (18 Nov 2026).
        let _ = (asking, state.line_notice_until);
        let line_notice = false;
        ui.set_line_notice(line_notice);
        if asking && state.mode == Mode::Hidden {
            state.mode = Mode::Compact;
        }
        if line && state.mode == Mode::Hidden {
            state.mode = Mode::Compact;
        }
        // Line never hides: no hiding time either (one left in the past made
        // the timer fire every 15 ms, and the busy loop starved the painting).
        if line {
            state.hide_at = None;
        }
        // Line just went (or the music did): the capsule goes up as usual.
        if !line && !state.pinned && state.mode == Mode::Compact && !asking && state.hide_at.is_none() {
            state.hide_at = Some(now + hide_after(&state.settings));
        }
        if state.mode == Mode::Compact && !asking && !line && state.hide_at.is_some_and(|t| now >= t) {
            state.mode = Mode::Hidden;
            state.hide_at = None;
        }
        // "Watch in island": the pointer went from the island onto the video;
        // once it leaves the video too, the island goes back as usual.
        if state.watch_hover {
            let on_video = state.watching.as_ref().and_then(|w| w.hwnd).is_some_and(|h| watch::pointer_over(windows::Win32::Foundation::HWND(h as *mut core::ffi::c_void)));
            if !on_video {
                state.watch_hover = false;
                if !state.pinned && !state.held && state.mode == Mode::Expanded {
                    state.mode = Mode::Compact;
                    state.hide_at = Some(now + hide_after(&state.settings));
                }
            }
        }
        ui.set_watching(state.watching.is_some());
        ui.set_watch_held(state.watch_held);
        ui.set_watch_title(state.watching.as_ref().map(|w| w.title.as_str()).unwrap_or("").into());
        let expanded = state.mode == Mode::Expanded;
        ui.set_mode(match state.mode {
            Mode::Hidden => "hidden",
            Mode::Compact => "compact",
            Mode::Expanded => "expanded",
        }
        .into());
        ui.set_date(local_time::date_text().into());
        ui.set_mascot_on(state.settings.look.mascot);
        ui.set_has_bg(state.bg_image.is_some() && !state.settings.look.background.is_empty());
        ui.set_bg(state.bg_image.clone().unwrap_or_default());
        let theme = state.settings.look.theme();
        if ui.global::<Ice>().get_theme() != theme {
            apply_theme!(ui.global::<Ice>(), theme);
        }
        apply_look(ui, state);
        // Settings → Appearance → Size: the idle capsule and the open island.
        let sizes = state.settings.look.sizes();
        let page_h = sizes.expanded_h - EXPANDED_CHROME;
        let size = match state.mode {
            // Each page as tall as its content, as Mini Island's list.
            Mode::Expanded => {
                let page = match state.active_tab.as_str() {
                    "music" if has_lyrics => sizes.expanded_tall_h - EXPANDED_CHROME,
                    "music" | "prayer" | "day" => page_h,
                    "accounts" => accounts_h.max(page_h),
                    "control" => control_h.max(page_h),
                    _ if state.notifications.is_empty() && ask_count == 0 => page_h,
                    _ => {
                        // Claude Code's questions first, one a row; then the
                        // notifications in two columns (one for a single one).
                        let q = ask_count.min(ASK_ROWS_MAX);
                        let n = notif_view(&state.notifications).len().min(NOTIF_ROWS_MAX);
                        let cols = if n == 1 { 1 } else { 2 };
                        (NOTIF_HEADER_H + (q + n.div_ceil(cols)) as f32 * NOTIF_ROW_PITCH).max(page_h)
                    }
                };
                (sizes.expanded_w, (EXPANDED_CHROME + page).min(EXPANDED_MAX_H))
            }
            Mode::Hidden => HIDDEN,
            // As wide as the line, a notice a row underneath.
            Mode::Compact if line => {
                let w = if capsule_line.is_empty() {
                    COMPACT_SONG.0
                } else {
                    {
                        // The romaji under it is often the longer of the two.
                        let ro = capsule_lyric_line(state).map(|l| l.romaji.chars().count()).unwrap_or(0) as f32 * 6.6;
                        (lyric_width(&capsule_line) as f32 * LYRIC_CHAR_W).max(ro) + LYRIC_PAD
                    }
                    .clamp(COMPACT_LYRIC.0, LYRIC_MAX_W)
                };
                let under = match state.arbiter.view().front.filter(|_| line_notice) {
                    Some(a) if !a.body.trim().is_empty() => LINE_NOTICE_H,
                    Some(_) => LINE_NOTICE_SHORT_H,
                    None => 0.0,
                };
                // Furigana over the line: a row of readings more.
                let ruby_h = if capsule_has_ruby { CAPSULE_RUBY_H } else { 0.0 };
                (if under > 0.0 { w.max(COMPACT_NOTICE.0) } else { w }, COMPACT_LYRIC.1 + ruby_h + under)
            }
            Mode::Compact if notice && front_id.as_deref().is_some_and(|id| id.starts_with("music:event")) => COMPACT_MEDIA,
            Mode::Compact if notice => COMPACT_NOTICE,
            Mode::Compact if front_question => COMPACT_QUESTION,
            Mode::Compact if front_kind == Some(Kind::Ask) => COMPACT_NOTICE,
            Mode::Compact if (front_kind == Some(Kind::Ambient) || song_over) && state.music.has_session => COMPACT_SONG,
            Mode::Compact if front_id.is_some() => COMPACT_BUSY,
            Mode::Compact => {
                {
                    // Room for what the idle island shows next to the time.
                    let extra = slot_room(&ui.get_idle_left()) + slot_room(&ui.get_idle_right());
                    // The time alone keeps Mini Island's idle width; with sides, just their room.
                    // (The idle width less the time's 120 px is its margin.)
                    (if extra > 0.0 { sizes.idle_w - (COMPACT_IDLE.0 - 120.0) + extra } else { sizes.idle_w }, sizes.idle_h)
                }
            }
        };
        morph_to(ui, state, size);
        glass_sync(ui, state);
        follow_watch(ui, state, now);
        // The mascot lives only while it can be seen: no ticks when hidden.
        if state.mode != Mode::Hidden && state.settings.look.mascot {
            if !state.life_timer.running() {
                let weak = ui.as_weak();
                state.life_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(mascot::TICK_MS), move || {
                    if let Some(ui) = weak.upgrade() {
                        mascot_tick(&ui);
                    }
                });
            }
        } else {
            state.life_timer.stop();
        }

        // One timer: the arbiter's next wake, else the clock's next minute.
        let wake = state.arbiter.next_wake(now).map(|t| t.saturating_sub(now));
        let music_on_screen = (expanded && (state.active_tab == "music" || state.active_tab == "lyrics") || line && state.mode == Mode::Compact) && state.music.playing;
        // The lyrics move on more often than the progress bar.
        let page_tick = match (music_on_screen, state.lyrics.as_ref().is_some_and(|l| !l.lines.is_empty())) {
            (true, true) => 250,
            (true, false) => 1_000,
            _ => u64::MAX,
        };
        // The Pomodoro counts down in seconds while its page is on screen.
        let focus_tick = if expanded && state.active_tab == "focus" && state.focus.running() { 1_000 } else { u64::MAX };
        // So does the next prayer, on the second, while its page is on screen.
        let prayer_tick = if expanded && state.active_tab == "prayer" { local_time::ms_to_next_second() } else { u64::MAX };
        let page_tick = page_tick.min(focus_tick).min(prayer_tick).min(daily_wake(state, now));
        let under_in = if line_notice && !asking { state.line_notice_until.saturating_sub(now) } else { u64::MAX };
        let page_tick = page_tick.min(under_in);
        // While a video is docked: often enough to follow the pointer on it,
        // and to find its window right after Chrome opens it.
        let watch_tick = match &state.watching {
            Some(w) if w.hwnd.is_none() || state.watch_hover => 250,
            Some(_) => 1_000,
            None => u64::MAX,
        };
        let page_tick = page_tick.min(watch_tick);
        let hide_in = state.hide_at.filter(|_| state.mode == Mode::Compact && !asking).map_or(u64::MAX, |t| t.saturating_sub(now));
        let delay = wake.unwrap_or(u64::MAX).min(local_time::ms_to_next_minute()).min(page_tick).min(hide_in).max(15);
        let weak = ui.as_weak();
        state.timer.start(TimerMode::SingleShot, std::time::Duration::from_millis(delay), move || {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
        // The idle island's word sides, re-read while they show (and only then).
        let (l, r) = (ui.get_idle_left().to_string(), ui.get_idle_right().to_string());
        let idle_shown = state.mode == Mode::Compact && front_id.is_none() && !line;
        let every = match () {
            _ if !idle_shown => 0,
            _ if is_stat(&l) || is_stat(&r) => STATS_MS,
            _ if l == "prayer" || r == "prayer" => 30_000,
            _ => 0,
        };
        if every != state.stats_every {
            state.stats_every = every;
            if every == 0 {
                state.stats_timer.stop();
            } else {
                let weak = ui.as_weak();
                state.stats_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(every), move || {
                    if let Some(ui) = weak.upgrade() {
                        idle_texts(&ui, true);
                    }
                });
                // Fresh numbers as it comes up, not two seconds later (outside this borrow).
                let weak = ui.as_weak();
                Timer::single_shot(std::time::Duration::ZERO, move || {
                    if let Some(ui) = weak.upgrade() {
                        idle_texts(&ui, true);
                    }
                });
            }
        }
        // Drawn again only when what the capsule shows has changed.
        let shown = format!("{}|{capsule_line}|{line_notice}|{front_sig}|{}|{}", state.watching.is_some(), ui.get_idle_left_text(), ui.get_idle_right_text());
        if state.mode == Mode::Compact && shown != state.last_shown {
            show_changes(ui);
        }
        state.last_shown = shown;
    });
    update_background(ui);
}

/// One tick of the mascot's life: the frame, and where it stands.
fn mascot_tick(ui: &Island) {
    let pose = with_state(|state| {
        let roam = state.mode == Mode::Expanded && state.active_tab == "home";
        state.life.tick(roam)
    });
    if let Some(p) = pose {
        let moved = ui.get_mascot_mood() != p.frame || ui.get_mascot_dx() != p.dx || ui.get_mascot_dy() != p.dy || ui.get_mascot_left() != p.left;
        ui.set_mascot_mood(p.frame.into());
        ui.set_mascot_dx(p.dx);
        ui.set_mascot_dy(p.dy);
        ui.set_mascot_left(p.left);
        if moved && with_state(|state| state.mode == Mode::Compact).unwrap_or(false) {
            show_changes(ui);
        }
    }
}

/// Clicked, or the island came back: she hops.
fn poke_mascot(ui: &Island) {
    with_state(|state| state.life.poke());
    mascot_tick(ui);
}

fn apply(ui: &Island, event: Event) {
    let show = matches!(event, Event::Show);
    with_state(|state| {
        let now = now_ms(state.started);
        match event {
            Event::Upsert(mut activity) => {
                activity.updated_at = now;
                state.arbiter.upsert(activity, now);
            }
            Event::End(id) => {
                state.arbiter.end(&id, now);
            }
            Event::Claude { event, payload, reply } => on_claude(state, &event, &payload, reply, now),
            Event::Ytm(ytm) => state.ytm = Some((ytm, now)),
            Event::Show => {
                if state.mode == Mode::Hidden {
                    state.mode = Mode::Compact;
                    state.hide_at = Some(now + hide_after(&state.settings));
                }
            }
        }
    });
    if show {
        store::log("island: started again while running, brought back");
        recover_island(ui);
    }
    refresh(ui);
}

/// Every live activity whose id starts with `prefix` ends.
fn end_prefix(state: &mut State, prefix: &str, now: u64) {
    let ids: Vec<String> = state.arbiter.ranked().iter().map(|a| a.id.clone()).filter(|id| id.starts_with(prefix)).collect();
    for id in ids {
        state.arbiter.end(&id, now);
        state.pending.remove(&id);
        state.ask_inputs.remove(&id);
    }
}

fn on_claude(state: &mut State, event: &str, payload: &serde_json::Value, reply: Option<island_webhook::Reply>, now: u64) {
    // Module off: no answer, so Claude Code asks in its terminal.
    if !state.settings.is_on("claude") {
        return;
    }
    state.ask_seq += 1;
    let ask_id = format!("claude:{}:ask:{}", claude::session(payload), state.ask_seq);
    let shows = |state: &mut State, list: Vec<serde_json::Value>| {
        for json in list {
            upsert_json(state, json, now);
        }
    };
    match claude::plan(event, payload, &ask_id) {
        claude::Plan::Show(list) => {
            if event == "PermissionRequest" {
                if let Some(r) = reply {
                    state.pending.insert(ask_id.clone(), r);
                }
                // A question: its input, to answer it with the option clicked.
                if payload.get("tool_name").and_then(|t| t.as_str()) == Some("AskUserQuestion") {
                    if let Some(input) = payload.get("tool_input") {
                        state.ask_inputs.insert(ask_id.clone(), input.clone());
                    }
                }
            }
            shows(state, list);
        }
        // Told, not asked: the reply is dropped at once, so the hook prints
        // nothing and Claude Code asks in its terminal.
        claude::Plan::Tell(list) => {
            drop(reply);
            shows(state, list);
        }
        claude::Plan::End(ids, prefix) => {
            for id in ids {
                state.arbiter.end(&id, now);
            }
            if let Some(p) = prefix {
                end_prefix(state, &p, now);
            }
        }
        claude::Plan::EndThenShow(ids, list) => {
            for id in ids {
                state.arbiter.end(&id, now);
            }
            shows(state, list);
        }
        claude::Plan::Nothing => {}
    }
}

/// The Music page's data, from the snapshot, with the position as of now.
/// A song's key for its like/dislike.
fn song_key(snap: &music::Snapshot) -> String {
    format!("{} — {}", snap.artist, snap.title)
}

/// The song's lyric lines as one list for the UI, made only when the lyrics change.
/// How wide a character of a lyric is, in ems: Japanese (and full-width) one
/// em, Latin about half, a space less.
fn char_em(c: char) -> f32 {
    match c {
        ' ' => 0.3,
        c if c.is_ascii_alphanumeric() => 0.58,
        c if c.is_ascii() => 0.4,
        _ => 1.0,
    }
}

/// A line's words with furigana, in rows that fit `width` px (the lines
/// are 14.5 px, the readings 8.5 px): a word is as wide as the wider of the
/// two. A word without furigana wider than a row is broken by character.
fn ruby_rows(words: &[island_lyrics::romaji::Ruby], width: f32) -> Vec<Vec<RubySeg>> {
    let w_of = |text: &str, ruby: &str| {
        let base: f32 = text.chars().map(char_em).sum::<f32>() * 14.5;
        let over = ruby.chars().count() as f32 * 8.5;
        base.max(over)
    };
    let mut pieces: Vec<(String, String)> = Vec::new();
    for w in words {
        if w.ruby.is_empty() && w_of(&w.text, "") > width {
            pieces.extend(w.text.chars().map(|c| (c.to_string(), String::new())));
        } else {
            pieces.push((w.text.clone(), w.ruby.clone()));
        }
    }
    let mut rows: Vec<Vec<RubySeg>> = vec![Vec::new()];
    let mut used = 0.0f32;
    for (text, ruby) in pieces {
        let w = w_of(&text, &ruby);
        let row_empty = rows.last().is_none_or(|r| r.is_empty());
        if !row_empty && used + w > width {
            rows.push(Vec::new());
            used = 0.0;
        }
        // A row doesn't start with a space.
        if used == 0.0 && text.trim().is_empty() {
            continue;
        }
        used += w;
        if let Some(r) = rows.last_mut() {
            r.push(RubySeg { text: text.into(), ruby: ruby.into() });
        }
    }
    rows.retain(|r| !r.is_empty());
    rows
}

fn lyric_lines(state: &mut State) -> Option<ModelRc<LyricLine>> {
    let l = state.lyrics.as_ref().filter(|l| l.status == island_lyrics::Status::Found && !l.lines.is_empty())?;
    let width = state.lyric_width;
    let key = format!("{}#{}#{}#{width}", state.lyrics_key, l.lines.len(), l.lines.iter().map(|x| x.text.len()).sum::<usize>());
    if let Some((k, model)) = &state.lyrics_model {
        if *k == key {
            return Some(model.clone());
        }
    }
    let rows: Vec<LyricLine> = l
        .lines
        .iter()
        .map(|x| {
            let rows = if x.ruby.iter().any(|w| !w.ruby.is_empty()) { ruby_rows(&x.ruby, width) } else { Vec::new() };
            LyricLine {
                text: if x.text.trim().is_empty() { "♪".into() } else { x.text.as_str().into() },
                romaji: x.romaji.as_str().into(),
                current: false,
                has_ruby: !rows.is_empty(),
                rows: ModelRc::new(VecModel::from(rows.into_iter().map(|segs| RubyRow { segs: ModelRc::new(VecModel::from(segs)) }).collect::<Vec<_>>())),
            }
        })
        .collect();
    let model = ModelRc::new(VecModel::from(rows));
    state.lyrics_model = Some((key, model.clone()));
    Some(model)
}

/// The line the capsule shows while Line holds: the lyric being sung, else
/// nothing (the capsule then runs the song's title and artist, as Mini
/// Island's small pill does between lines).
fn capsule_lyric(state: &State) -> String {
    capsule_lyric_line(state).map(|l| l.text.trim().to_string()).unwrap_or_default()
}

/// The line being sung now, for the capsule (its text and romaji).
/// The capsule lyric's karaoke sweep: how far into its line the song is (ms)
/// and how long the line lasts (ms, until the next line, at most 12 s).
/// How much later the lines run than they were timed (an MV's intro). Not for a
/// browser: its length is missing or 2x-6x the song, which pushed the lyrics up
/// to 8 s late (18 Nov 2026).
fn intro_shift(snap: &music::Snapshot, timed_for: f64) -> f64 {
    let browser = matches!(snap.app.to_lowercase().as_str(), "chrome" | "edge" | "firefox" | "brave" | "opera" | "vivaldi");
    if browser { 0.0 } else { island_lyrics::intro_shift(snap.duration, timed_for) }
}

fn capsule_sweep(state: &State) -> (i32, i32) {
    let snap = &state.music;
    let Some(l) = state.lyrics.as_ref().filter(|l| l.status == island_lyrics::Status::Found && !l.lines.is_empty()) else { return (0, 0) };
    let shift = intro_shift(snap, l.duration) + state.lyrics_nudge;
    let at = snap.position_now() - shift;
    let Some(i) = island_lyrics::line_at(&l.lines, at) else { return (0, 0) };
    let start = l.lines[i].time;
    let end = l.lines.get(i + 1).map_or(start + 5.0, |n| n.time).min(start + 12.0);
    (((at - start).max(0.0) * 1000.0) as i32, ((end - start).max(0.3) * 1000.0) as i32)
}

fn capsule_lyric_line(state: &State) -> Option<&island_lyrics::Line> {
    let snap = &state.music;
    let l = state.lyrics.as_ref().filter(|l| l.status == island_lyrics::Status::Found && !l.lines.is_empty())?;
    let shift = intro_shift(snap, l.duration) + state.lyrics_nudge;
    island_lyrics::line_at(&l.lines, snap.position_now() - shift).map(|i| &l.lines[i])
}

/// A line's width in Latin characters: kana and kanji count twice.
fn lyric_width(text: &str) -> usize {
    text.chars().map(|c| if c as u32 >= 0x2E80 { 2 } else { 1 }).sum()
}

/// The extension's report, while it is fresh (it repeats every 15 s) and the
/// music is playing in a browser.
fn ytm_live(state: &State, now: u64) -> Option<island_webhook::YtmState> {
    let (ytm, at) = state.ytm.as_ref()?;
    let browser = matches!(state.music.app.as_str(), "Chrome" | "Edge" | "Brave" | "Opera") || state.music.app.to_lowercase().contains("chrome");
    (browser && now.saturating_sub(*at) < 40_000).then(|| ytm.clone())
}

/// The song's length as the island shows it: a browser's (Apple Music on the
/// web) is missing or 2x-6x the song, so the length the lyrics were timed for
/// wins there, and anywhere the player's own is no use. 0 when unknown.
fn song_length(snap: &music::Snapshot, position: f64, lyric_len: f64) -> f64 {
    let browser = matches!(snap.app.to_lowercase().as_str(), "chrome" | "edge" | "firefox" | "brave" | "opera" | "vivaldi");
    if (browser || snap.duration <= position + 0.5) && lyric_len > position {
        lyric_len
    } else if snap.duration > position + 0.5 {
        snap.duration
    } else {
        0.0
    }
}

/// A click or drag on the progress bar (0..1 of its width) as seconds into
/// the song: None when the length is unknown or the player can't seek.
fn seek_target(fraction: f64, length: f64, can_seek: bool) -> Option<f64> {
    (can_seek && length > 0.0 && fraction.is_finite()).then(|| fraction.clamp(0.0, 1.0) * length)
}

/// The song's cover, rounded, ART_PX square, and its images at the sizes it
/// is drawn (physical px), made once each: the software renderer scales
/// pictures by the nearest pixel, which left the rounded corners jagged.
struct Art {
    px: Vec<u8>,
    sized: std::cell::RefCell<HashMap<u32, slint::Image>>,
}

impl Art {
    fn new(rounded: Vec<u8>) -> Self {
        Art { px: rounded, sized: Default::default() }
    }

    fn at(&self, phys: u32) -> slint::Image {
        let phys = phys.clamp(1, music::ART_PX);
        self.sized.borrow_mut().entry(phys).or_insert_with(|| pixels::image(&pixels::resize_square(&self.px, music::ART_PX, phys), phys)).clone()
    }
}

/// Where the cover is drawn, logical px: the capsule, the music card, the
/// lyrics page's player; the Music page's big one is measured by the page.
const COVER_CAPSULE: f32 = 30.0;
const COVER_CARD: f32 = 40.0;
const COVER_LYRICS: f32 = 64.0;

#[allow(clippy::too_many_arguments)]
fn music_view(
    snap: &music::Snapshot,
    art: &Option<Art>,
    scale: f32,
    big: f32,
    accent: Option<(u8, u8, u8)>,
    lyrics: &Option<island_lyrics::Lyrics>,
    lines: Option<ModelRc<LyricLine>>,
    lyrics_on: bool,
    nudge: f64,
    rating: i8,
    ytm: Option<&island_webhook::YtmState>,
) -> Music {
    let position = snap.position_now();
    // The lines are timed for the recording they came from: an MV's intro
    // (and the user's nudge) moves them later.
    let shift = lyrics.as_ref().map_or(0.0, |l| intro_shift(snap, l.duration)) + nudge;
    let sung_at = position - shift;
    // The song's length: a browser's (Apple Music on the web) is missing or 2x-6x
    // the song, so the length the lyrics were timed for wins there, and anywhere
    // the player's own is no use.
    let length = song_length(snap, position, lyrics.as_ref().map_or(0.0, |l| l.duration));
    let fraction = if length > 0.0 { (position / length).min(1.0) as f32 } else { 0.0 };
    // The line being sung (none before the first); the list centres on it.
    let at = lyrics.as_ref().filter(|_| lines.is_some()).and_then(|l| island_lyrics::line_at(&l.lines, sung_at));
    let note = match (lyrics_on, lyrics.as_ref().map(|l| l.status)) {
        (false, _) => "",
        (true, None) => "Looking for lyrics…",
        (true, Some(island_lyrics::Status::Instrumental)) => "Instrumental",
        (true, Some(island_lyrics::Status::Error)) => "Lyrics service unreachable",
        (true, Some(_)) => "No synced lyrics",
    };
    Music {
        has_session: snap.has_session,
        title: snap.title.as_str().into(),
        artist: snap.artist.as_str().into(),
        app: snap.app.as_str().into(),
        playing: snap.playing,
        position: fraction,
        elapsed: local_time::mm_ss(position).into(),
        // A browser often reports no length (or a wrong one): no "-0:00" then.
        remaining: if length > 0.0 { format!("-{}", local_time::mm_ss((length - position).max(0.0))).into() } else { SharedString::default() },
        has_art: art.is_some(),
        // Before the Music page has said how big its cover is: the full picture.
        art: art.as_ref().map(|a| a.at(if big > 0.0 { pixels::physical(big, scale) } else { music::ART_PX })).unwrap_or_default(),
        art_30: art.as_ref().map(|a| a.at(pixels::physical(COVER_CAPSULE, scale))).unwrap_or_default(),
        art_40: art.as_ref().map(|a| a.at(pixels::physical(COVER_CARD, scale))).unwrap_or_default(),
        art_64: art.as_ref().map(|a| a.at(pixels::physical(COVER_LYRICS, scale))).unwrap_or_default(),
        has_accent: accent.is_some(),
        accent: accent.map_or_else(slint::Color::default, |(r, g, b)| slint::Color::from_rgb_u8(r, g, b)),
        has_lyrics: lines.is_some(),
        lyrics: lines.unwrap_or_default(),
        lyric_index: at.map_or(-1, |i| i as i32),
        lyric_center: at.unwrap_or(0) as i32,
        lyrics_note: note.into(),
        lyrics_shift: if shift.abs() >= 0.05 { format!("{shift:+.1} s").into() } else { SharedString::default() },
        // The player's own say first; YouTube Music in a browser through the extension.
        can_shuffle: snap.shuffle.is_some() || ytm.is_some(),
        shuffle: snap.shuffle.map_or(ytm.is_some_and(|y| y.shuffle), |s| s),
        can_repeat: snap.repeat.is_some() || ytm.is_some(),
        repeat: match (snap.repeat, ytm.map(|y| y.repeat.as_str())) {
            (Some(music::Repeat::All), _) => 1,
            (Some(music::Repeat::One), _) => 2,
            (Some(music::Repeat::Off), _) => 0,
            (None, Some("all")) => 1,
            (None, Some("one")) => 2,
            _ => 0,
        },
        rating: rating as i32,
        length: length as f32,
        can_seek: seek_target(0.0, length, snap.can_seek).is_some(),
    }
}

/// A new snapshot from the Music module: the page, and the ambient "now playing".
fn on_music(ui: &Island, snap: music::Snapshot) {
    with_state(|state| {
        let now = now_ms(state.started);
        let ytm_cover = !state.cover_for.is_empty() && state.cover_for == state.lyrics_key;
        if state.music.art != snap.art && !ytm_cover {
            state.music_art = snap.art.as_ref().and_then(|rgba| {
                (rgba.len() == (music::ART_PX * music::ART_PX * 4) as usize).then(|| {
                    let mut rounded = rgba.clone();
                    round_cover(&mut rounded, music::ART_PX);
                    Art::new(rounded)
                })
            });
            state.music_accent = snap.art.as_deref().and_then(accent::of_rgba);
        }
        if snap.has_session && snap.playing {
            let json = serde_json::json!({
                "id": "music:now",
                "source": if snap.app.is_empty() { "music".to_string() } else { snap.app.clone() },
                "kind": "ambient",
                "title": format!("♪ {}", snap.title),
                "body": snap.artist,
            });
            if let Ok(a) = island_core::Incoming::parse(json.to_string().as_bytes()).and_then(|i| i.into_activity(now)) {
                state.arbiter.upsert(a, now);
            }
        } else {
            state.arbiter.end("music:now", now);
        }
        // A new song, play, pause and stop float down in the small card like a
        // notification (not in the first seconds, when the session is found).
        let old = &state.music;
        let event = if old.has_session && !snap.has_session {
            Some(("Stopped", format!("{} · {}", old.title, old.artist)))
        } else if snap.has_session && !snap.title.is_empty() && song_key(&snap) != song_key(old) {
            snap.playing.then(|| ("Now playing", format!("{} · {}", snap.title, snap.artist)))
        } else if snap.has_session && old.has_session && snap.playing != old.playing {
            Some((if snap.playing { "Playing" } else { "Paused" }, format!("{} · {}", snap.title, snap.artist)))
        } else {
            None
        };
        if let Some((title, body)) = event.filter(|_| now > MUSIC_EVENTS_AFTER && !state.dnd) {
            state.arbiter.end(&format!("music:event:{}", state.music_events), now);
            state.music_events += 1;
            let json = serde_json::json!({
                "id": format!("music:event:{}", state.music_events),
                "source": if snap.app.is_empty() { old.app.clone() } else { snap.app.clone() },
                "kind": "status",
                "title": title,
                "body": body.trim_end_matches(" · "),
                "ttl_ms": NOTIF_HOLD_MS,
            });
            upsert_json(state, json, now);
        }
        state.music = snap;
    });
    fetch_lyrics(ui);
    refresh(ui);
}

/// Asks for the lyrics of the song playing, once a song (and again when the
/// lyrics switches change), on a thread of its own.
fn fetch_lyrics(ui: &Island) {
    let job = with_state(|state| {
        let m = &state.settings.music;
        let sources = island_lyrics::Sources { lrclib: m.lyrics, youtube: m.lyrics_youtube, romaji: m.romaji };
        if !state.settings.is_on("music") || !state.music.has_session || (!sources.lrclib && !sources.youtube) {
            state.lyrics_key.clear();
            state.lyrics = None;
            return None;
        }
        // A browser's length can't be trusted (Apple Music in a browser reports 2x-6x the song,
        // or nothing): the lookup goes by name there, and the key stays the same while that
        // number moves.
        let browser = matches!(state.music.app.to_lowercase().as_str(), "chrome" | "edge" | "firefox" | "brave" | "opera" | "vivaldi");
        let query = island_lyrics::Query {
            title: state.music.title.clone(),
            artist: state.music.artist.clone(),
            album: state.music.album.clone(),
            duration: if browser { 0.0 } else { state.music.duration },
        };
        let key = format!("{}\u{1f}{}{}{}", query.key(), sources.lrclib as u8, sources.youtube as u8, sources.romaji as u8);
        if key == state.lyrics_key {
            return None;
        }
        state.lyrics_nudge = 0.0;
        // A live stream: nothing to look up (a search finds another song's). A music player's
        // song without a length (it names an album) is still looked up.
        if !island_lyrics::worth_looking_up(&query) {
            state.lyrics_key = key;
            state.lyrics = Some(island_lyrics::Lyrics::of(island_lyrics::Status::None));
            return None;
        }
        state.lyrics_key = key.clone();
        state.lyrics = None;
        state.cover_for.clear();
        Some((key, query, sources, browser && sources.youtube))
    })
    .flatten();
    let Some((key, query, sources, want_cover)) = job else { return };
    // A browser's song: its real cover from YouTube Music, in its own thread.
    if want_cover {
        let weak = ui.as_weak();
        let (key, query) = (key.clone(), query.clone());
        std::thread::spawn(move || {
            let Some(rgba) = island_lyrics::cover(&query).and_then(|b| music::decode_bytes(&b)) else { return };
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                with_state(|state| {
                    if state.lyrics_key != key || rgba.len() != (music::ART_PX * music::ART_PX * 4) as usize {
                        return;
                    }
                    let mut rounded = rgba.clone();
                    round_cover(&mut rounded, music::ART_PX);
                    state.music_art = Some(Art::new(rounded));
                    state.music_accent = accent::of_rgba(&rgba);
                    state.cover_for = key;
                });
                refresh(&ui);
            });
        });
    }
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let lyrics = island_lyrics::resolve(&query, sources);
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            with_state(|state| {
                // Still the same song?
                if state.lyrics_key == key {
                    state.lyrics = Some(lyrics);
                }
            });
            refresh(&ui);
        });
    });
}

/// The app list the notification thread reads (it can't reach the UI state).
static NOTIF_APPS: Mutex<Vec<String>> = Mutex::new(Vec::new());
const NOTIF_HISTORY: usize = 30;

fn start_notifications(ui: &Island) {
    let weak = ui.as_weak();
    let watcher = notif::start(
        || NOTIF_APPS.lock().unwrap_or_else(|e| e.into_inner()).clone(),
        move |n| {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    on_notification(&ui, n);
                }
            });
        },
        // Access known: Windows' banners follow the module.
        || {
            let _ = slint::invoke_from_event_loop(|| {
                with_state(sync_banners);
            });
        },
    );
    with_state(|state| state.notif_watcher = Some(watcher));
}

/// While the Notifications module is on (and can read them), Windows' pop-up
/// banners are off for every app that is on here, the Start menu's included,
/// so even a first notification shows only on the island; back as they were
/// for the others, when the module is off, or without access.
fn sync_banners(state: &mut State) {
    // A snapshot check never touches Windows' settings.
    if snapshot_mode() {
        return;
    }
    let want = state.settings.is_on("notifications") && notif::has_access();
    // The apps switched off here keep their banners: nothing would show them.
    let off: Vec<String> = state.settings.notifications.off.iter().filter_map(|name| state.data.notif_apps.get(name).cloned()).collect();
    let mut kept = std::mem::take(&mut state.data.banners);
    let mut changed = banners::give_back(banners::SETTINGS, &mut kept, |a| !want || off.iter().any(|o| o == a));
    if want {
        let mut apps = banners::apps(banners::SETTINGS);
        apps.extend(banners::start_apps());
        changed |= banners::take(banners::SETTINGS, apps.iter().map(String::as_str).filter(|a| !off.iter().any(|o| o == a)), &mut kept);
    }
    state.data.banners = kept;
    if changed {
        store::save_data(&state.data);
    }
}

/// A call by its words (WhatsApp, Teams, Phone Link…): it comes through Do not disturb.
fn is_call(title: &str, body: &str) -> bool {
    let text = format!("{title} {body}").to_lowercase();
    ["call", "panggilan", "telepon", "memanggil"].iter().any(|w| text.contains(w))
}

/// KANMURI_SNAPSHOT: a picture is drawn and the app quits.
/// A browser's video picture-in-picture window docked under the island
/// (found by the WinEvent hook; none for a snapshot's demo).
struct Watching {
    hwnd: Option<isize>,
    title: String,
    /// When it was docked (ms since start).
    since: u64,
    /// Where Kanmuri last put it (physical pixels): elsewhere means the
    /// user dragged it, and it is let go.
    placed: Option<(i32, i32)>,
}

fn as_hwnd(h: isize) -> windows::Win32::Foundation::HWND {
    windows::Win32::Foundation::HWND(h as *mut core::ffi::c_void)
}

/// A browser's video picture-in-picture appeared, or the docked one may have
/// gone (from the WinEvent hook, and once at start for one already open):
/// dock a new one, let go of a closed one.
fn pip_check(ui: &Island) {
    if snapshot_mode() {
        return;
    }
    let changed = with_state(|state| {
        let now = now_ms(state.started);
        if let Some(w) = &state.watching {
            if w.hwnd.is_some_and(|h| !watch::alive(as_hwnd(h))) {
                state.watching = None;
                state.watch_held = false;
                state.watch_hover = false;
                watch::track(None);
                return true;
            }
            return false;
        }
        let Some(h) = watch::find_plain() else {
            state.watch_freed = None;
            return false;
        };
        // The one the user dragged away stays free until it is closed.
        if state.watch_freed == Some(h.0 as isize) {
            return false;
        }
        state.watch_freed = None;
        let title = if state.music.has_session { state.music.title.clone() } else { String::new() };
        state.watching = Some(Watching { hwnd: Some(h.0 as isize), title, since: now, placed: None });
        // Just opened: shown at once, until the user lets it follow the island.
        state.watch_held = true;
        watch::track(Some(h));
        true
    })
    .unwrap_or(false);
    if changed {
        refresh(ui);
    }
}

/// The island's "back to tab": the picture-in-picture window is closed, and
/// the browser puts the video back in its tab.
fn watch_back() {
    if let Some(h) = with_state(|state| state.watching.as_ref().and_then(|w| w.hwnd)).flatten() {
        watch::close(as_hwnd(h));
    }
}

/// Keeps the docked video under the island: shown while the island is open,
/// the video is held or the pointer is on it, hidden otherwise (Chrome keeps
/// playing). Dragged away by the user, it is let go: Chrome's PiP is
/// movable again, until it is closed.
fn follow_watch(ui: &Island, state: &mut State, now: u64) {
    if snapshot_mode() {
        return;
    }
    let Some(w) = state.watching.as_ref() else { return };
    let Some(hwnd) = w.hwnd.map(as_hwnd).filter(|h| watch::alive(*h)) else {
        // Closed (Chrome hides a closed PiP): let go now.
        state.watching = None;
        state.watch_held = false;
        state.watch_hover = false;
        watch::track(None);
        return;
    };
    // Not where it was put (and not Chrome settling it right after it
    // opened): the user dragged it. Theirs now.
    if let Some((px, py)) = w.placed {
        let r = watch::rect(hwnd);
        if now.saturating_sub(w.since) > 1_500 && ((r.left - px).abs() > 8 || (r.top - py).abs() > 8) {
            state.watch_freed = Some(hwnd.0 as isize);
            state.watching = None;
            state.watch_held = false;
            state.watch_hover = false;
            watch::track(None);
            return;
        }
    }
    let Some(island) = hwnd_of(ui.window()) else { return };
    let r = watch::rect(island);
    let scale = ui.window().scale_factor();
    let below = if state.mode == Mode::Expanded { state.size_to.1 } else { state.settings.look.sizes().expanded_h };
    let x = (r.left + r.right) / 2;
    let y = r.top + ((TOP + below + 10.0) * scale).round() as i32;
    let shown = state.watch_held || state.mode == Mode::Expanded || state.watch_hover;
    // As wide as the island's video area: 384 px (Chrome's own size is 366).
    let width = (384.0 * scale).round() as i32;
    let at = watch::place(hwnd, Some(island), x, y, shown, width);
    if let Some(w) = state.watching.as_mut() {
        w.placed = Some(at);
    }
}

fn snapshot_mode() -> bool {
    std::env::var_os("KANMURI_SNAPSHOT").is_some()
}

/// A new notification: into the history, and up on the island if pop-ups are on.
fn on_notification(ui: &Island, n: notif::Notif) {
    with_state(|state| {
        let now = now_ms(state.started);
        // Do not disturb: into the history only, unless it's a call.
        if state.settings.notifications.popups && (!state.dnd || is_call(&n.title, &n.body)) {
            let json = serde_json::json!({
                "id": format!("notif:{}", n.id),
                "source": n.app,
                "kind": "alert",
                "title": if n.title.is_empty() { n.app.clone() } else { n.title.clone() },
                "body": n.body,
            });
            if let Ok(a) = island_core::Incoming::parse(json.to_string().as_bytes()).and_then(|i| i.into_activity(now)) {
                state.arbiter.upsert(a, now);
            }
        }
        // A new app's banners go too (this first one already showed): by its
        // own AUMID, as some apps' settings sit in nested keys Windows makes
        // only then.
        let off_here = state.settings.notifications.off.iter().any(|o| o.eq_ignore_ascii_case(&n.app));
        if !snapshot_mode() && state.settings.is_on("notifications") && notif::has_access() && !off_here && !n.aumid.is_empty() && !n.aumid.starts_with(notif::FLASH_PREFIX) && !state.data.banners.contains_key(&n.aumid) {
            if banners::take(banners::SETTINGS, [n.aumid.as_str()], &mut state.data.banners) {
                store::save_data(&state.data);
            }
        }
        state.notifications.retain(|old| old.id != n.id);
        state.notifications.push_front(n);
        state.notifications.truncate(NOTIF_HISTORY);
    });
    refresh(ui);
}

/// A new message in an app without Windows notifications (WeChat): a
/// notification like any other, when the Notifications module is on and the
/// app is allowed there.
fn on_flash(ui: &Island, f: flash::Flash) {
    let n = with_state(|state| {
        let aumid = flash::key(&f.exe, &f.app);
        let id = state.flash_id;
        state.flash_id = state.flash_id.checked_add(1).unwrap_or(4_000_000_000);
        let n = notif::Notif { id, app: f.app.clone(), aumid, title: "New message".into(), body: format!("Open {} to read it", f.app), time: local_time::clock_text() };
        // Listed under Settings' per-app switches either way.
        notif::note(&n);
        let on = state.settings.is_on("notifications") && notif::allowed(&state.settings.notifications.filter(), &n.app, &n.aumid);
        on.then_some(n)
    })
    .flatten();
    if let Some(n) = n {
        on_notification(ui, n);
    }
}

/// The room a side of the idle island takes, px: fixed per kind (the widest
/// it gets), so the capsule doesn't twitch as a number changes.
fn slot_room(k: &str) -> f32 {
    match k {
        "date" => 84.0,
        "network" => 26.0,
        "battery" => 64.0,
        // "Maghrib 18:05"
        "prayer" => 100.0,
        // "CPU 100%", "RAM 100%", "C: 100%"
        "cpu" => 72.0,
        "memory" => 74.0,
        "disk" => 62.0,
        _ => 0.0,
    }
}

/// The sides that are system stats (read every STATS_MS while shown).
fn is_stat(k: &str) -> bool {
    matches!(k, "cpu" | "memory" | "disk")
}
const STATS_MS: u64 = 2_000;

/// The next prayer and its time, "Asar 15:12" (Subuh tomorrow after Isya).
fn next_prayer_short(cache: &mut Option<(String, prayer::Day)>, settings: &Settings) -> String {
    let (y, m, d, secs) = prayer_now();
    let day = prayer_day(cache, settings, y, m, d);
    match day.prayers().into_iter().find(|(_, at)| at * 60 > secs) {
        Some((w, at)) => format!("{} {}", w.name(), prayer::hh_mm(at)),
        None => {
            let (_, lat, lon) = prayer_place(settings);
            let (ty, tm, td) = local_time::add_days(y, m, d, 1);
            let at = prayer::day(ty, tm, td, lat, lon, local_time::utc_offset_hours()).get(prayer::Which::Subuh);
            format!("{} {}", prayer::Which::Subuh.name(), prayer::hh_mm(at))
        }
    }
}

/// The text of the idle island's sides that are words: the next prayer, and
/// (when `read_stats`) CPU, memory and disk read now; a stat not read keeps
/// what it showed.
fn idle_texts(ui: &Island, read_stats: bool) {
    let (left, right) = (ui.get_idle_left().to_string(), ui.get_idle_right().to_string());
    let stats = if read_stats && (is_stat(&left) || is_stat(&right)) {
        Some((stats::cpu_percent(), stats::memory_percent(), stats::disk_percent()))
    } else {
        None
    };
    let text = |k: &str, old: SharedString| -> SharedString {
        let pct = |v: Option<u32>| v.map_or("–".to_string(), |p| format!("{p}%"));
        match (k, stats) {
            ("prayer", _) => with_state(|s| next_prayer_short(&mut s.prayer_day, &s.settings)).unwrap_or_default().into(),
            ("cpu", Some((c, _, _))) => format!("CPU {}", pct(c)).into(),
            ("memory", Some((_, mem, _))) => format!("RAM {}", pct(mem)).into(),
            ("disk", Some((_, _, d))) => format!("C: {}", pct(d)).into(),
            (k, None) if is_stat(k) && !old.is_empty() => old,
            ("cpu", None) => "CPU –".into(),
            ("memory", None) => "RAM –".into(),
            ("disk", None) => "C: –".into(),
            _ => SharedString::default(),
        }
    };
    let (old_l, old_r) = (ui.get_idle_left_text(), ui.get_idle_right_text());
    let (new_l, new_r) = (text(&left, old_l.clone()), text(&right, old_r.clone()));
    if new_l != old_l || new_r != old_r {
        ui.set_idle_left_text(new_l);
        ui.set_idle_right_text(new_r);
        // The software renderer can miss a change in the compact island.
        show_changes(ui);
    }
}

/// A word side as Settings' preview shows it, read now (real numbers).
fn slot_preview(kind: &str, s: &Settings) -> String {
    let pct = |v: Option<u32>| v.map_or("–".to_string(), |p| format!("{p}%"));
    match kind {
        "prayer" => next_prayer_short(&mut None, s),
        "cpu" => format!("CPU {}", pct(stats::cpu_percent())),
        "memory" => format!("RAM {}", pct(stats::memory_percent())),
        "disk" => format!("C: {}", pct(stats::disk_percent())),
        _ => String::new(),
    }
}

/// The idle island's status icons, per Settings > General > Idle island.
fn show_status(ui: &Island) {
    let st = system::status_now();
    let g = with_state(|s| s.settings.general.clone()).unwrap_or_default();
    ui.set_status_net(st.net.into());
    match st.battery {
        Some((p, charging)) => {
            ui.set_status_battery(format!("{p}%").into());
            ui.set_status_battery_level(p as f32 / 100.0);
            ui.set_status_charging(charging);
        }
        None => ui.set_status_battery("".into()),
    }
    // A battery slot on a PC without one shows nothing.
    let slot = |k: &str| if k == "battery" && st.battery.is_none() { String::new() } else { k.to_string() };
    ui.set_idle_left(slot(&g.idle_left).into());
    ui.set_idle_right(slot(&g.idle_right).into());
    idle_texts(ui, true);
    refresh(ui);
}

/// A change from the System module: shown when its switch is on (and the module is).
fn on_system(ui: &Island, e: system::Event) {
    if matches!(e, system::Event::Power(..) | system::Event::Network(_) | system::Event::Status) {
        show_status(ui);
    }
    if e == system::Event::Status {
        return;
    }
    // The volume changed elsewhere (keys, the tray): the Control page follows.
    if let system::Event::Volume(v, muted) = e {
        with_state(|state| {
            if state.control.volume.is_some() {
                state.control.volume = Some((v, muted));
            }
        });
    }
    with_state(|state| {
        let sys = &state.settings.system;
        let (on, id) = match e {
            system::Event::Volume(..) => (sys.volume, "sys:volume"),
            system::Event::CapsLock(_) => (sys.caps_lock, "sys:caps"),
            system::Event::Layout(_) => (sys.keyboard_layout, "sys:layout"),
            system::Event::Power(..) => (sys.charger, "sys:power"),
            system::Event::Network(_) => (sys.network, "sys:network"),
            system::Event::Headphones(..) => (sys.audio_device, "sys:audio"),
            system::Event::Status => return,
        };
        if !on || !state.settings.is_on("system") {
            return;
        }
        let now = now_ms(state.started);
        let (title, progress) = system::describe(&e);
        let source = format!("icon:{}", system::icon(&e));
        let mut json = serde_json::json!({ "id": id, "source": source, "kind": "status", "title": title });
        if let Some(p) = progress {
            json["progress"] = serde_json::json!(p);
        }
        if let Ok(a) = island_core::Incoming::parse(json.to_string().as_bytes()).and_then(|i| i.into_activity(now)) {
            state.arbiter.upsert(a, now);
        }
    });
    refresh(ui);
}

// ── Daily modules ─────────────────────────────────────────────────────────────

fn upsert_json(state: &mut State, json: serde_json::Value, now: u64) {
    if let Ok(a) = island_core::Incoming::parse(json.to_string().as_bytes()).and_then(|i| i.into_activity(now)) {
        state.arbiter.upsert(a, now);
    }
}

/// The Water page: today against the goal, the next reminder, the hours.
fn water_view(state: &State) -> WaterView {
    let plan = &state.water_plan;
    let day = &state.data.water;
    let total = day.total();
    let minute = local_time::minute_of_day();
    let next = if total >= plan.goal_ml {
        "Goal reached".to_string()
    } else if plan.every_min == 0 {
        "Reminders off".to_string()
    } else {
        match daily::next_reminder(plan, day, minute, state.water_reminded) {
            Some(due) if due <= minute => "Time for a glass".to_string(),
            Some(due) => format!("Next reminder {}", local_time::hh_mm(due)),
            None if minute < plan.from_min => format!("Reminders from {}", local_time::hh_mm(plan.from_min)),
            None => "Done for today".to_string(),
        }
    };
    let chart = day.per_hour(plan.from_min / 60, plan.until_min.saturating_sub(1) / 60);
    let top = chart.iter().map(|(_, ml)| *ml).max().unwrap_or(0).max(plan.glass_ml) as f32;
    let hours: Vec<HourBar> = chart
        .iter()
        .map(|(h, ml)| HourBar { label: format!("{h}").into(), fraction: *ml as f32 / top, now: *h == minute / 60 })
        .collect();
    WaterView {
        total: format!("{total}").into(),
        goal: format!("{} ml", plan.goal_ml).into(),
        glass: format!("{} ml", plan.glass_ml).into(),
        percent: format!("{}%", (total * 100 / plan.goal_ml.max(1)).min(999)).into(),
        fraction: (total as f32 / plan.goal_ml.max(1) as f32).min(1.0),
        drinks: day.drinks.len() as i32,
        next: next.into(),
        hours_text: if plan.every_min == 0 {
            format!("Active {}–{}", local_time::hh_mm(plan.from_min), local_time::hh_mm(plan.until_min))
        } else {
            format!("{} min · {}–{}", plan.every_min, local_time::hh_mm(plan.from_min), local_time::hh_mm(plan.until_min))
        }
        .into(),
        hours: ModelRc::new(VecModel::from(hours)),
        due: plan.every_min > 0 && matches!(daily::next_reminder(plan, day, minute, state.water_reminded), Some(d) if d <= minute),
    }
}

/// The running Pomodoro as a quiet progress line ("Focus · until 15:40").
fn focus_activity(state: &mut State, now: u64) {
    if state.focus.running() && state.settings.is_on("focus") {
        let left_min = state.focus.left(now).div_ceil(60_000) as u32;
        let until = local_time::hh_mm(local_time::minute_of_day() + left_min);
        let length = state.focus_plan.length_ms(state.focus.phase) as f32;
        let progress = 1.0 - state.focus.left(now) as f32 / length;
        let json = serde_json::json!({
            "id": "focus", "source": "focus", "kind": "progress",
            "title": format!("{} · until {until}", state.focus.phase.label()),
            "progress": progress,
        });
        upsert_json(state, json, now);
    } else {
        state.arbiter.end("focus", now);
    }
}

/// Writes the focus round under way to the log (a phase that ended, was
/// skipped or reset): how long the user actually focused.
fn log_focus(state: &mut State, now: u64, full: bool) {
    if state.focus.phase != daily::Phase::Focus {
        return;
    }
    let length = state.focus_plan.length_ms(daily::Phase::Focus);
    let focused_s = if full { length / 1000 } else { length.saturating_sub(state.focus.left(now)) / 1000 } as u32;
    let to_min = local_time::minute_of_day();
    let from_min = state.focus_started_min.take().unwrap_or(to_min.saturating_sub(focused_s / 60));
    daily::log_round(&mut state.data.focus_log, daily::FocusRound { date: local_time::today(), from_min, to_min, focused_s, full });
    store::save_data(&state.data);
}

/// Time-driven daily things: a Pomodoro phase that ended, a water reminder due.
fn daily_tick(state: &mut State, now: u64) {
    if state.focus.ends_at.is_some_and(|end| now >= end) {
        log_focus(state, now, true);
    }
    if let Some(ended) = state.focus.tick(now, &state.focus_plan) {
        let next = state.focus.phase.label();
        let title = match ended {
            daily::Phase::Focus => format!("Focus done · {next} next"),
            _ => "Break over · back to focus".to_string(),
        };
        upsert_json(state, serde_json::json!({ "id": "focus:done", "source": "focus", "kind": "alert", "title": title }), now);
        focus_activity(state, now);
    }
    if state.settings.is_on("water") {
        state.data.water.for_date(&local_time::today());
        let minute = local_time::minute_of_day();
        if let Some(due) = daily::next_reminder(&state.water_plan, &state.data.water, minute, state.water_reminded) {
            if due <= minute {
                state.water_reminded = Some(minute);
                let title = format!("Time for water · {} / {} ml", state.data.water.total(), state.water_plan.goal_ml);
                upsert_json(state, serde_json::json!({ "id": "water:remind", "source": "water", "kind": "alert", "title": title }), now);
            }
        }
    }
    if state.settings.is_on("prayer") {
        prayer_tick(state, now);
    }
}

/// Milliseconds until a daily module needs a look (a phase end, a reminder).
fn daily_wake(state: &State, now: u64) -> u64 {
    let mut wake = u64::MAX;
    if let Some(end) = state.focus.ends_at {
        wake = wake.min(end.saturating_sub(now));
    }
    if state.settings.is_on("water") {
        let minute = local_time::minute_of_day();
        if let Some(due) = daily::next_reminder(&state.water_plan, &state.data.water, minute, state.water_reminded) {
            wake = wake.min(if due > minute { (due - minute) as u64 * 60_000 } else { 1_000 });
        }
    }
    if state.settings.is_on("prayer") {
        if let Some((_, day)) = state.prayer_day.as_ref() {
            let (_, _, _, secs) = prayer_now();
            let next = prayer_events(day, &state.settings.prayer).into_iter().map(|e| e.0 * 60).find(|&at| at > secs);
            // None left today: just after midnight, for the next day's.
            let at = next.unwrap_or(24 * 3600);
            wake = wake.min((at - secs) as u64 * 1000 + 50);
        }
    }
    wake
}

// ── Prayer ────────────────────────────────────────────────────────────────────

/// Now for the Prayer module: (year, month, day, seconds since midnight).
/// KANMURI_DEMO_PRAYER=HH:MM pretends it is that time today (snapshots).
fn prayer_now() -> (i32, u32, u32, u32) {
    static DEMO: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    let demo = *DEMO.get_or_init(|| std::env::var("KANMURI_DEMO_PRAYER").ok().and_then(|v| parse_hh_mm(&v)));
    let (y, m, d, secs) = local_time::now_parts();
    (y, m, d, demo.map_or(secs, |min| min * 60))
}

/// The chosen place (Settings > Prayer), else the first preset.
fn prayer_place(s: &Settings) -> (&'static str, f64, f64) {
    prayer::PLACES.iter().copied().find(|p| p.0 == s.prayer.place).unwrap_or(prayer::PLACES[0])
}

/// The day's times, worked out again only for a new date, place or time zone.
fn prayer_day(cache: &mut Option<(String, prayer::Day)>, settings: &Settings, y: i32, m: u32, d: u32) -> prayer::Day {
    let (name, lat, lon) = prayer_place(settings);
    let tz = local_time::utc_offset_hours();
    let key = format!("{y}-{m:02}-{d:02}|{name}|{tz}");
    if cache.as_ref().is_none_or(|(k, _)| *k != key) {
        *cache = Some((key, prayer::day(y, m, d, lat, lon, tz)));
    }
    cache.as_ref().map(|(_, day)| day.clone()).unwrap_or_else(|| prayer::day(y, m, d, lat, lon, tz))
}

/// The day's pop-ups in order: (minute, prayer, a reminder before it).
fn prayer_events(day: &prayer::Day, p: &island_core::settings::PrayerSettings) -> Vec<(u32, prayer::Which, bool)> {
    let before = p.remind_before_min.min(30);
    let mut events = Vec::new();
    for (w, at) in day.prayers() {
        if before > 0 && at >= before {
            events.push((at - before, w, true));
        }
        if p.at_time {
            events.push((at, w, false));
        }
    }
    events.sort_by_key(|e| (e.0, !e.2));
    events
}

/// A prayer pop-up that came due since the last look (the latest one, and
/// only while it is still timely).
fn prayer_tick(state: &mut State, now: u64) {
    let (y, m, d, secs) = prayer_now();
    let date = format!("{y}-{m:02}-{d:02}");
    let minute = secs / 60;
    let day = prayer_day(&mut state.prayer_day, &state.settings, y, m, d);
    if state.prayer_seen.0 != date {
        state.prayer_seen = (date, -1);
    }
    let seen = state.prayer_seen.1;
    let due = prayer_events(&day, &state.settings.prayer).into_iter().filter(|e| e.0 as i64 > seen && e.0 <= minute).last();
    state.prayer_seen.1 = seen.max(minute as i64);
    let Some((at, w, before)) = due else { return };
    if minute - at > PRAYER_LATE_MIN {
        return;
    }
    let time = prayer::hh_mm(day.get(w));
    let title = if before { format!("{} in {} min · {time}", w.name(), day.get(w) - at) } else { format!("{} · {time}", w.name()) };
    upsert_json(state, serde_json::json!({ "id": "prayer:remind", "source": "icon:prayer", "kind": "alert", "title": title }), now);
}

/// Where a minute of the day falls on the timeline, 0..1: the five
/// prayers evenly spaced (0, 1/4 .. 1), and a time between two of them in
/// between their dots by how much of that stretch has gone; before the
/// first at the start, after the last at the end.
fn timeline_at(times: &[u32], minute: f32) -> f32 {
    let n = times.len();
    if n < 2 {
        return 0.0;
    }
    let step = 1.0 / (n - 1) as f32;
    for (i, w) in times.windows(2).enumerate() {
        let (a, b) = (w[0] as f32, w[1] as f32);
        if minute < b {
            let part = ((minute - a) / (b - a).max(1.0)).clamp(0.0, 1.0);
            return (i as f32 + part) * step;
        }
    }
    1.0
}

/// The Prayer page: the next prayer and how long until it, the day's marks
/// and the five prayers on a line from Subuh to Isya.
fn prayer_view(cache: &mut Option<(String, prayer::Day)>, settings: &Settings) -> PrayerView {
    let (y, m, d, secs) = prayer_now();
    let day = prayer_day(cache, settings, y, m, d);
    let (place, lat, lon) = prayer_place(settings);
    let five = day.prayers();
    let (next, next_s, tomorrow) = match five.iter().find(|(_, at)| at * 60 > secs) {
        Some(&(w, at)) => (w, at * 60, false),
        None => {
            let (ty, tm, td) = local_time::add_days(y, m, d, 1);
            let at = prayer::day(ty, tm, td, lat, lon, local_time::utc_offset_hours()).get(prayer::Which::Subuh);
            (prayer::Which::Subuh, 24 * 3600 + at * 60, true)
        }
    };
    let left = next_s - secs;
    let (deg, point) = prayer::qibla(lat, lon);
    let times: Vec<u32> = five.iter().map(|p| p.1).collect();
    let usable = PRAYER_TRACK_W - 2.0 * PRAYER_TRACK_INSET;
    let x_of = |min: f32| PRAYER_TRACK_INSET + timeline_at(&times, min) * usable;
    let marks: Vec<PrayerMark> = five
        .iter()
        .map(|&(w, at)| PrayerMark {
            name: w.name().into(),
            time: prayer::hh_mm(at).into(),
            dot_x: x_of(at as f32),
            past: at * 60 <= secs,
            next: w == next,
        })
        .collect();
    PrayerView {
        next_name: next.name().into(),
        countdown: format!("{:02}:{:02}:{:02}", left / 3600, left / 60 % 60, left % 60).into(),
        when: format!("{} {}", prayer::hh_mm(next_s / 60), if tomorrow { "tomorrow" } else { "today" }).into(),
        place: place.into(),
        info: format!(
            "Imsak {} · Terbit {} · Qibla {deg}° {point}",
            prayer::hh_mm(day.get(prayer::Which::Imsak)),
            prayer::hh_mm(day.get(prayer::Which::Terbit))
        )
        .into(),
        now_text: prayer::hh_mm(secs / 60).into(),
        now_x: x_of(secs as f32 / 60.0),
        track_from: PRAYER_TRACK_INSET,
        track_to: PRAYER_TRACK_W - PRAYER_TRACK_INSET,
        marks: ModelRc::new(VecModel::from(marks)),
    }
}

// ── Day Progress ──────────────────────────────────────────────────────────────

/// Now for Day Progress: (year, month, day, seconds since midnight).
/// KANMURI_DEMO_DAY=HH:MM pretends it is that time today (snapshots).
fn day_now() -> (i32, u32, u32, u32) {
    static DEMO: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    let demo = *DEMO.get_or_init(|| std::env::var("KANMURI_DEMO_DAY").ok().and_then(|v| parse_hh_mm(&v)));
    let (y, m, d, secs) = local_time::now_parts();
    (y, m, d, demo.map_or(secs, |min| min * 60))
}

/// The Day Progress page: the waking day (wake to bedtime) with now on it,
/// the week, month and year, and the next prayer when Prayer is on (Kanmuri
/// has no calendar, so nothing else is "coming up").
fn day_view(cache: &mut Option<(String, prayer::Day)>, settings: &Settings) -> DayView {
    let (y, m, d, secs) = day_now();
    let (wake, bed) = settings.day.span();
    let f = day::day_fraction(secs, wake, bed);
    let detail = if secs < wake * 60 {
        format!("of your day · it starts at {}", prayer::hh_mm(wake))
    } else if secs >= bed * 60 {
        format!("of your day · past bedtime ({})", prayer::hh_mm(bed))
    } else {
        String::new()
    };
    let left = if detail.is_empty() { day::h_m(day::left_until(secs, bed).max(1)) } else { String::new() };
    let card = |label: &str, f: f32| DayCard { label: label.into(), percent: format!("{}%", day::percent(f)).into(), fraction: f };
    let mut next = (String::new(), String::new(), String::new());
    if settings.is_on("prayer") {
        let today = prayer_day(cache, settings, y, m, d);
        let found = today.prayers().into_iter().find(|(_, at)| at * 60 > secs).map(|(w, at)| (w, at, at * 60));
        let found = found.or_else(|| {
            let (_, lat, lon) = prayer_place(settings);
            let (ty, tm, td) = local_time::add_days(y, m, d, 1);
            let at = prayer::day(ty, tm, td, lat, lon, local_time::utc_offset_hours()).get(prayer::Which::Subuh);
            Some((prayer::Which::Subuh, at, 24 * 3600 + at * 60))
        });
        if let Some((w, at, at_s)) = found {
            let tomorrow = at_s >= 24 * 3600;
            let mins = (at_s - secs).div_ceil(60);
            next = (
                w.name().into(),
                format!("{}{}", prayer::hh_mm(at), if tomorrow { " tomorrow" } else { "" }),
                format!("in {}", day::h_m(mins)),
            );
        }
    }
    DayView {
        percent: format!("{}%", day::percent(f)).into(),
        left: left.into(),
        bedtime: prayer::hh_mm(bed).into(),
        detail: detail.into(),
        fraction: f,
        ticks: ModelRc::new(VecModel::from(
            day::ticks(wake, bed).into_iter().map(|(t, at)| DayTick { text: prayer::hh_mm(t).into(), at }).collect::<Vec<_>>(),
        )),
        cards: ModelRc::new(VecModel::from(vec![
            card("WEEK", day::week_fraction(y, m, d, secs)),
            card("MONTH", day::month_fraction(y, m, d, secs)),
            card("YEAR", day::year_fraction(y, m, d, secs)),
        ])),
        has_next: !next.0.is_empty(),
        next_name: next.0.into(),
        next_when: next.1.into(),
        next_in: next.2.into(),
    }
}

fn start_music(ui: &Island) {
    let weak = ui.as_weak();
    music::start(move |snap| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                on_music(&ui, snap);
            }
        });
    });
}

// ── Settings ────────────────────────────────────────────────────────────────

/// Fills the Settings window from the settings. Don't call it inside
/// `with_state`: it reads the background preview from the state itself.
fn fill_settings(win: &SettingsWindow, s: &Settings) {
    let modules: Vec<ModuleRow> = s
        .modules
        .iter()
        .filter_map(|m| module_info(&m.id).map(|info| ModuleRow { id: m.id.as_str().into(), name: info.name.into(), enabled: m.enabled }))
        .collect();
    let (installed, status) = match claude::read_settings() {
        Ok(v) if claude::installed(&v) => (true, format!("Hooks in {}.", claude::settings_path().display())),
        Ok(_) => (false, "Connecting adds Kanmuri's hooks to Claude Code's settings.json (a backup first).".to_string()),
        Err(e) => (false, e),
    };
    let preview = with_state(|state| state.bg_preview.clone()).flatten();
    let wallpaper = with_state(|state| state.wallpaper).flatten();
    let sizes = s.look.sizes();
    // The apps for the per-app switches: those seen now and before, by name.
    let known = with_state(|state| {
        let seen = notif::seen_apps();
        if seen.iter().any(|(k, _)| !state.data.notif_apps.contains_key(k)) {
            state.data.notif_apps.extend(seen);
            store::save_data(&state.data);
        }
        state.data.notif_apps.keys().cloned().collect::<Vec<_>>()
    })
    .unwrap_or_default();
    let notif_list: Vec<AppSwitch> = known
        .iter()
        .map(|name| AppSwitch { name: name.as_str().into(), on: !s.notifications.off.iter().any(|o| o.eq_ignore_ascii_case(name)) })
        .collect();
    let w = &s.water;
    let plan = w.plan();
    // What the idle island's preview shows (General > Idle island).
    let now = system::status_now();
    // The screens to pick from: the main one first, then the others by name.
    let mut screens: Vec<ScreenRow> = Vec::new();
    // Asked of the island's window: a Settings window just made has no native
    // window yet, so it saw no screens and General hid Screen and Move
    // automatically (19 Nov 2026).
    let island = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade()));
    let any = |f: &mut dyn FnMut(&winit::window::Window)| {
        let done = island.as_ref().and_then(|ui| ui.window().with_winit_window(|w: &winit::window::Window| f(w)));
        if done.is_none() {
            win.window().with_winit_window(|w: &winit::window::Window| f(w));
        }
    };
    any(&mut |w: &winit::window::Window| {
        let main = w.primary_monitor().and_then(|m| m.name());
        let mut all: Vec<String> = w.available_monitors().filter_map(|m| m.name()).collect();
        all.sort();
        for (i, name) in all.iter().enumerate() {
            let is_main = main.as_deref() == Some(name.as_str());
            screens.push(ScreenRow { id: if is_main { "".into() } else { name.as_str().into() }, label: format!("Screen {}{}", i + 1, if is_main { " (main)" } else { "" }).into() });
        }
    });
    screens.sort_by_key(|r| !r.id.is_empty());
    // KANMURI_DEMO_SCREENS=2: two made-up screens (a snapshot sees none).
    if screens.len() < 2 && std::env::var_os("KANMURI_DEMO_SCREENS").is_some() {
        screens = vec![ScreenRow { id: "".into(), label: "Screen 1 (main)".into() }, ScreenRow { id: "\\\\.\\DISPLAY2".into(), label: "Screen 2".into() }];
    }
    win.set_s(SettingsView {
        screens: ModelRc::new(VecModel::from(screens)),
        screen: s.general.screen.as_str().into(),
        auto_screen: s.general.auto_screen.as_str().into(),
        modules: ModelRc::new(VecModel::from(modules)),
        lyrics: s.music.lyrics,
        lyrics_youtube: s.music.lyrics_youtube,
        romaji: s.music.romaji,
        notif_popups: s.notifications.popups,
        notif_access: notif::has_access(),
        notif_list: ModelRc::new(VecModel::from(notif_list)),
        sys_volume: s.system.volume,
        sys_caps: s.system.caps_lock,
        sys_layout: s.system.keyboard_layout,
        sys_charger: s.system.charger,
        sys_network: s.system.network,
        sys_audio: s.system.audio_device,
        sys_mic_cam: s.system.mic_cam,
        autostart: s.autostart,
        mascot: s.look.mascot,
        clock: local_time::clock_text().into(),
        date: local_time::date_text().into(),
        net: now.net.into(),
        battery: now.battery.map(|(p, _)| format!("{p}%")).unwrap_or_default().into(),
        battery_level: now.battery.map(|(p, _)| p as f32 / 100.0).unwrap_or(0.0),
        popups: s.general.popups,
        idle_left: s.general.idle_left.as_str().into(),
        idle_right: s.general.idle_right.as_str().into(),
        idle_left_text: slot_preview(&s.general.idle_left, s).into(),
        idle_right_text: slot_preview(&s.general.idle_right, s).into(),
        hide_after: s.general.hide_after_s.clamp(2, 60) as i32,
        theme: s.look.theme().into(),
        has_background: !s.look.background.is_empty(),
        background_name: Path::new(&s.look.background).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into(),
        background_preview: preview.unwrap_or_default(),
        background_dim: (s.look.background_dim.clamp(0.0, 0.9) * 100.0).round() as i32,
        look_style: s.look.style().into(),
        glass_tint: s.look.glass_tint.as_str().into(),
        look_color: look::color(s.look.color_rgb()),
        look_color_hex: island_core::look::hex(s.look.color_rgb()).into(),
        wallpaper_color: look::color(wallpaper.unwrap_or((0x0b, 0x0b, 0x0d))),
        night_warm: s.look.night_warm,
        night_warmth: s.look.night_warmth.min(100) as i32,
        closed_opacity: s.look.closed_opacity.clamp(20, 100) as i32,
        idle_w: sizes.idle_w as i32,
        idle_h: sizes.idle_h as i32,
        expanded_w: sizes.expanded_w as i32,
        expanded_h: sizes.expanded_h as i32,
        expanded_tall_h: sizes.expanded_tall_h as i32,
        water_reminders: w.reminders,
        water_goal: plan.goal_ml as i32,
        water_glass: plan.glass_ml as i32,
        water_every: w.every_min.clamp(10, 480) as i32,
        water_from: plan.from_min as i32,
        water_until: plan.until_min as i32,
        focus_min: s.focus.plan().focus_min as i32,
        short_min: s.focus.plan().short_min as i32,
        long_min: s.focus.plan().long_min as i32,
        rounds: s.focus.plan().rounds as i32,
        prayer_places: ModelRc::new(VecModel::from(prayer::PLACES.iter().map(|p| SharedString::from(p.0)).collect::<Vec<_>>())),
        prayer_place: prayer_place(s).0.into(),
        prayer_remind: s.prayer.remind_before_min.min(30) as i32,
        prayer_at_time: s.prayer.at_time,
        day_wake: s.day.span().0 as i32,
        day_bedtime: s.day.span().1 as i32,
        data_folder: store::data_dir().display().to_string().into(),
        claude_status: status.into(),
        claude_installed: installed,
    });
    apply_theme!(win.global::<Ice>(), s.look.theme());
}

/// Changes the settings, saves them, and shows the result everywhere.
fn change_settings(ui: &Island, f: impl FnOnce(&mut Settings)) {
    let refill = with_state(|state| {
        f(&mut state.settings);
        *SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = state.settings.general.screen.clone();
        *AUTO_MODE.lock().unwrap_or_else(|e| e.into_inner()) = state.settings.general.auto_screen.clone();
        glass::TINT.store(match state.settings.look.glass_tint.as_str() { "dark" => 1, "light" => 2, _ => 0 }, std::sync::atomic::Ordering::Relaxed);
        // Water and Focus follow at once (v1: new lengths apply to a phase not yet started).
        state.water_plan = state.settings.water.plan();
        let plan = state.settings.focus.plan();
        if plan != state.focus_plan {
            state.focus_plan = plan;
            if !state.focus.running() && state.focus.done_rounds == 0 && state.focus.phase == daily::Phase::Focus {
                state.focus.reset(&plan);
            }
        }
        if let Err(err) = store::save(&state.settings) {
            eprintln!("settings not saved: {err}");
        }
        sync_banners(state);
        state.settings_window.as_ref().map(|w| (w.as_weak(), state.settings.clone()))
    })
    .flatten();
    if let Some((win, settings)) = refill {
        if let Some(win) = win.upgrade() {
            fill_settings(&win, &settings);
        }
    }
    refresh(ui);
}

fn set_flag(s: &mut Settings, key: &str, on: bool) {
    match key {
        "music.lyrics" => s.music.lyrics = on,
        "music.lyrics_youtube" => s.music.lyrics_youtube = on,
        "music.romaji" => s.music.romaji = on,
        "notifications.popups" => s.notifications.popups = on,
        "system.volume" => s.system.volume = on,
        "system.caps_lock" => s.system.caps_lock = on,
        "system.keyboard_layout" => s.system.keyboard_layout = on,
        "system.charger" => s.system.charger = on,
        "system.network" => s.system.network = on,
        "system.audio_device" => s.system.audio_device = on,
        "system.mic_cam" => s.system.mic_cam = on,
        "look.mascot" => s.look.mascot = on,
        "look.night_warm" => s.look.night_warm = on,
        "general.popups" => s.general.popups = on,
        "water.reminders" => s.water.reminders = on,
        "prayer.at_time" => s.prayer.at_time = on,
        "autostart" => {
            if let Err(err) = autostart::set(on) {
                eprintln!("autostart: {err}");
            }
            s.autostart = on;
        }
        _ => {}
    }
}

/// A number from a stepper, by key; the ranges are the settings' own.
fn set_number(s: &mut Settings, key: &str, v: i32) {
    let u = v.max(0) as u32;
    match key {
        "general.hide_after_s" => s.general.hide_after_s = u.clamp(2, 60),
        "look.background_dim" => s.look.background_dim = (u.min(90) as f32) / 100.0,
        "look.night_warmth" => s.look.night_warmth = u.min(100),
        "look.closed_opacity" => s.look.closed_opacity = u.clamp(20, 100),
        "look.idle_w" => s.look.idle_w = u,
        "look.idle_h" => s.look.idle_h = u,
        "look.expanded_w" => s.look.expanded_w = u,
        "look.expanded_h" => s.look.expanded_h = u,
        "look.expanded_tall_h" => s.look.expanded_tall_h = u,
        "water.every_min" => s.water.every_min = u.clamp(10, 480),
        "water.goal_ml" => s.water.goal_ml = u.clamp(100, 10_000),
        "water.glass_ml" => s.water.glass_ml = u.clamp(10, 2000),
        "water.from" => s.water.from = local_time::hh_mm(u.min(1439)),
        "water.until" => s.water.until = local_time::hh_mm(u.min(1439)),
        "focus.focus_min" => s.focus.focus_min = u.clamp(1, 180),
        "focus.short_min" => s.focus.short_min = u.clamp(1, 60),
        "focus.long_min" => s.focus.long_min = u.clamp(1, 120),
        "focus.rounds" => s.focus.rounds = u.clamp(1, 12),
        "prayer.remind_before_min" => s.prayer.remind_before_min = u.min(30),
        "day.wake" => s.day.wake = local_time::hh_mm(u.min(1439)),
        "day.bedtime" => s.day.bedtime = local_time::hh_mm(u.min(1439)),
        _ => {}
    }
}

/// Puts Settings → Appearance → Look on the island: its colours (set again
/// only when they changed: the night's warmth moves with refresh's
/// per-minute wake, never per frame), and whether Liquid Glass is on.
fn apply_look(ui: &Island, state: &mut State) {
    let warmth = if state.settings.look.night_warm {
        // Sunset and sunrise at the Prayer module's place, else 18:00 / 06:00.
        let (y, m, d, secs) = prayer_now();
        let (sunset, sunrise) = if state.settings.is_on("prayer") {
            let day = prayer_day(&mut state.prayer_day, &state.settings, y, m, d);
            (day.get(prayer::Which::Maghrib), day.get(prayer::Which::Terbit))
        } else {
            (18 * 60, 6 * 60)
        };
        look::warmth(&state.settings.look, secs as f32 / 60.0, sunset, sunrise)
    } else {
        0.0
    };
    let c = look::colors(&state.settings.look, state.wallpaper, warmth, state.glass_light);
    if state.look_shown.as_ref() == Some(&c) {
        return;
    }
    let mini = ui.global::<Mini>();
    mini.set_base(look::color(c.base));
    mini.set_solid(c.solid);
    mini.set_closed(c.closed);
    mini.set_light(c.light);
    mini.set_ink(look::color(c.ink[0]));
    mini.set_text_2(look::color(c.ink[1]));
    mini.set_text_3(look::color(c.ink[2]));
    mini.set_label(look::color(c.ink[3]));
    mini.set_faint(look::color(c.ink[4]));
    mini.set_wash(look::color(c.wash));
    mini.set_glass(c.glass);
    state.look_shown = Some(c);
}

// ── Liquid Glass ─────────────────────────────────────────────────────────────

/// The keyboard hook that lets screenshots see the island (0 = none).
static SHOT_HOOK: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
/// How long the island stays in screen capture after a screenshot key:
/// Snip & Sketch and Print Screen freeze the screen within a second.
const SHOT_MS: u64 = 4000;

/// Liquid Glass leaves the island out of screen capture, so screenshots
/// missed it. While the look is on, a low-level keyboard hook (Windows calls
/// it per key, nothing is polled) watches for Print Screen, Win+Shift+S and
/// Win+Shift+R: the island goes back into capture for a few seconds, the
/// glass keeps its last frame meanwhile. Off with the look.
fn screenshot_keys(on: bool) {
    use std::sync::atomic::Ordering::Relaxed;
    use windows::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, UnhookWindowsHookEx, HHOOK, WH_KEYBOARD_LL};
    let h = SHOT_HOOK.load(Relaxed);
    unsafe {
        if on && h == 0 {
            let module = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok().map(|m| windows::Win32::Foundation::HINSTANCE(m.0));
            if let Ok(hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(on_shot_key), module, 0) {
                SHOT_HOOK.store(hook.0 as isize, Relaxed);
            }
        } else if !on && h != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(h as *mut core::ffi::c_void));
            SHOT_HOOK.store(0, Relaxed);
        }
    }
}

unsafe extern "system" fn on_shot_key(code: i32, wparam: windows::Win32::Foundation::WPARAM, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_RWIN, VK_SHIFT, VK_SNAPSHOT};
    use windows::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_SYSKEYDOWN};
    if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
        let k = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = |v: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe { GetAsyncKeyState(v.0 as i32) } < 0;
        let win = down(VK_LWIN) || down(VK_RWIN);
        let snip = win && down(VK_SHIFT) && (k.vkCode == b'S' as u32 || k.vkCode == b'R' as u32);
        if k.vkCode == VK_SNAPSHOT.0 as u32 || snip {
            screenshot_soon();
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// A screenshot is coming: the island back into capture now, out again once
/// it is taken.
fn screenshot_soon() {
    glass::SHOOTING_UNTIL.store(glass::now_ms() + SHOT_MS, std::sync::atomic::Ordering::Relaxed);
    let Some(ui) = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade())) else { return };
    if let Some(hwnd) = island_hwnd(&ui) {
        glass::exclude_from_capture(hwnd, false);
    }
    // Later, from the event loop (not inside the hook): left out again.
    Timer::single_shot(std::time::Duration::from_millis(SHOT_MS + 50), || {
        if glass::shooting() {
            return;
        }
        if let Some(ui) = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade())) {
            with_state(|state| {
                state.glass_excluded = None;
                glass_sync(&ui, state);
            });
        }
    });
}

/// Starts or stops Liquid Glass' clock: it runs only while the look is
/// Liquid Glass and the island is shown (0 % CPU hidden). Also leaves the
/// island out of screen capture while the look is on (and only then: it
/// hides the island from screenshots and recordings too).
fn glass_sync(ui: &Island, state: &mut State) {
    let look_on = state.settings.look.style() == "liquid";
    let on = look_on && state.mode != Mode::Hidden;
    if snapshot_mode() {
        if on {
            glass_demo_frame(ui, state);
        }
        return;
    }
    if look_on {
        screenshot_keys(true);
    } else {
        screenshot_keys(false);
    }
    if state.glass_excluded != Some(look_on) && !glass::shooting() {
        if let Some(hwnd) = island_hwnd(ui) {
            let ok = glass::exclude_from_capture(hwnd, look_on);
            store::log(&format!("glass: left out of capture {look_on} ({})", if ok { "ok" } else { "refused" }));
            // Refused: no grabbing, or the island would grab itself.
            state.glass_excluded = Some(if ok { look_on } else { false });
        }
    }
    let can_grab = on && state.glass_excluded == Some(true);
    if !can_grab {
        state.glass_timer.stop();
        if state.mode == Mode::Hidden || !look_on {
            // A fresh picture when it shows again, not the old screen.
            state.glass_key.clear();
            state.glass_hash = 0;
            ui.set_has_glass(false);
        }
        return;
    }
    if !state.glass_timer.running() {
        state.glass_still = 0;
        glass_restart(ui, state, GLASS_FAST_MS);
        glass_tick(ui, state);
    } else if glass_shape(ui, state).1 != state.glass_key {
        // A new size: a frame now, not on the next tick.
        glass_tick(ui, state);
    }
}

/// Liquid Glass' frame rate while the screen behind changes (~10 fps), and
/// after a few unchanged frames.
const GLASS_FAST_MS: u64 = 100;
const GLASS_SLOW_MS: u64 = 250;

fn glass_restart(ui: &Island, state: &mut State, ms: u64) {
    let weak = ui.as_weak();
    state.glass_timer.start(TimerMode::Repeated, std::time::Duration::from_millis(ms), move || {
        if let Some(ui) = weak.upgrade() {
            with_state(|state| glass_tick(&ui, state));
        }
    });
}

/// The island's rectangle in physical window pixels, its radius (physical)
/// and the scale, and a key for that shape.
fn glass_shape(ui: &Island, state: &State) -> ((i32, i32, u32, u32, f32, f32), String) {
    let scale = ui.window().scale_factor().max(0.5);
    let (w, h) = state.size_to;
    let r = ui.get_target_r().min(w / 2.0).min(h / 2.0);
    let x = ((WINDOW.0 - w) / 2.0 * scale).round() as i32;
    let y = (TOP * scale).round() as i32;
    let (pw, ph) = ((w * scale).round() as u32, (h * scale).round() as u32);
    let key = format!("{pw}x{ph}|{r}|{scale}");
    ((x, y, pw, ph, r * scale, scale), key)
}

/// Asks the glass worker for a frame (one at a time).
fn glass_tick(ui: &Island, state: &mut State) {
    if state.glass_busy || state.mode == Mode::Hidden {
        return;
    }
    let ((x, y, w, h, radius, scale), key) = glass_shape(ui, state);
    if key != state.glass_key {
        ui.set_has_glass(false);
    }
    let pos = ui.window().position();
    state.glass_busy = true;
    let job = glass::Job { x: pos.x + x, y: pos.y + y, w, h, radius, scale, was_light: state.glass_light, last_hash: state.glass_hash, last_key: state.glass_key.clone(), key };
    let weak = ui.as_weak();
    glass::request(
        job,
        Box::new(move |done| {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    glass_done(&ui, done);
                }
            });
        }),
    );
}

fn glass_done(ui: &Island, done: glass::Done) {
    with_state(|state| {
        state.glass_busy = false;
        let wanted = glass_shape(ui, state).1;
        // Stale (the island changed size meanwhile, or went away): the next tick redoes it.
        if done.key != wanted || state.mode == Mode::Hidden || !state.glass_timer.running() {
            return;
        }
        match done.pixels {
            None => {
                state.glass_still += 1;
                if state.glass_still == 5 {
                    glass_restart(ui, state, GLASS_SLOW_MS);
                }
            }
            Some((px, w, h)) => {
                if state.glass_still >= 5 {
                    glass_restart(ui, state, GLASS_FAST_MS);
                }
                state.glass_still = 0;
                state.glass_hash = done.hash;
                state.glass_key = done.key;
                ui.set_glass(bg::image(&px, w, h));
                let s = ui.window().scale_factor().max(0.5);
                ui.set_glass_w(w as f32 / s);
                ui.set_glass_h(h as f32 / s);
                ui.set_has_glass(true);
                if done.light != state.glass_light {
                    state.glass_light = done.light;
                    apply_look(ui, state);
                }
            }
        }
    });
}

/// A snapshot's glass, over KANMURI_DEMO_BACKDROP: made here and now.
fn glass_demo_frame(ui: &Island, state: &mut State) {
    let ((x, y, w, h, radius, scale), key) = glass_shape(ui, state);
    if key == state.glass_key {
        return;
    }
    let win = ((WINDOW.0 * scale).round() as u32, (WINDOW.1 * scale).round() as u32);
    if let Some((px, light)) = glass::demo(win, x, y, w, h, radius, scale, state.glass_light) {
        state.glass_key = key;
        ui.set_glass(bg::image(&px, w, h));
                let s = ui.window().scale_factor().max(0.5);
                ui.set_glass_w(w as f32 / s);
                ui.set_glass_h(h as f32 / s);
        ui.set_has_glass(true);
        if light != state.glass_light {
            state.glass_light = light;
            apply_look(ui, state);
        }
    }
}

/// Reads the wallpaper's colour (Wallpaper color look) on a thread: at
/// start and when Windows says the wallpaper changed (WM_SETTINGCHANGE).
/// Nothing polls it. Read whatever the look, so Settings' Wallpaper card
/// shows the real colour.
fn load_wallpaper(ui: &Island) {
    // A snapshot has no event loop to come back to.
    if snapshot::requested().is_some() {
        let c = look::wallpaper_color();
        with_state(|s| s.wallpaper = c);
        return;
    }
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let c = look::wallpaper_color();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                let refill = with_state(|s| {
                    s.wallpaper = c;
                    s.settings_window.as_ref().map(|w| (w.as_weak(), s.settings.clone()))
                })
                .flatten();
                if let Some((w, settings)) = refill {
                    if let Some(w) = w.upgrade() {
                        fill_settings(&w, &settings);
                    }
                }
                refresh(&ui);
            }
        });
    });
}

fn wallpaper_changed() {
    if let Some(ui) = ISLAND.with(|i| i.borrow().as_ref().and_then(|w| w.upgrade())) {
        load_wallpaper(&ui);
    }
}

/// The background preview for Settings and the island's background, on a
/// thread (decoding a photo takes a moment); the island's at the size it is
/// springing to, with its corners.
fn update_background(ui: &Island) {
    let job = with_state(|state| {
        let look = &state.settings.look;
        if look.background.is_empty() {
            state.bg_key.clear();
            state.bg_image = None;
            state.bg_preview = None;
            return None;
        }
        let scale = ui.window().scale_factor().max(0.5);
        let (w, h) = state.size_to;
        let r = if state.mode == Mode::Expanded || (w, h) == COMPACT_MEDIA { MEDIA_RADIUS } else if (w, h) == COMPACT_NOTICE { NOTICE_RADIUS } else if (w, h) == COMPACT_QUESTION { QUESTION_RADIUS } else { h / 2.0 };
        // The picture is dimmed toward the island's body (the Look's colour).
        let tint = state.look_shown.as_ref().map_or((0x0b, 0x0b, 0x0d), |c| c.base);
        let key = format!("{}|{w}x{h}|{scale}|{}|{tint:?}", look.background, look.background_dim);
        if key == state.bg_key || state.mode == Mode::Hidden {
            return None;
        }
        state.bg_key = key.clone();
        let need_preview = state.bg_preview.is_none();
        // 1 px inside the edge (see the island's Image).
        Some((PathBuf::from(&look.background), key, ((w - 2.0) * scale).round() as u32, ((h - 2.0) * scale).round() as u32, (r - 1.0) * scale, look.background_dim, tint, need_preview))
    })
    .flatten();
    let Some((path, key, w, h, r, dim, tint, need_preview)) = job else { return };
    // A snapshot has no event loop to come back to: do it here and now.
    if snapshot::requested().is_some() {
        let island = bg::render_rgba(&path, w, h, r, dim, tint);
        let preview = if need_preview { bg::render_rgba(&path, 312, 120, 21.0, 0.0, tint) } else { None };
        with_state(|state| {
            state.bg_image = island.map(|(px, w, h)| bg::image(&px, w, h));
            state.bg_preview = preview.map(|(px, w, h)| bg::image(&px, w, h));
        });
        return;
    }
    let weak = ui.as_weak();
    let work = move || {
        let island = bg::render_rgba(&path, w, h, r, dim, tint);
        let preview = if need_preview { bg::render_rgba(&path, 312, 120, 21.0, 0.0, tint) } else { None };
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            let refill = with_state(|state| {
                if state.bg_key != key {
                    return None;
                }
                state.bg_image = island.map(|(px, w, h)| bg::image(&px, w, h));
                if let Some((px, w, h)) = preview {
                    state.bg_preview = Some(bg::image(&px, w, h));
                    return state.settings_window.as_ref().map(|w| (w.as_weak(), state.settings.clone()));
                }
                None
            })
            .flatten();
            if let Some((w, s)) = refill {
                if let Some(w) = w.upgrade() {
                    fill_settings(&w, &s);
                }
            }
            refresh(&ui);
        });
    };
    std::thread::spawn(work);
}

/// Settings > Accounts > Send setup: the accounts to pick from, the two
/// picked, the parts picked (all but the model at first), what "From" has.
fn fill_setup(win: &SettingsWindow) {
    let claude: Vec<accounts::Managed> = accounts::managed().into_iter().filter(|m| m.service == "claude").collect();
    let (from, to, parts, report) = with_state(|s| {
        if !claude.iter().any(|m| m.id == s.setup_from) {
            s.setup_from = claude.iter().find(|m| m.active).or(claude.first()).map(|m| m.id.clone()).unwrap_or_default();
        }
        if !claude.iter().any(|m| m.id == s.setup_to) || s.setup_to == s.setup_from {
            s.setup_to = claude.iter().find(|m| m.id != s.setup_from).map(|m| m.id.clone()).unwrap_or_default();
        }
        if s.setup_parts.is_none() {
            s.setup_parts = Some(setup::PARTS.iter().map(|p| p.0.to_string()).filter(|p| p != "model").collect());
        }
        (s.setup_from.clone(), s.setup_to.clone(), s.setup_parts.clone().unwrap_or_default(), s.setup_report.clone())
    })
    .unwrap_or_default();
    let have = accounts::claude_slot(&from).map(|slot| setup::summary(&slot)).unwrap_or_default();
    let chips: Vec<SetupPart> = setup::PARTS
        .iter()
        .enumerate()
        .map(|(i, (id, label))| SetupPart { id: (*id).into(), label: (*label).into(), on: parts.iter().any(|p| p == id), have: have.get(i).map(|h| h.1.as_str()).unwrap_or("").into() })
        .collect();
    win.set_setup_accounts(ModelRc::new(VecModel::from(claude.iter().map(|m| ScreenRow { id: m.id.as_str().into(), label: m.name.as_str().into() }).collect::<Vec<_>>())));
    win.set_setup_from(from.into());
    win.set_setup_to(to.into());
    win.set_setup_parts_a(ModelRc::new(VecModel::from(chips[..4].to_vec())));
    win.set_setup_parts_b(ModelRc::new(VecModel::from(chips[4..].to_vec())));
    win.set_setup_report(report.into());
    fill_mcp(win);
    fill_chats(win);
}

/// Settings > Accounts > MCP servers: the Send setup "From" account's servers.
fn fill_mcp(win: &SettingsWindow) {
    let (from, picked, report) = with_state(|s| (s.setup_from.clone(), s.mcp_picked.clone(), s.mcp_report.clone())).unwrap_or_default();
    let entries = accounts::claude_slot(&from).map(|slot| setup::mcp_entries(&slot)).unwrap_or_default();
    let rows: Vec<McpItem> = entries
        .iter()
        .map(|e| {
            let place = match e.scope.as_str() {
                "project" => format!("Project {}", e.project.rsplit(['\\', '/']).next().unwrap_or(&e.project)),
                "desktop" => "Claude Desktop".into(),
                _ => "Account".into(),
            };
            McpItem { id: e.id.as_str().into(), name: e.name.as_str().into(), meta: format!("{place} · {} · {}", e.kind, e.summary).into(), on: picked.contains(&e.id) }
        })
        .collect();
    win.set_mcp_picked(rows.iter().filter(|r| r.on).count() as i32);
    win.set_mcp_rows(ModelRc::new(VecModel::from(rows)));
    win.set_mcp_report(report.into());
}

fn ago(ms: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let m = now.saturating_sub(ms) / 60_000;
    match m {
        0 => "just now".into(),
        1..=59 => format!("{m} min ago"),
        60..=1439 => format!("{} h ago", m / 60),
        _ => format!("{} d ago", m / 1440),
    }
}

fn size_text(b: u64) -> String {
    if b >= 1_048_576 {
        format!("{:.1} MB", b as f64 / 1_048_576.0)
    } else {
        format!("{} KB", (b / 1024).max(1))
    }
}

/// Settings > Accounts > Chats: accounts with chats (Claude Code, Claude
/// Desktop), the list of the "From" one (read off the UI thread once per
/// account), folders open or shut, the picks.
fn fill_chats(win: &SettingsWindow) {
    let all: Vec<accounts::Managed> = accounts::managed().into_iter().filter(|m| m.service == "claude" || m.service == "desktop").collect();
    let label = |m: &accounts::Managed| m.name.clone();
    let (from, to, list, picked, open, report, loading) = with_state(|s| {
        if !all.iter().any(|m| m.id == s.chats_from) {
            s.chats_from = all.iter().find(|m| m.service == "claude" && m.active).or(all.first()).map(|m| m.id.clone()).unwrap_or_default();
        }
        let service = all.iter().find(|m| m.id == s.chats_from).map(|m| m.service.clone()).unwrap_or_default();
        if !all.iter().any(|m| m.id == s.chats_to && m.service == service && m.id != s.chats_from) {
            s.chats_to = all.iter().find(|m| m.service == service && m.id != s.chats_from).map(|m| m.id.clone()).unwrap_or_default();
        }
        (s.chats_from.clone(), s.chats_to.clone(), s.chats_list.clone(), s.chats_picked.clone(), s.chats_open.clone(), s.chats_report.clone(), s.chats_loading)
    })
    .unwrap_or_default();
    let service = all.iter().find(|m| m.id == from).map(|m| m.service.clone()).unwrap_or_default();
    win.set_chats_app(service.as_str().into());
    win.set_chats_from_choices(ModelRc::new(VecModel::from(all.iter().filter(|m| m.service == service).map(|m| ScreenRow { id: m.id.as_str().into(), label: label(m).into() }).collect::<Vec<_>>())));
    win.set_chats_to_choices(ModelRc::new(VecModel::from(all.iter().filter(|m| m.service == service && m.id != from).map(|m| ScreenRow { id: m.id.as_str().into(), label: label(m).into() }).collect::<Vec<_>>())));
    win.set_chats_from(from.as_str().into());
    win.set_chats_to(to.as_str().into());
    let mut rows: Vec<ChatItem> = Vec::new();
    let mut note = report;
    match list {
        Some((who, folders)) if who == from => {
            for f in &folders {
                let n = f.chats.iter().filter(|c| picked.contains(&c.id)).count();
                let is_open = open.contains(&f.path);
                rows.push(ChatItem {
                    id: f.path.as_str().into(),
                    header: true,
                    title: f.name.as_str().into(),
                    meta: format!("{} chat{}{}", f.chats.len(), if f.chats.len() == 1 { "" } else { "s" }, if n > 0 { format!(" · {n} picked") } else { String::new() }).into(),
                    on: n > 0 && n == f.chats.len(),
                    open: is_open,
                    can_resume: false,
                });
                if is_open {
                    for c in f.chats.iter().take(200) {
                        rows.push(ChatItem {
                            id: c.id.as_str().into(),
                            header: false,
                            title: c.title.as_str().into(),
                            meta: format!("{} · {}", ago(c.date), size_text(c.size)).into(),
                            on: picked.contains(&c.id),
                            open: false,
                            can_resume: service == "claude" && !c.sid.is_empty(),
                        });
                    }
                }
            }
            if folders.is_empty() && note.is_empty() {
                note = if service == "desktop" { "No Code-tab chats for this account yet (it needs to have signed in once).".into() } else { "No chats in this account yet.".into() };
            }
        }
        _ => {
            if note.is_empty() {
                note = "Reading chats…".into();
            }
            if snapshot_mode() && !from.is_empty() {
                // A snapshot can't wait for the reading thread.
                let folders = accounts::chat_home(&from).map(|h| chats::list(&h)).unwrap_or_default();
                with_state(|s| s.chats_list = Some((from.clone(), folders)));
                return fill_chats(win);
            }
            if !loading && !from.is_empty() {
                load_chats(win, from.clone());
            }
        }
    }
    win.set_chats_picked(picked.len() as i32);
    win.set_chats_rows(ModelRc::new(VecModel::from(rows)));
    win.set_chats_report(note.into());
}

/// Reads an account's chats off the UI thread (hundreds of files), then fills again.
fn load_chats(win: &SettingsWindow, id: String) {
    with_state(|s| s.chats_loading = true);
    let me = win.as_weak();
    std::thread::spawn(move || {
        let folders = accounts::chat_home(&id).map(|h| chats::list(&h)).unwrap_or_default();
        let _ = slint::invoke_from_event_loop(move || {
            with_state(|s| {
                s.chats_loading = false;
                if s.chats_from == id {
                    s.chats_list = Some((id.clone(), folders));
                }
            });
            if let Some(w) = me.upgrade() {
                fill_chats(&w);
            }
        });
    });
}

/// Settings > Accounts' three lists, read now, and a line after a failed change.
fn fill_accounts(win: &SettingsWindow, note: &str) {
    let all = accounts::managed();
    let rows = |service: &str| {
        let v: Vec<AccountSetting> = all
            .iter()
            .filter(|m| m.service == service)
            .map(|m| AccountSetting {
                id: m.id.as_str().into(),
                name: m.name.as_str().into(),
                signed_in_as: m.signed_in_as.as_str().into(),
                warning: m.warning.as_str().into(),
                active: m.active,
                is_default: m.is_default,
                can_sign_in: m.service != "desktop",
            })
            .collect();
        ModelRc::new(VecModel::from(v))
    };
    win.set_accounts_desktop(rows("desktop"));
    win.set_accounts_claude(rows("claude"));
    win.set_accounts_codex(rows("codex"));
    win.set_account_note(note.into());
    fill_setup(win);
}

fn open_settings(ui: &Island) {
    let existing = with_state(|state| state.settings_window.as_ref().map(|w| w.as_weak())).flatten();
    if let Some(win) = existing.and_then(|w| w.upgrade()) {
        let _ = win.show();
        return;
    }
    let Ok(win) = SettingsWindow::new() else { return };
    if let Some(s) = with_state(|state| state.settings.clone()) {
        fill_settings(&win, &s);
    }

    let island = ui.as_weak();
    win.on_module_toggled(move |id, on| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| {
                if let Some(m) = s.modules.iter_mut().find(|m| m.id == id.as_str()) {
                    m.enabled = on;
                }
            });
            // A module turned on or off starts or stops now, no restart.
            if id.as_str() == "notifications" {
                if on {
                    start_notifications(&ui);
                } else {
                    with_state(|state| state.notif_watcher = None);
                }
            }
            if id.as_str() == "music" {
                if on {
                    start_music(&ui);
                } else {
                    music::stop();
                    on_music(&ui, music::Snapshot::default());
                }
            }
        }
    });
    let island = ui.as_weak();
    win.on_module_moved(move |id, step| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| s.move_module(&id, step));
        }
    });
    let island = ui.as_weak();
    win.on_flag_changed(move |key, on| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| set_flag(s, &key, on));
            if key.starts_with("music.") {
                fetch_lyrics(&ui);
            }
        }
    });
    let island = ui.as_weak();
    win.on_number_changed(move |key, v| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| set_number(s, &key, v));
            if key.starts_with("look.") {
                with_state(|state| state.bg_preview = None);
                update_background(&ui);
            }
        }
    });
    let island = ui.as_weak();
    win.on_slot_picked(move |side, kind| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| {
                if side == "left" { s.general.idle_left = kind.to_string() } else { s.general.idle_right = kind.to_string() }
            });
            show_status(&ui);
        }
    });
    let island = ui.as_weak();
    win.on_screen_picked(move |id| {
        store::log(&format!("settings: screen picked {id}"));
        if let Some(ui) = island.upgrade() {
            // "auto:<mode>": Move automatically; else the home screen.
            match id.strip_prefix("auto:") {
                Some(m) => change_settings(&ui, |s| s.general.auto_screen = m.to_string()),
                // Picking a screen means "put it there": Follow me would take it straight back.
                None => change_settings(&ui, |s| {
                    s.general.screen = id.to_string();
                    if s.general.auto_screen == "follow" {
                        s.general.auto_screen = "off".into();
                    }
                }),
            }
            // Back home first; the next check moves it again if it should.
            AUTO_AT.lock().unwrap_or_else(|e| e.into_inner()).clear();
            recover_island(&ui);
            auto_screen_check();
        }
    });
    let island = ui.as_weak();
    win.on_place_picked(move |name| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| s.prayer.place = name.to_string());
        }
    });
    let island = ui.as_weak();
    win.on_theme_picked(move |theme| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| s.look.theme = theme.to_string());
            with_state(|state| state.bg_preview = None);
            update_background(&ui);
        }
    });
    let island = ui.as_weak();
    win.on_look_picked(move |style| {
        if let Some(ui) = island.upgrade() {
            // "tint:auto|dark|light": Liquid Glass' tint; else the look itself.
            match style.strip_prefix("tint:") {
                Some(t) => change_settings(&ui, |s| s.look.glass_tint = t.to_string()),
                None => change_settings(&ui, |s| s.look.style = style.to_string()),
            }
        }
    });
    let island = ui.as_weak();
    win.on_color_picked(move |text| {
        let Some(ui) = island.upgrade() else { return };
        // A colour that isn't one ("#12", "blue") changes nothing.
        if let Some(c) = island_core::look::parse_hex(&text) {
            change_settings(&ui, |s| {
                s.look.color = island_core::look::hex(c);
                s.look.style = "color".into();
            });
        }
    });
    let island = ui.as_weak();
    win.on_notif_app_toggled(move |name, on| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| {
                let off = &mut s.notifications.off;
                off.retain(|o| !o.eq_ignore_ascii_case(&name));
                if !on {
                    off.push(name.to_string());
                }
            });
        }
    });
    let island = ui.as_weak();
    win.on_apps_changed(move |text| {
        let apps: Vec<String> = text.split(',').map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect();
        if let Some(ui) = island.upgrade() {
            // Typing: save without refilling the field under the cursor.
            with_state(|state| {
                state.settings.notifications.apps = apps;
                let _ = store::save(&state.settings);
            });
            refresh(&ui);
        }
    });
    let island = ui.as_weak();
    let me_for_action = win.as_weak();
    win.on_action(move |what| {
        let Some(ui) = island.upgrade() else { return };
        match what.as_str() {
            // Settings > Accounts opened: read again (a sign-in may have finished).
            "accounts.refresh" => {
                if let Some(win) = me_for_action.upgrade() {
                    fill_accounts(&win, "");
                }
            }
            "background.pick" => {
                let island = ui.as_weak();
                std::thread::spawn(move || {
                    let Some(path) = bg::pick() else { return };
                    let _ = slint::invoke_from_event_loop(move || {
                        let Some(ui) = island.upgrade() else { return };
                        match bg::import(&path, &store::data_dir()) {
                            Ok(copy) => {
                                with_state(|state| {
                                    state.bg_preview = None;
                                    state.bg_key.clear();
                                });
                                change_settings(&ui, |s| s.look.background = copy.display().to_string());
                                update_background(&ui);
                            }
                            Err(e) => store::log(&format!("background: {e}")),
                        }
                    });
                });
            }
            "background.clear" => {
                change_settings(&ui, |s| s.look.background.clear());
                update_background(&ui);
            }
            "open-data-folder" => {
                let _ = std::process::Command::new("explorer.exe").arg(store::data_dir()).spawn();
            }
            _ => {}
        }
    });
    let island = ui.as_weak();
    win.on_claude_connect(move |install| {
        if let Some(ui) = island.upgrade() {
            ask_claude_hooks(&ui, install);
        }
    });
    // Settings > Accounts: read on open and after each change (files only, quick).
    fill_accounts(&win, "");
    let island = ui.as_weak();
    let me = win.as_weak();
    win.on_account_act(move |action, id, value| {
        let (Some(ui), Some(win)) = (island.upgrade(), me.upgrade()) else { return };
        let done = match action.as_str() {
            "rename" => accounts::rename(&id, &value),
            "remove" => accounts::remove(&id),
            "signin" => accounts::sign_in(&id),
            "terminal" => accounts::terminal(&id, None, None),
            "reveal" => accounts::reveal(&id),
            // `id` is the service here; Claude Code / Codex sign the new one in at once.
            "add" => accounts::add(&id, &value).and_then(|new_id| if id == "desktop" { Ok(()) } else { accounts::sign_in(&new_id) }),
            _ => Ok(()),
        };
        if let Err(e) = &done {
            store::log(&format!("accounts: {action} {id} failed: {e}"));
        }
        fill_accounts(&win, &done.err().map(|e| format!("Couldn't {action}: {e}")).unwrap_or_default());
        refresh(&ui);
    });
    let me = win.as_weak();
    win.on_mcp_act(move |action, id| {
        let Some(win) = me.upgrade() else { return };
        match action.as_str() {
            "pick" => {
                with_state(|s| {
                    s.mcp_report.clear();
                    match s.mcp_picked.iter().position(|p| *p == id.as_str()) {
                        Some(i) => {
                            s.mcp_picked.remove(i);
                        }
                        None => s.mcp_picked.push(id.to_string()),
                    }
                });
            }
            "send" => {
                let (from, to, picked) = with_state(|s| (s.setup_from.clone(), s.setup_to.clone(), s.mcp_picked.clone())).unwrap_or_default();
                let report = match (accounts::claude_slot(&from), accounts::claude_slot(&to)) {
                    (Some(f), Some(t)) if from != to => match setup::mcp_send(&picked, &f, &t) {
                        Ok((added, skipped)) => format!("Sent {added}{}.", if skipped > 0 { format!(", {skipped} already there") } else { String::new() }),
                        Err(e) => format!("Couldn't send: {e}"),
                    },
                    _ => "Pick two Claude Code accounts in Send setup.".to_string(),
                };
                store::log(&format!("mcp: {from} -> {to}: {report}"));
                with_state(|s| {
                    s.mcp_report = report;
                    s.mcp_picked.clear();
                });
            }
            _ => {}
        }
        fill_setup(&win);
    });
    let me = win.as_weak();
    win.on_chats_act(move |action, id| {
        let Some(win) = me.upgrade() else { return };
        match action.as_str() {
            "app" => {
                // The app's account in use (else its first) becomes "From".
                let pick = accounts::managed().into_iter().filter(|m| m.service == id.as_str()).max_by_key(|m| m.active).map(|m| m.id);
                with_state(|s| {
                    if let Some(p) = pick {
                        if s.chats_from != p {
                            s.chats_from = p;
                            s.chats_picked.clear();
                            s.chats_open.clear();
                            s.chats_list = None;
                            s.chats_report.clear();
                        }
                    }
                });
            }
            "from" => {
                with_state(|s| {
                    if s.chats_from != id.as_str() {
                        s.chats_from = id.to_string();
                        s.chats_picked.clear();
                        s.chats_open.clear();
                        s.chats_list = None;
                        s.chats_report.clear();
                    }
                });
            }
            "to" => {
                with_state(|s| {
                    s.chats_to = id.to_string();
                    s.chats_report.clear();
                });
            }
            "open" => {
                with_state(|s| match s.chats_open.iter().position(|p| *p == id.as_str()) {
                    Some(i) => {
                        s.chats_open.remove(i);
                    }
                    None => s.chats_open.push(id.to_string()),
                });
            }
            "pick" => {
                with_state(|s| {
                    s.chats_report.clear();
                    match s.chats_picked.iter().position(|p| *p == id.as_str()) {
                        Some(i) => {
                            s.chats_picked.remove(i);
                        }
                        None => s.chats_picked.push(id.to_string()),
                    }
                });
            }
            "pick-folder" => {
                with_state(|s| {
                    s.chats_report.clear();
                    let ids: Vec<String> = s.chats_list.as_ref().and_then(|(_, f)| f.iter().find(|f| f.path == id.as_str())).map(|f| f.chats.iter().map(|c| c.id.clone()).collect()).unwrap_or_default();
                    let all_on = ids.iter().all(|c| s.chats_picked.contains(c));
                    s.chats_picked.retain(|c| !ids.contains(c));
                    if !all_on {
                        s.chats_picked.extend(ids);
                    }
                });
            }
            "resume" => {
                let found = with_state(|s| {
                    s.chats_list.as_ref().and_then(|(_, f)| f.iter().flat_map(|f| f.chats.iter()).find(|c| c.id == id.as_str()).cloned()).map(|c| (s.chats_from.clone(), c))
                })
                .flatten();
                if let Some((acc, c)) = found {
                    if c.sid.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_') && !c.sid.is_empty() {
                        let r = accounts::terminal(&acc, Some(std::path::Path::new(&c.folder)), Some(&format!("claude --resume {}", c.sid)));
                        if let Err(e) = r {
                            with_state(|s| s.chats_report = format!("Couldn't resume: {e}"));
                        }
                    }
                }
            }
            "copy" => {
                let (from, to, picked) = with_state(|s| (s.chats_from.clone(), s.chats_to.clone(), s.chats_picked.clone())).unwrap_or_default();
                let report = match (accounts::chat_home(&from), accounts::chat_home(&to)) {
                    (Some(f), Some(t)) => match chats::copy(&picked, &f, &t) {
                        Ok((copied, skipped)) => format!("Copied {copied} chat{}{}.", if copied == 1 { "" } else { "s" }, if skipped > 0 { format!(", {skipped} skipped (already there)") } else { String::new() }),
                        Err(e) => format!("Couldn't copy: {e}"),
                    },
                    (_, None) => "The other account hasn't signed in yet.".to_string(),
                    _ => "Pick an account to copy from.".to_string(),
                };
                store::log(&format!("chats: {from} -> {to}: {report}"));
                with_state(|s| {
                    s.chats_report = report;
                    s.chats_picked.clear();
                });
            }
            _ => {}
        }
        fill_chats(&win);
    });
    let me = win.as_weak();
    win.on_setup_act(move |action, id| {
        let Some(win) = me.upgrade() else { return };
        match action.as_str() {
            "pick-from" => {
                with_state(|s| {
                    s.setup_from = id.to_string();
                    s.setup_report.clear();
                });
            }
            "pick-to" => {
                with_state(|s| {
                    s.setup_to = id.to_string();
                    s.setup_report.clear();
                });
            }
            "toggle" => {
                with_state(|s| {
                    let parts = s.setup_parts.get_or_insert_with(Vec::new);
                    if let Some(i) = parts.iter().position(|p| p == id.as_str()) {
                        parts.remove(i);
                    } else {
                        parts.push(id.to_string());
                    }
                });
            }
            "send" => {
                let (from, to, parts) = with_state(|s| (s.setup_from.clone(), s.setup_to.clone(), s.setup_parts.clone().unwrap_or_default())).unwrap_or_default();
                let report = match (accounts::claude_slot(&from), accounts::claude_slot(&to)) {
                    (Some(f), Some(t)) => {
                        let picked: Vec<&str> = parts.iter().map(|p| p.as_str()).collect();
                        match setup::send(&picked, &f, &t) {
                            Ok(lines) if lines.is_empty() => "Nothing picked to send.".to_string(),
                            Ok(lines) => format!("Sent: {}.", lines.join(", ")),
                            Err(e) => format!("Couldn't send: {e}"),
                        }
                    }
                    _ => "Pick two Claude Code accounts.".to_string(),
                };
                store::log(&format!("setup: {from} -> {to}: {report}"));
                with_state(|s| s.setup_report = report);
            }
            _ => {}
        }
        fill_setup(&win);
    });
    let _ = win.show();
    let weak = win.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(60), move || {
        if let Some(w) = weak.upgrade() {
            let again = w.as_weak();
            repaint_on_expose(w.window(), move || {
                if let Some(w) = again.upgrade() {
                    w.set_repaint(!w.get_repaint());
                }
            });
        }
    });
    with_state(|state| state.settings_window = Some(win));
    update_background(ui);
}

/// Shows what connecting (or disconnecting) Claude Code changes, and does it
/// only when the user says yes: dated backup, then the merged file.
fn ask_claude_hooks(ui: &Island, install: bool) {
    let current = match claude::read_settings() {
        Ok(v) => v,
        Err(e) => {
            store::log(&format!("claude hooks: {e}"));
            return;
        }
    };
    if install && claude::relay_command("x").is_none() {
        store::log("claude hooks: kanmuri-hook.exe is not next to the app");
        return;
    }
    let new = match claude::merged(&current, install, |e| claude::relay_command(e).unwrap_or_default()) {
        Ok(v) => v,
        Err(e) => {
            store::log(&format!("claude hooks: {e}"));
            return;
        }
    };
    let events: Vec<&str> = claude::EVENTS.iter().map(|(e, _)| *e).collect();
    let path = claude::settings_path().display().to_string();
    let message = if install {
        format!(
            "Adds Kanmuri's hooks for {} to:\n{path}\n\nA backup of the file is saved first (settings.json.bak-kanmuri-<date>). Your other settings and hooks, FubuNotch v1's included, stay as they are. If both v1 and v2 are running, both islands may ask for the same permission.",
            events.join(", ")
        )
    } else {
        format!("Removes Kanmuri's hooks from:\n{path}\n\nA backup is saved first. Nothing else in the file changes.")
    };
    let Ok(confirm) = ConfirmWindow::new() else { return };
    confirm.set_heading(if install { "Connect Claude Code?" } else { "Disconnect Claude Code?" }.into());
    confirm.set_message(message.into());
    confirm.set_confirm_label(if install { "Connect" } else { "Disconnect" }.into());
    let island = ui.as_weak();
    let weak_confirm = confirm.as_weak();
    confirm.on_answered(move |yes| {
        if yes {
            let stamp = local_time::stamp().replace([' ', ':', '-'], "");
            match claude::write_settings(&new, &stamp) {
                Ok(backup) => store::log(&format!("claude hooks {}: backup {backup}", if install { "connected" } else { "disconnected" })),
                Err(e) => store::log(&format!("claude hooks: {e}")),
            }
        }
        if let Some(c) = weak_confirm.upgrade() {
            let _ = c.hide();
        }
        // The Settings window shows the new state.
        if let Some(ui) = island.upgrade() {
            let refill = with_state(|state| state.settings_window.as_ref().map(|w| (w.as_weak(), state.settings.clone()))).flatten();
            if let Some((w, s)) = refill {
                if let Some(w) = w.upgrade() {
                    fill_settings(&w, &s);
                }
            }
            refresh(&ui);
        }
    });
    let _ = confirm.show();
    let weak = confirm.as_weak();
    Timer::single_shot(std::time::Duration::from_millis(60), move || {
        if let Some(c) = weak.upgrade() {
            let again = c.as_weak();
            repaint_on_expose(c.window(), move || {
                if let Some(c) = again.upgrade() {
                    c.set_repaint(!c.get_repaint());
                }
            });
        }
    });
    with_state(|state| state.confirm = Some(confirm));
}

// ── Start ───────────────────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Release builds abort on a panic with no window to say so: leave a line
    // in the log first, so a crash can be traced.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        store::log(&format!("panic: {info}"));
        default_hook(info);
    }));
    // A borderless tool window: no taskbar button, never takes focus, on top.
    // Software rendering: ~17 MB RAM against ~71 MB with an OpenGL context,
    // and the island is small enough that the CPU never notices.
    let snapshot = snapshot::requested();
    if snapshot.is_some() {
        snapshot::install()?;
    } else {
    slint::BackendSelector::new()
        .renderer_name("software".into())
        .with_winit_window_attributes_hook(|attrs| {
            use winit::platform::windows::WindowAttributesExtWindows;
            // Only the island is a borderless tool window; Settings is an ordinary one.
            // The first window made is the island (Settings and the confirm
            // window come later): a title check can't tell, it isn't set yet.
            if !ISLAND_MADE.swap(true, std::sync::atomic::Ordering::Relaxed) {
                attrs
                    .with_transparent(true)
                    .with_decorations(false)
                    .with_skip_taskbar(true)
                    .with_active(false)
                    .with_resizable(false)
                    .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
            } else {
                attrs
            }
        })
        .with_winit_custom_application_handler(shelf::DropWatch {
            notify: Box::new(|| {
                let _ = slint::invoke_from_event_loop(take_drops);
            }),
        })
        .select()?;
    }

    let ui = Island::new()?;
    ISLAND.with(|i| *i.borrow_mut() = Some(ui.as_weak()));
    island_lyrics::set_logger(store::log);
    let pinned = std::env::var_os("KANMURI_EXPANDED").is_some();
    let first_tab = std::env::var("KANMURI_TAB").unwrap_or_else(|_| "home".into());
    let mut settings = store::load();
    *SCREEN.lock().unwrap_or_else(|e| e.into_inner()) = settings.general.screen.clone();
    *AUTO_MODE.lock().unwrap_or_else(|e| e.into_inner()) = settings.general.auto_screen.clone();
    glass::TINT.store(match settings.look.glass_tint.as_str() { "dark" => 1, "light" => 2, _ => 0 }, std::sync::atomic::Ordering::Relaxed);
    // KANMURI_THEME: a theme for this run only (checks of each look).
    if let Ok(t) = std::env::var("KANMURI_THEME") {
        settings.look.theme = t;
    }
    // KANMURI_LOOK=<black|color|wallpaper|liquid>, KANMURI_LOOK_COLOR=rrggbb,
    // KANMURI_NIGHT=<0..100> (night warmth on, at that strength),
    // KANMURI_CLOSED_OPACITY=<20..100>: a Look for this run only (checks).
    if let Ok(v) = std::env::var("KANMURI_LOOK") {
        settings.look.style = v;
    }
    if let Ok(v) = std::env::var("KANMURI_LOOK_COLOR") {
        settings.look.color = format!("#{}", v.trim_start_matches('#'));
    }
    if let Some(v) = std::env::var("KANMURI_NIGHT").ok().and_then(|v| v.parse::<u32>().ok()) {
        settings.look.night_warm = true;
        settings.look.night_warmth = v.min(100);
    }
    if let Some(v) = std::env::var("KANMURI_CLOSED_OPACITY").ok().and_then(|v| v.parse::<u32>().ok()) {
        settings.look.closed_opacity = v;
    }
    // KANMURI_BG: a background picture for this run only.
    if let Ok(b) = std::env::var("KANMURI_BG") {
        settings.look.background = b;
    }
    let (focus_plan, water_plan) = (settings.focus.plan(), settings.water.plan());
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            arbiter: Arbiter::new(),
            started: Instant::now(),
            mode: if pinned { Mode::Expanded } else { Mode::Compact },
            hide_at: Some(hide_after(&settings)),
            last_front: None,
            pinned,
            held: false,
            line_held: false,
            music_events: 0,
            dnd: std::env::var_os("KANMURI_DND").is_some(),
            // KANMURI_DEMO_WATCH: a docked video, for snapshots.
            watching: std::env::var_os("KANMURI_DEMO_WATCH")
                .map(|_| Watching { hwnd: None, title: "Demo stream".into(), since: 0, placed: None }),
            watch_held: false,
            watch_hover: false,
            watch_freed: None,
            last_focus_running: false,
            line_notice_until: 0,
            last_shown: String::new(),
            life: mascot::Life::default(),
            life_timer: Timer::default(),
            glass_timer: Timer::default(),
            stats_timer: Timer::default(),
            stats_every: 0,
            glass_light: false,
            glass_hash: 0,
            glass_key: String::new(),
            glass_busy: false,
            glass_still: 0,
            glass_excluded: None,
            size_to: (0.0, 0.0),
            region_timer: Timer::default(),
            active_tab: first_tab,
            account_pending: String::new(),
            account_hint: (String::new(), String::new()),
            accounts_open: false,
            control: control::Snapshot::default(),
            control_note: String::new(),
            control_open: false,
            control_tx: None,
            weather: None,
            weather_busy: false,
            weather_failed: false,
            keep_awake: false,
            setup_from: String::new(),
            setup_to: String::new(),
            setup_parts: None,
            setup_report: String::new(),
            mcp_picked: Vec::new(),
            mcp_report: String::new(),
            chats_from: String::new(),
            chats_to: String::new(),
            chats_picked: Vec::new(),
            chats_open: Vec::new(),
            chats_list: None,
            chats_loading: false,
            chats_report: String::new(),
            settings,
            music: music::Snapshot::default(),
            music_art: None,
            lyric_width: 327.0,
            cover_big: 0.0,
            music_accent: None,
            lyrics_key: String::new(),
            cover_for: String::new(),
            lyrics: None,
            lyrics_model: None,
            ytm: None,
            lyrics_nudge: 0.0,
            notif_watcher: None,
            sensors: sensors::InUse::default(),
            flash_id: 4_000_000_000,
            notifications: VecDeque::new(),
            focus: Focus::new(&focus_plan),
            focus_plan,
            water_plan,
            water_reminded: None,
            focus_started_min: None,
            prayer_day: None,
            // From the minute before start: a pop-up due right now still comes.
            prayer_seen: {
                let (y, m, d, secs) = prayer_now();
                (format!("{y}-{m:02}-{d:02}"), (secs / 60) as i64 - 1)
            },
            data: store::load_data(),
            pending: HashMap::new(),
            ask_inputs: HashMap::new(),
            ask_seq: 0,
            logos: HashMap::new(),
            confirm: None,
            drop_hover: false,
            timer: Timer::default(),
            settings_window: None,
            bg_key: String::new(),
            bg_image: None,
            bg_preview: None,
            wallpaper: None,
            look_shown: None,
        });
    });
    load_wallpaper(&ui);

    let weak = ui.as_weak();
    ui.on_hover_changed(move |over| {
        let Some(ui) = weak.upgrade() else { return };
        let inside = false;
        let was_hidden = with_state(|state| {
            let was_hidden = state.mode == Mode::Hidden;
            let onto_video = !over && state.watching.as_ref().and_then(|w| w.hwnd).is_some_and(|h| watch::pointer_over(windows::Win32::Foundation::HWND(h as *mut core::ffi::c_void)));
            // A question from Claude Code stays compact under the pointer: its
            // Allow / Deny are in the capsule, and opening would move them away.
            let asking = state.mode == Mode::Compact && state.arbiter.view().front.is_some_and(|a| a.kind == Kind::Ask);
            if over && asking {
                state.hide_at = None;
            } else if over || inside {
                // Opening while Claude Code waits for an answer: its question
                // is at the top of the Notifications page.
                if state.mode != Mode::Expanded && state.arbiter.ranked().iter().any(|a| a.kind == Kind::Ask) {
                    state.active_tab = "notifications".into();
                }
                state.mode = Mode::Expanded;
                state.hide_at = None;
                state.watch_hover = false;
            } else if onto_video {
                state.watch_hover = true;
            } else if !state.pinned && !state.held && !state.drop_hover {
                state.mode = Mode::Compact;
                state.hide_at = Some(now_ms(state.started) + hide_after(&state.settings));
            }
            was_hidden && (over || inside)
        })
        .unwrap_or(false);
        // Coming back: the mascot says hello.
        if was_hidden {
            poke_mascot(&ui);
        }
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_mascot_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            poke_mascot(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_dismiss(move || {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            let now = now_ms(state.started);
            if let Some(id) = state.arbiter.view().front.map(|a| a.id.clone()) {
                state.arbiter.end(&id, now);
                // A dismissed permission request: Claude Code asks in its terminal.
                state.pending.remove(&id);
                state.ask_inputs.remove(&id);
            }
        });
        refresh(&ui);
    });

    let tx = start_control_worker(&ui);
    with_state(|state| state.control_tx = Some(tx));
    let weak = ui.as_weak();
    ui.on_control_volume(move |v| {
        if weak.upgrade().is_none() {
            return;
        }
        with_state(|state| {
            // Shown at once; the worker applies the latest and reads back.
            // Moving the slider of a muted output turns the sound back on.
            let was_muted = state.control.volume.is_some_and(|x| x.1);
            if let Some(vol) = state.control.volume.as_mut() {
                vol.0 = v;
                vol.1 = false;
            }
            control_send(&state.control_tx, &mut state.control, ControlJob::Do(control::Command::Volume(v)));
            if was_muted {
                control_send(&state.control_tx, &mut state.control, ControlJob::Do(control::Command::Mute(false)));
            }
        });
    });
    let weak = ui.as_weak();
    ui.on_control_brightness(move |v| {
        if weak.upgrade().is_none() {
            return;
        }
        with_state(|state| {
            state.control.brightness = Some((v * 100.0).round() as u32);
            control_send(&state.control_tx, &mut state.control, ControlJob::Do(control::Command::Brightness((v * 100.0).round() as u32)));
        });
    });
    let weak = ui.as_weak();
    ui.on_control_toggle(move |what| {
        let Some(ui) = weak.upgrade() else { return };
        if what == "keepawake" {
            let on = with_state(|state| {
                state.keep_awake = !state.keep_awake;
                state.keep_awake
            })
            .unwrap_or(false);
            set_keep_awake(on);
            refresh(&ui);
            return;
        }
        with_state(|state| {
            let c = &state.control;
            let cmd = match what.as_str() {
                "mute" => c.volume.map(|v| control::Command::Mute(!v.1)),
                "mic" => c.mic_muted.map(|m| control::Command::MicMute(!m)),
                "wifi" => c.wifi.map(|w| control::Command::Wifi(!w)),
                "bluetooth" => c.bluetooth.map(|b| control::Command::Bluetooth(!b)),
                "dark" => c.dark.map(|d| control::Command::DarkMode(!d)),
                "lock" => Some(control::Command::Lock),
                _ => None,
            };
            // Dark mode shows at once; the worker sets it and reads back.
            if let Some(control::Command::DarkMode(d)) = &cmd {
                state.control.dark = Some(*d);
            }
            if let Some(cmd) = cmd {
                control_send(&state.control_tx, &mut state.control, ControlJob::Do(cmd));
            }
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_control_weather_on(move || {
        let Some(ui) = weak.upgrade() else { return };
        change_settings(&ui, |s| {
            if let Some(m) = s.modules.iter_mut().find(|m| m.id == "weather") {
                m.enabled = true;
            }
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_control_device(move |kind, id| {
        if weak.upgrade().is_none() {
            return;
        }
        with_state(|state| {
            let cmd = if kind == "mic" { control::Command::Mic(id.to_string()) } else { control::Command::Output(id.to_string()) };
            control_send(&state.control_tx, &mut state.control, ControlJob::Do(cmd));
        });
    });

    let weak = ui.as_weak();
    ui.on_account_clicked(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        let id = id.to_string();
        let busy = with_state(|state| !state.account_pending.is_empty()).unwrap_or(false);
        if busy {
            return;
        }
        // Claude Desktop restarts on a switch: the first click only asks.
        let asked = with_state(|state| state.account_hint.0 == id && state.account_hint.1 == CONFIRM_DESKTOP).unwrap_or(false);
        if accounts::needs_confirm(&id) && !asked {
            with_state(|state| state.account_hint = (id.clone(), CONFIRM_DESKTOP.to_string()));
            refresh(&ui);
            let weak = ui.as_weak();
            Timer::single_shot(std::time::Duration::from_secs(5), move || {
                let Some(ui) = weak.upgrade() else { return };
                with_state(|state| {
                    if state.account_hint.1 == CONFIRM_DESKTOP {
                        state.account_hint = (String::new(), String::new());
                    }
                });
                refresh(&ui);
            });
            return;
        }
        with_state(|state| {
            state.account_pending = id.clone();
            state.account_hint = (String::new(), String::new());
        });
        refresh(&ui);
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let result = accounts::switch(&id);
            if let Err(e) = &result {
                store::log(&format!("accounts: switch {id} failed: {e}"));
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let line = match result {
                    Ok(m) => m,
                    Err(e) => format!("Couldn't switch: {e}"),
                };
                with_state(|state| {
                    state.account_pending.clear();
                    state.account_hint = (id.clone(), line);
                });
                refresh(&ui);
                let weak = ui.as_weak();
                Timer::single_shot(std::time::Duration::from_secs(6), move || {
                    let Some(ui) = weak.upgrade() else { return };
                    with_state(|state| {
                        if state.account_hint.0 == id {
                            state.account_hint = (String::new(), String::new());
                        }
                    });
                    refresh(&ui);
                });
            });
        });
    });

    let weak = ui.as_weak();
    ui.on_tab_clicked(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| state.active_tab = id.to_string());
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_watch_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            with_state(|state| state.watch_held = !state.watch_held);
            refresh(&ui);
        }
    });
    ui.on_watch_back(watch_back);

    // Any browser PiP docks itself: Windows reports windows appearing and
    // going, nothing is polled. One may be open already.
    let weak = ui.as_weak();
    watch::hook(move || {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                pip_check(&ui);
            }
        });
    });
    pip_check(&ui);

    let weak = ui.as_weak();
    ui.on_dnd_clicked(move || {
        if let Some(ui) = weak.upgrade() {
            with_state(|state| state.dnd = !state.dnd);
            refresh(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_open_settings(move || {
        if let Some(ui) = weak.upgrade() {
            open_settings(&ui);
        }
    });

    let weak = ui.as_weak();
    ui.on_music_command(move |command| {
        let Some(ui) = weak.upgrade() else { return };
        // Shuffle and repeat go to the extension when Windows can't reach them.
        if matches!(command.as_str(), "shuffle" | "repeat") {
            let via_extension = with_state(|state| {
                let now = now_ms(state.started);
                let can = if command.as_str() == "shuffle" { state.music.shuffle.is_none() } else { state.music.repeat.is_none() };
                can && ytm_live(state, now).is_some()
            })
            .unwrap_or(false);
            if via_extension {
                island_webhook::ytm_command(&command);
                return;
            }
        }
        // "seek:<0..1>": a click or drag on the progress bar, let go.
        if let Some(f) = command.strip_prefix("seek:").and_then(|f| f.parse::<f64>().ok()) {
            with_state(|state| {
                let position = state.music.position_now();
                let lyric_len = state.lyrics.as_ref().map_or(0.0, |l| l.duration);
                let length = song_length(&state.music, position, lyric_len);
                if let Some(at) = seek_target(f, length, state.music.can_seek) {
                    music::seek(at);
                    // Shown there at once; the player's own word follows.
                    state.music.position = at;
                    state.music.at_unix_ms = music::now_unix_ms();
                }
            });
            return refresh(&ui);
        }
        if command.as_str() == "hold" {
            with_state(|state| {
                state.held = !state.held;
                state.line_held &= !state.held;
            });
            return refresh(&ui);
        }
        if command.as_str() == "line-hold" {
            with_state(|state| {
                state.line_held = !state.line_held;
                state.held &= !state.line_held;
            });
            return refresh(&ui);
        }
        // Like/dislike stay on this PC; a dislike also skips, as in YouTube Music.
        let vote = match command.as_str() {
            "like" => 1,
            "dislike" => -1,
            _ => return music::command(&command),
        };
        let skip = with_state(|state| {
            if !state.music.has_session {
                return false;
            }
            let key = song_key(&state.music);
            let skip = if state.data.ratings.get(&key) == Some(&vote) {
                state.data.ratings.remove(&key);
                false
            } else {
                state.data.ratings.insert(key, vote);
                vote < 0
            };
            store::save_data(&state.data);
            skip
        });
        if skip == Some(true) {
            music::command("next");
        }
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_lyrics_nudge(move |d| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| state.lyrics_nudge = (state.lyrics_nudge + d as f64).clamp(-10.0, 10.0));
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_shelf_open(move |i| {
        if weak.upgrade().is_none() {
            return;
        }
        with_state(|state| {
            if let Some(f) = state.data.shelf.get(i as usize) {
                shelf::open(&f.path);
            }
        });
    });
    ui.on_shelf_copy(move |i| {
        with_state(|state| {
            if let Some(f) = state.data.shelf.get(i as usize) {
                if !shelf::copy(&f.path) {
                    store::log(&format!("shelf: could not copy {}", f.path));
                }
            }
        });
    });
    let weak = ui.as_weak();
    ui.on_shelf_remove(move |i| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            if (i as usize) < state.data.shelf.len() {
                state.data.shelf.remove(i as usize);
                store::save_data(&state.data);
            }
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_shelf_clear(move || {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.shelf.clear();
            store::save_data(&state.data);
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_action_clicked(move |action| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            let now = now_ms(state.started);
            let Some(id) = state.arbiter.view().front.map(|a| a.id.clone()) else { return };
            if id == "water:remind" {
                state.active_tab = "water".into();
                return;
            }
            answer(state, &id, &action, now);
        });
        refresh(&ui);
    });

    // A question answered from its row on the Notifications page.
    let weak = ui.as_weak();
    ui.on_ask_answered(move |id, action| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            let now = now_ms(state.started);
            if state.arbiter.ranked().iter().any(|a| a.id == id.as_str() && a.kind == Kind::Ask) {
                answer(state, &id, &action, now);
            }
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_focus_command(move |c| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            let now = now_ms(state.started);
            let plan = state.focus_plan;
            match c.as_str() {
                "start" => {
                    if state.focus.phase == daily::Phase::Focus && state.focus_started_min.is_none() {
                        state.focus_started_min = Some(local_time::minute_of_day());
                    }
                    state.focus.start(now)
                }
                "pause" => state.focus.pause(now),
                "skip" => {
                    log_focus(state, now, false);
                    state.focus.skip(&plan)
                }
                "reset" => {
                    log_focus(state, now, false);
                    state.focus_started_min = None;
                    state.focus.reset(&plan)
                }
                _ => {}
            }
            focus_activity(state, now);
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_water_command(move |c| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.water.for_date(&local_time::today());
            match c.as_str() {
                "drink" => {
                    let glass = state.water_plan.glass_ml;
                    state.data.water.drink(glass, local_time::minute_of_day());
                    let now = now_ms(state.started);
                    state.arbiter.end("water:remind", now);
                }
                "undo" => state.data.water.undo(),
                _ => {}
            }
            store::save_data(&state.data);
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_todo_add(move |text| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            if state.data.todo.add(&text).is_some() {
                store::save_data(&state.data);
            }
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_todo_toggle(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.todo.toggle(id as u64, &stamp_now());
            store::save_data(&state.data);
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_todo_move(move |id, to| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.todo.move_open(id as u64, to.max(0) as usize);
            store::save_data(&state.data);
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_todo_clear_done(move || {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.todo.clear_done(&stamp_now());
            store::save_data(&state.data);
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_todo_remove(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.todo.remove(id as u64, &stamp_now());
            store::save_data(&state.data);
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_front_clicked(move || {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            let now = now_ms(state.started);
            let Some(id) = state.arbiter.view().front.map(|a| a.id.clone()) else { return };
            if let Some(n) = id.strip_prefix("notif:").and_then(|x| x.parse::<u32>().ok()).and_then(|x| state.notifications.iter().find(|n| n.id == x)) {
                notif::open(n);
                state.arbiter.end(&id, now);
            }
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_notif_clear(move || {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| state.notifications.clear());
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_lyric_text_width(move |w| {
        let Some(ui) = weak.upgrade() else { return };
        let changed = with_state(|state| {
            let c = w > 50.0 && (state.lyric_width - w).abs() > 0.5;
            if c {
                state.lyric_width = w;
            }
            c
        })
        .unwrap_or(false);
        if changed {
            // The lines are made again for the new width, on the next turn (not inside Slint's evaluation).
            let weak = ui.as_weak();
            Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(ui) = weak.upgrade() {
                    refresh(&ui);
                }
            });
        }
    });

    let weak = ui.as_weak();
    ui.on_music_cover_size(move |s| {
        let Some(ui) = weak.upgrade() else { return };
        let changed = with_state(|state| {
            let c = (state.cover_big - s).abs() > 0.01;
            state.cover_big = s;
            c
        })
        .unwrap_or(false);
        if changed {
            // Not from inside Slint's own property evaluation: on the next turn.
            let weak = ui.as_weak();
            Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(ui) = weak.upgrade() {
                    refresh(&ui);
                }
            });
        }
    });

    let weak = ui.as_weak();
    ui.on_notif_clicked(move |index| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            if let Some(n) = state.notifications.get(index as usize) {
                notif::open(n);
            }
        });
        refresh(&ui);
    });

    // The System module's watchers cost nothing while nothing changes; its
    // switches are read per event, so turning one off needs no restart.
    show_status(&ui);
    let weak = ui.as_weak();
    system::start(move |e| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                on_system(&ui, e);
            }
        });
    });

    // Apps that send no Windows notification (WeChat) and the microphone /
    // camera: both wait on Windows (a shell hook, a registry notification).
    if !snapshot_mode() {
        let weak = ui.as_weak();
        flash::start(move |f| {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    on_flash(&ui, f);
                }
            });
        });
        let weak = ui.as_weak();
        sensors::start(move |now| {
            let weak = weak.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    with_state(|state| state.sensors = now);
                    refresh(&ui);
                }
            });
        });
    }
    if with_state(|s| s.settings.is_on("notifications")).unwrap_or(false) {
        *NOTIF_APPS.lock().unwrap_or_else(|e| e.into_inner()) = with_state(|s| s.settings.notifications.filter()).unwrap_or_default();
        start_notifications(&ui);
    }
    // The Claude app's logo for Claude Code's questions and notices: read once,
    // off the UI thread (a notification from an app brings its own).
    if !snapshot_mode() {
        let weak = ui.as_weak();
        notif::preload(vec![notif::CLAUDE_AUMID.into()], move || {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    refresh(&ui);
                }
            });
        });
    }
    // A newly installed app gets a key from Windows: its banners follow too.
    if !snapshot_mode() {
        banners::watch(banners::SETTINGS, || {
            let _ = slint::invoke_from_event_loop(|| {
                with_state(sync_banners);
            });
        });
    }

    if std::env::var_os("KANMURI_DEMO_WATER").is_some() {
        // A day with some glasses, in memory only.
        with_state(|state| {
            let mut day = island_core::daily::WaterDay { date: local_time::today(), ..Default::default() };
            for m in [9 * 60 + 10, 10 * 60 + 40, 11 * 60 + 5, 13 * 60 + 30, 15 * 60] {
                day.drink(250, m);
            }
            state.data.water = day;
        });
    }
    if std::env::var_os("KANMURI_DEMO_FOCUS").is_some() {
        // Today's rounds, in memory only.
        with_state(|state| {
            let today = local_time::today();
            state.data.focus_log = [(540, 565, 1500, true), (575, 600, 1500, true), (610, 628, 1080, false)]
                .into_iter()
                .map(|(from_min, to_min, focused_s, full)| daily::FocusRound { date: today.clone(), from_min, to_min, focused_s, full })
                .collect();
        });
    }
    if std::env::var_os("KANMURI_DEMO_TODO").is_some() {
        // A long list, in memory only (never saved), for checks of the To-do page.
        with_state(|state| {
            let mut todo = island_core::daily::Todo::default();
            let names = [
                "Reply to the design review", "Water the plants", "Book the dentist", "Send the weekly report",
                "Review the pull request", "Buy cat food", "Call mum", "Back up the photos",
                "Plan the weekend trip", "Return the library books", "Fix the bike light", "Read chapter 4",
            ];
            let ids: Vec<u64> = names.iter().filter_map(|n| todo.add(n)).collect();
            for i in [1, 4, 7] {
                todo.toggle(ids[i], &stamp_now());
            }
            // KANMURI_DEMO_TODO=log: some cleared ones too, over three days.
            if std::env::var("KANMURI_DEMO_TODO").is_ok_and(|v| v == "log") {
                let today = local_time::today();
                for (date, min, text) in [
                    ("2026-11-08", 600, "Order the poster prints"),
                    (today.as_str(), 545, "Check the equipment list"),
                    (today.as_str(), 610, "Buy the brochure paper, the thick one the print shop asked for last time"),
                ] {
                    todo.log.push(island_core::daily::LoggedTask { text: text.into(), done: island_core::daily::Stamp { date: date.into(), min } });
                }
            }
            state.data.todo = todo;
        });
        ui.set_todo_show_log(std::env::var_os("KANMURI_TODO_LOG").is_some());
    }
    if std::env::var_os("KANMURI_DEMO_NOTIFS").is_some() {
        // A few notifications, for checks of the page without real ones.
        with_state(|state| {
            // Newest first, more than the list shows (its header counts them).
            for (id, app, title, body, time) in [
                (6, "Claude", "Build finished", "All 214 tests passed on the release branch", "10:28"),
                (5, "Discord", "#general", "The build is green, shipping tonight", "10:27"),
                (4, "WhatsApp", "Mika", "See you at seven? I'll bring the prints", "10:24"),
                (3, "Claude", "Waiting for you", "Approve the migration before it runs", "10:22"),
                (2, "Discord", "Ren", "Pushed the fix, have a look when you can", "10:12"),
                (1, "WhatsApp", "Mum", "Call me later", "09:58"),
            ] {
                let aumid = DEMO_APPS.iter().find(|(a, _)| *a == app).map_or("", |(_, id)| id);
                state.notifications.push_back(notif::Notif { id, app: app.into(), aumid: aumid.into(), title: title.into(), body: body.into(), time: time.into() });
            }
            // the newest one up on the capsule, as a real one would be
            upsert_json(state, serde_json::json!({ "id": "notif:6", "source": "Claude", "kind": "alert",
                "title": "Build finished", "body": "All 214 tests passed on the release branch, ready to ship" }), 0);
        });
    }
    // KANMURI_DEMO_MIC=1|mic|cam|both: an app on the microphone (1 / mic),
    // the camera, or both: the capsule's dots.
    if let Ok(v) = std::env::var("KANMURI_DEMO_MIC") {
        with_state(|state| {
            let (mic, cam) = match v.as_str() {
                "cam" => (false, true),
                "both" => (true, true),
                _ => (true, false),
            };
            state.sensors = sensors::InUse {
                mic: if mic { vec!["Zoom".into()] } else { vec![] },
                cam: if cam { vec!["Zoom".into()] } else { vec![] },
            };
        });
    }
    // KANMURI_DEMO_WECHAT=1: a WeChat message (its taskbar button flashed),
    // with WeChat's own icon when it is installed, through the same path as a
    // real one.
    if std::env::var_os("KANMURI_DEMO_WECHAT").is_some() {
        let exe = flash::installed_wechat().unwrap_or_default();
        let key = flash::key(&exe, "WeChat");
        if !exe.is_empty() {
            let (k, e) = (key.clone(), exe.clone());
            let _ = std::thread::spawn(move || {
                let com = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
                notif::keep_file_logo(&k, &e);
                if com.is_ok() {
                    unsafe { windows::Win32::System::Com::CoUninitialize() };
                }
            })
            .join();
        }
        if notif::logo(&key).is_none() {
            notif::put_logo(&key, demo_logo(accent_of("WeChat", Kind::Alert)));
        }
        on_flash(&ui, flash::Flash { app: "WeChat".into(), exe });
    }
    // KANMURI_DEMO_WEATHER=1: the Control page's weather, made up (no network).
    if std::env::var_os("KANMURI_DEMO_WEATHER").is_some() {
        with_state(|state| {
            if let Some(m) = state.settings.modules.iter_mut().find(|m| m.id == "weather") {
                m.enabled = true;
            }
            state.weather = Some((weather::Weather { now: 29.4, high: 32.1, low: 24.8, code: 2, day: true }, 0));
        });
    }
    if std::env::var_os("KANMURI_DEMO_ASK").is_some() {
        // A question from Claude Code waiting for Allow / Deny (nobody gets the answer);
        // KANMURI_DEMO_ASK=question: a multiple-choice AskUserQuestion instead.
        let question = std::env::var("KANMURI_DEMO_ASK").is_ok_and(|v| v == "question");
        with_state(|state| {
            let p = if question {
                serde_json::json!({ "session_id": "demo", "cwd": "C:\\Codingers\\kanmuri", "tool_name": "AskUserQuestion",
                    "tool_input": { "questions": [{ "question": "Where should the build run?", "header": "Build in", "multiSelect": false,
                        "options": [{ "label": "Install here", "description": "" }, { "label": "Office PC", "description": "" }, { "label": "NotchDeck first", "description": "" }] }] } })
            } else {
                serde_json::json!({ "session_id": "demo", "cwd": "C:\\Codingers\\kanmuri", "tool_name": "Bash",
                    "tool_input": { "command": "cargo build --workspace && cargo test --workspace" } })
            };
            if let claude::Plan::Show(list) = claude::plan("PermissionRequest", &p, "claude:demo:ask:1") {
                for json in list {
                    upsert_json(state, json, 0);
                }
            }
        });
    }
    if std::env::var_os("KANMURI_DEMO_NOTIFS").is_some() || std::env::var_os("KANMURI_DEMO_ASK").is_some() {
        // The demo apps' logos: the real ones where Windows has the app, else a
        // made-up tile in the app's colour (snapshots wait for them).
        let ids: Vec<String> = DEMO_APPS.iter().map(|(_, id)| id.to_string()).collect();
        if let Some(t) = notif::preload(ids, || {}) {
            let _ = t.join();
        }
        for (app, id) in DEMO_APPS {
            if notif::logo(id).is_none() {
                notif::put_logo(id, demo_logo(accent_of(app, Kind::Alert)));
            }
        }
    }
    // KANMURI_DEMO_VOLUME=<0..100>: a player volume, to see the music
    // card's volume row (Kanmuri doesn't read the player's volume yet).
    if let Some(v) = std::env::var("KANMURI_DEMO_VOLUME").ok().and_then(|v| v.parse::<f32>().ok()) {
        ui.set_volume((v / 100.0).clamp(0.0, 1.0));
    }
    if std::env::var_os("KANMURI_DEMO_MUSIC").is_some() {
        // A song with lyrics, for checks of the Music page without touching the player.
        with_state(|state| {
            state.music = music::Snapshot {
                has_session: true,
                title: "FOXXXROOM".into(),
                artist: "Shirakami Fubuki".into(),
                app: "demo".into(),
                playing: true,
                // KANMURI_DEMO_POS: seconds in (11 by default), to see the wave grown.
                position: std::env::var("KANMURI_DEMO_POS").ok().and_then(|v| v.parse().ok()).unwrap_or(11.0),
                duration: 207.0,
                at_unix_ms: music::now_unix_ms(),
                shuffle: Some(false),
                repeat: Some(music::Repeat::All),
                can_seek: true,
                ..Default::default()
            };
            // Now playing, as a real song would be (the capsule's song pill).
            upsert_json(state, serde_json::json!({ "id": "music:now", "source": "demo", "kind": "ambient",
                "title": format!("♪ {}", state.music.title), "body": state.music.artist }), 0);
            // A made-up cover in one colour (KANMURI_DEMO_ART=rrggbb, pink by default):
            // a diagonal shade and a pale disc, enough to check the island taking its colour.
            let hex = std::env::var("KANMURI_DEMO_ART").unwrap_or_else(|_| "ff5fa2".into());
            let c = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0xff5fa2);
            let (cr, cg, cb) = ((c >> 16) as f32, ((c >> 8) & 0xff) as f32, (c & 0xff) as f32);
            let n = music::ART_PX;
            let mut art = Vec::with_capacity((n * n * 4) as usize);
            for y in 0..n {
                for x in 0..n {
                    let shade = 0.45 + 0.55 * (x + y) as f32 / (2 * n) as f32;
                    let (dx, dy) = (x as f32 - n as f32 * 0.62, y as f32 - n as f32 * 0.38);
                    let disc = if dx * dx + dy * dy < (n as f32 * 0.2).powi(2) { 0.55 } else { 0.0 };
                    let mix = |v: f32| (v * shade * (1.0 - disc) + 255.0 * disc).min(255.0) as u8;
                    art.extend([mix(cr), mix(cg), mix(cb), 255]);
                }
            }
            state.music_accent = accent::of_rgba(&art);
            let mut rounded = art.clone();
            round_cover(&mut rounded, n);
            state.music_art = Some(Art::new(rounded));
            state.music.art = Some(art);
            // =nolyrics: the song without; KANMURI_LINE: Line on.
            state.line_held = std::env::var_os("KANMURI_LINE").is_some();
            if std::env::var("KANMURI_DEMO_MUSIC").is_ok_and(|v| v == "nolyrics") {
                state.lyrics = Some(island_lyrics::Lyrics { status: island_lyrics::Status::None, lines: vec![], duration: 0.0 });
                return;
            }
            // =ja: the lines in Japanese (the lyrics card then offers あ and Ro).
            let latin = ["koko wa kitto yume no naka ne", "", "mezameta no wa uso mitai da, hontou no kimi wa doko ni iru no ka mada wakaranai mama", "kimi no koe ga kikoeru", "tooku made"];
            let japanese = ["ここはきっと夢の中ね", "", "目覚めたのは嘘みたいだ", "君の声が聞こえる", "遠くまで"];
            let words = if std::env::var("KANMURI_DEMO_MUSIC").is_ok_and(|v| v == "ja") { japanese } else { latin };
            state.lyrics = Some(island_lyrics::Lyrics {
                status: island_lyrics::Status::Found,
                lines: words
                    .iter()
                    .zip([0.25, 4.78, 10.0, 14.5, 18.0])
                    .map(|(t, s)| island_lyrics::Line {
                        time: s,
                        text: t.to_string(),
                        romaji: if island_lyrics::romaji::is_japanese(t) { island_lyrics::romaji::line_to_romaji(t) } else { String::new() },
                        ruby: if island_lyrics::romaji::has_kanji(t) { island_lyrics::romaji::line_to_ruby(t) } else { Vec::new() },
                    })
                    .collect(),
                duration: 0.0,
            });
        });
    } else if with_state(|s| s.settings.is_on("music")).unwrap_or(false) {
        with_state(|s| s.line_held = std::env::var_os("KANMURI_LINE").is_some());
        start_music(&ui);
    }

    // Tray: Settings and Quit.
    // KANMURI_SNAPSHOT: draw the island (or Settings, with
    // KANMURI_SETTINGS) into a PNG and stop; nothing is shown.
    if let Some(path) = snapshot {
        // The watcher threads (notifications, music) report in real time.
        std::thread::sleep(std::time::Duration::from_millis(2000));
        // The first refresh ran before the demo data: a tab that only exists
        // with it (Lyrics) fell back to the first one. Ask for it again.
        if let Ok(tab) = std::env::var("KANMURI_TAB") {
            with_state(|s| s.active_tab = tab);
        }
        refresh(&ui);
        snapshot::advance(1500);
        refresh(&ui);
        snapshot::advance(1500);
        if std::env::var_os("KANMURI_SETTINGS").is_some() {
            open_settings(&ui);
            if let Some(page) = std::env::var("KANMURI_SETTINGS").ok().filter(|p| p.len() > 1) {
                if let Some(w) = with_state(|s| s.settings_window.as_ref().map(|w| w.as_weak())).flatten().and_then(|w| w.upgrade()) {
                    w.set_page(page.into());
                }
            }
            snapshot::advance(4000);
            let size = with_state(|s| s.settings_window.as_ref().map(|w| {
                let p = w.window().size();
                (p.width as f32 / snapshot::scale(), p.height as f32 / snapshot::scale())
            }))
            .flatten()
            .filter(|s| s.0 > 10.0)
            .unwrap_or((900.0, 600.0));
            // KANMURI_SETTINGS_HEIGHT=<px>: a taller window, for a long page.
            let size = match std::env::var("KANMURI_SETTINGS_HEIGHT").ok().and_then(|v| v.parse::<f32>().ok()) {
                Some(h) => (size.0, h.clamp(300.0, 3000.0)),
                None => size,
            };
            snapshot::write(1, size, &path)?;
        } else {
            snapshot::write(0, WINDOW, &path)?;
        }
        return Ok(());
    }

    let tray = Tray::new()?;
    let weak = ui.as_weak();
    tray.on_open_settings(move || {
        if let Some(ui) = weak.upgrade() {
            open_settings(&ui);
        }
    });
    tray.on_quit(|| {
        // Closed, Kanmuri can't show notifications: Windows' banners come
        // back as they were (and go again at the next start).
        with_state(|state| {
            if banners::give_back(banners::SETTINGS, &mut state.data.banners, |_| true) {
                store::save_data(&state.data);
            }
            // A docked video comes back in sight: nothing would show it.
            if let Some(h) = state.watching.as_ref().and_then(|w| w.hwnd).filter(|h| watch::alive(as_hwnd(*h))) {
                watch::release(as_hwnd(h));
            }
        });
        let _ = slint::quit_event_loop();
    });

    let token = island_webhook::load_or_create_token(&store::data_dir())?;
    let weak = ui.as_weak();
    let served = island_webhook::spawn(island_webhook::PORT, token.clone(), move |event| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply(&ui, event);
            }
        });
    });
    if let Err(err) = served {
        // The port is taken: Kanmuri is already running (its island hidden,
        // moved off screen, or under something). Bring that one back rather
        // than quit without a word.
        let shown = island_webhook::ask_to_show(island_webhook::PORT, &token);
        store::log(&format!("start: port {} taken ({err}); running copy {}", island_webhook::PORT, if shown { "brought back" } else { "did not answer" }));
        if shown {
            return Ok(());
        }
        return Err(err.into());
    }

    place(&ui);
    ui.show()?;
    // No taskbar button, whatever the window attributes did: the tray icon is the app's.
    ui.window().with_winit_window(|w: &winit::window::Window| {
        use winit::platform::windows::WindowExtWindows;
        w.set_skip_taskbar(true);
    });
    tray.show()?;
    refresh(&ui);
    // KANMURI_CYCLE switches tabs on its own (checks of tab changes without the mouse).
    if std::env::var_os("KANMURI_CYCLE").is_some() {
        let weak = ui.as_weak();
        let n = std::cell::Cell::new(0usize);
        let cycle = Timer::default();
        cycle.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(900), move || {
            const TABS: [&str; 4] = ["notifications", "music", "lyrics", "accounts"];
            if let Some(ui) = weak.upgrade() {
                // A click activates the island: do the same.
                ui.window().with_winit_window(|w: &winit::window::Window| w.focus_window());
                ui.invoke_tab_clicked(TABS[n.get() % TABS.len()].into());
                n.set(n.get() + 1);
            }
        });
        std::mem::forget(cycle);
    }
    // The native window only exists once the event loop runs: a position set
    // before that is lost, so place the island again right after.
    let weak = ui.as_weak();
    // KANMURI_SETTINGS opens Settings at start (checks without the mouse).
    let settings_at_start = std::env::var_os("KANMURI_SETTINGS").is_some();
    Timer::single_shot(std::time::Duration::from_millis(80), move || {
        if let Some(ui) = weak.upgrade() {
            place(&ui);
            // Before the first morph: the island is a popup from the start.
            if let Some(hwnd) = island_hwnd(&ui) {
                popup_style(hwnd);
                guard_on_top(hwnd);
            }
            // Move automatically starts checking (when it is on).
            auto_screen_check();
            refresh(&ui);
            if settings_at_start {
                open_settings(&ui);
            }
        }
    });
    // The island is hidden from the taskbar, so closing Settings must not end the app.
    slint::run_event_loop_until_quit()?;
    Ok(())
}

#[cfg(test)]
mod notif_view_tests {
    use super::*;

    #[test]
    fn one_app_gets_two_rows_at_most_and_the_order_stays() {
        let mk = |id: u32, app: &str| notif::Notif { id, app: app.into(), aumid: format!("{app}!App"), title: String::new(), body: String::new(), time: String::new() };
        // Newest first: five from WhatsApp around one from Discord.
        let list: std::collections::VecDeque<notif::Notif> =
            [mk(7, "WhatsApp"), mk(6, "WhatsApp"), mk(5, "WhatsApp"), mk(4, "Discord"), mk(3, "WhatsApp"), mk(2, "WhatsApp")].into_iter().collect();
        assert_eq!(notif_view(&list), vec![0, 1, 3]);
    }
}

#[cfg(test)]
mod prayer_tests {
    use super::*;

    #[test]
    fn a_click_on_the_bar_maps_to_seconds_of_the_length_shown() {
        let spotify = music::Snapshot { app: "Spotify".into(), duration: 200.0, ..Default::default() };
        assert_eq!(song_length(&spotify, 30.0, 0.0), 200.0);
        // A browser whose length is wrong or missing: the lyrics' length.
        let chrome = music::Snapshot { app: "Chrome".into(), duration: 900.0, ..Default::default() };
        assert_eq!(song_length(&chrome, 30.0, 240.0), 240.0);
        let unknown = music::Snapshot { app: "Chrome".into(), ..Default::default() };
        assert_eq!(song_length(&unknown, 30.0, 0.0), 0.0);
        assert_eq!(seek_target(0.25, 240.0, true), Some(60.0));
        assert_eq!(seek_target(1.4, 240.0, true), Some(240.0));
        assert_eq!(seek_target(-0.2, 240.0, true), Some(0.0));
        assert_eq!(seek_target(0.5, 240.0, false), None, "the player can't seek");
        assert_eq!(seek_target(0.5, 0.0, true), None, "length unknown");
    }

    #[test]
    fn prayers_evenly_spaced_now_in_between() {
        // Subuh 04:04, Dzuhur 11:40, Ashar 15:02, Maghrib 17:52, Isya 19:05.
        let t = [244, 700, 902, 1072, 1145];
        let at: Vec<f32> = t.iter().map(|&m| timeline_at(&t, m as f32)).collect();
        assert_eq!(at, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(timeline_at(&t, 60.0), 0.0, "before Subuh: the start");
        assert_eq!(timeline_at(&t, 1230.0), 1.0, "after Isya: the end");
        // 16:27, halfway from Ashar to Maghrib.
        assert!((timeline_at(&t, 987.0) - 0.625).abs() < 1e-6);
        // 11:47: just past Dzuhur.
        let x = timeline_at(&t, 707.0);
        assert!(x > 0.25 && x < 0.27, "{x}");
    }

    #[test]
    fn events_follow_the_settings() {
        let day = prayer::day(2026, 11, 18, -6.2615, 107.1527, 7.0);
        let mut p = island_core::settings::PrayerSettings::default();
        let e = prayer_events(&day, &p);
        assert_eq!(e.len(), 10, "a reminder and the time for each of five");
        let dzuhur = day.get(prayer::Which::Dzuhur);
        assert!(e.contains(&(dzuhur - 10, prayer::Which::Dzuhur, true)) && e.contains(&(dzuhur, prayer::Which::Dzuhur, false)));
        assert!(e.windows(2).all(|w| w[0].0 <= w[1].0));
        p.remind_before_min = 0;
        assert!(prayer_events(&day, &p).iter().all(|e| !e.2), "0 = no reminders");
        p.at_time = false;
        assert!(prayer_events(&day, &p).is_empty());
    }
}
