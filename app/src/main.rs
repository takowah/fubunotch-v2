// FubuNotch v2: the island. Activities come in through the local webhook
// (island-webhook) and the modules; the arbiter (island-core) picks the front
// one; the user's settings decide which modules run and which tabs the
// expanded island shows. One timer: the arbiter's next wake, or the clock's
// next minute. Nothing else runs while nothing changes.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod autostart;
mod mascot;
mod bg;
mod pet;
mod claude;
mod local_time;
mod music;
mod notif;
mod system;
mod shelf;
mod snapshot;
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
const TOP: f32 = 6.0;
/// Room around the island for its glow.
const GLOW: f32 = 6.0;
const HIDDEN: (f32, f32) = (64.0, 6.0);
/// The hidden island's hover strip (wider than its bar, easy to point at).
const HIDDEN_HOVER: (f32, f32) = (200.0, 18.0);
const COMPACT_IDLE: (f32, f32) = (170.0, 34.0);
const COMPACT_BUSY: (f32, f32) = (390.0, 38.0);
const EXPANDED: (f32, f32) = (520.0, 200.0);
/// The expanded island grows for a long list (To-do), up to this tall; the
/// rest scrolls.
const EXPANDED_MAX_H: f32 = 362.0;
/// One to-do row, and how many fit the regular height.
const TODO_PITCH: f32 = 27.0;
const TODO_ROWS_FIT: usize = 4;
/// The compact island goes up this long after the pointer left (or a notice
/// came): Settings → General.
fn hide_after(s: &Settings) -> u64 {
    s.general.hide_after_s.clamp(2, 60) as u64 * 1000
}
/// The island springs between sizes over this long (Ice.morph); the window
/// region follows once it is done.
const MORPH_MS: u64 = 450;
/// The window never changes size: the largest island and its glow.
const WINDOW: (f32, f32) = (EXPANDED.0 + 2.0 * GLOW + 8.0, TOP + EXPANDED_MAX_H + GLOW + 4.0);

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
    /// Keeps the island expanded (FUBUNOTCH_V2_EXPANDED): screenshots without the mouse.
    pinned: bool,
    /// Music's Hold: the island stays open when the pointer leaves (let go
    /// on leaving the Music tab, so it can't stay stuck open).
    held: bool,
    /// The mascot: its mood, and the timers that blink it and end a mood.
    /// The mascot's life and its clock (running only while the island shows).
    life: mascot::Life,
    life_timer: Timer,
    /// The island's size for its mode, and the timer that narrows the window
    /// region to it once the spring has finished.
    size_to: (f32, f32),
    region_timer: Timer,
    active_tab: String,
    settings: Settings,
    /// What is playing (the Music module), and its artwork as an image.
    music: music::Snapshot,
    music_art: Option<slint::Image>,
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
    /// The daily modules.
    focus: Focus,
    focus_plan: FocusPlan,
    water_plan: WaterPlan,
    /// When the last water reminder went (minute of the day).
    water_reminded: Option<u32>,
    /// When the focus round under way was first started (minutes after midnight).
    focus_started_min: Option<u32>,
    data: store::Data,
    /// Claude Code permission requests waiting for a click: activity id → answer.
    pending: HashMap<String, island_webhook::Reply>,
    ask_seq: u64,
    confirm: Option<ConfirmWindow>,
    /// A file is being dragged over the island.
    drop_hover: bool,
    timer: Timer,
    settings_window: Option<SettingsWindow>,
    /// The mascot living in Settings, and its clock (only while Settings is open).
    pet: Option<pet::Pet>,
    pet_timer: Option<Timer>,
    /// The island's background picture: for which file, size and look, and the picture.
    bg_key: String,
    bg_image: Option<slint::Image>,
    bg_preview: Option<slint::Image>,
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
    const TONES: [(u8, u8, u8); 5] = [(0x5f, 0xd4, 0xff), (0xb8, 0xe8, 0xff), (0x7c, 0xf2, 0xd0), (0xc4, 0xb5, 0xff), (0x9f, 0xc8, 0xff)];
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

fn item(a: &Activity) -> Item {
    let actions: Vec<ActionInfo> = a.actions.iter().map(|x| ActionInfo { id: x.id.as_str().into(), label: x.label.as_str().into() }).collect();
    Item {
        actions: ModelRc::new(VecModel::from(actions)),
        // System notices travel as "icon:<name>": the capsule draws that icon.
        source: if a.source.starts_with("icon:") { "system".into() } else { a.source.as_str().into() },
        icon: a.source.strip_prefix("icon:").unwrap_or("").into(),
        title: a.title.as_str().into(),
        body: a.body.as_str().into(),
        kind: kind_name(a.kind).into(),
        progress: a.progress.unwrap_or(0.0),
        has_progress: a.progress.is_some(),
        accent: accent(a),
    }
}

/// Puts the (fixed-size) window at the top centre of its monitor.
fn place(ui: &Island) {
    ui.window().set_size(LogicalSize::new(WINDOW.0, WINDOW.1));
    ui.window().with_winit_window(|win: &winit::window::Window| {
        if let Some(monitor) = win.current_monitor().or_else(|| win.primary_monitor()) {
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

/// The island's native window, once the event loop has made it.
fn island_hwnd(ui: &Island) -> Option<windows::Win32::Foundation::HWND> {
    hwnd_of(ui.window())
}

/// A Slint window's native window, once the event loop has made it.
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
fn refresh(ui: &Island) {
    with_state(|state| {
        let now = now_ms(state.started);
        daily_tick(state, now);
        // Before anything borrows the state: the lyric list is made (once a song) here.
        let lines = lyric_lines(state);
        state.arbiter.tick(now);
        let live: Vec<String> = state.arbiter.ranked().iter().map(|a| a.id.clone()).collect();
        state.pending.retain(|id, _| live.contains(id));
        let view = state.arbiter.view();
        let front_id = view.front.map(|a| a.id.clone());
        ui.set_has_front(view.front.is_some());
        if let Some(front) = view.front {
            ui.set_front(item(front));
        }
        ui.set_waiting(view.waiting as i32);
        let others: Vec<Item> = state
            .arbiter
            .ranked()
            .into_iter()
            .filter(|a| Some(&a.id) != front_id.as_ref())
            .take(OTHERS_MAX)
            .map(item)
            .collect();
        ui.set_others(ModelRc::new(VecModel::from(others)));
        ui.set_clock(SharedString::from(local_time::clock_text()));

        // Tabs: Home, then the enabled modules that have a page, in the user's order.
        let tabs: Vec<TabInfo> = state.settings.tabs().iter().map(|t| TabInfo { id: t.id.into(), label: t.name.into() }).collect();
        if state.active_tab != "home" && !tabs.iter().any(|t| t.id == state.active_tab.as_str()) {
            state.active_tab = "home".into();
        }
        ui.set_tabs(ModelRc::new(VecModel::from(tabs)));
        ui.set_active_tab(state.active_tab.as_str().into());
        let lyrics_on = state.settings.music.lyrics || state.settings.music.lyrics_youtube;
        let rows: Vec<NotifRow> = state
            .notifications
            .iter()
            .map(|n| NotifRow {
                app: n.app.as_str().into(),
                title: n.title.as_str().into(),
                body: n.body.as_str().into(),
                time: n.time.as_str().into(),
                accent: accent_of(&n.app, Kind::Alert),
            })
            .collect();
        ui.set_notifications(ModelRc::new(VecModel::from(rows)));

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
        let tasks: Vec<TaskRow> = state.data.todo.ordered().into_iter().map(|t| TaskRow { id: t.id as i32, text: t.text.as_str().into(), done: t.done }).collect();
        ui.set_tasks(ModelRc::new(VecModel::from(tasks)));
        ui.set_todo_open(state.data.todo.open_count() as i32);
        let shelf_rows: Vec<ShelfRow> = state.data.shelf.iter().map(|f| ShelfRow { name: f.name().into(), detail: f.detail().into() }).collect();
        ui.set_shelf(ModelRc::new(VecModel::from(shelf_rows)));
        ui.set_drop_hover(state.drop_hover);
        *NOTIF_APPS.lock().unwrap_or_else(|e| e.into_inner()) = state.settings.notifications.filter();
        let rating = state.data.ratings.get(&song_key(&state.music)).copied().unwrap_or(0);
        if state.active_tab != "music" {
            state.held = false;
        }
        ui.set_held(state.held);
        let ytm = ytm_live(state, now);
        ui.set_music(music_view(&state.music, &state.music_art, &state.lyrics, lines, lyrics_on, state.lyrics_nudge, rating, ytm.as_ref()));

        // Modes: a new notice pops the hidden island up; the compact island
        // goes up hide_after() later unless someone waits for an answer.
        let front_kind = view.front.map(|a| a.kind);
        let front_changed = front_id.is_some() && front_id != state.last_front;
        state.last_front = front_id.clone();
        if state.pinned {
            // FUBUNOTCH_V2_EXPANDED=compact pins the capsule instead (checks).
            state.mode = if std::env::var("FUBUNOTCH_V2_EXPANDED").is_ok_and(|v| v == "compact") { Mode::Compact } else { Mode::Expanded };
            state.hide_at = None;
        } else if front_changed
            && front_kind != Some(Kind::Ambient)
            && state.mode == Mode::Hidden
            // Settings → General → Pop up for activities; water reminders still come (v1).
            && (state.settings.general.popups || front_id.as_deref() == Some("water:remind"))
        {
            state.mode = Mode::Compact;
            state.hide_at = Some(now + hide_after(&state.settings));
        }
        let asking = front_kind == Some(Kind::Ask);
        if asking && state.mode == Mode::Hidden {
            state.mode = Mode::Compact;
        }
        if state.mode == Mode::Compact && !asking && state.hide_at.is_some_and(|t| now >= t) {
            state.mode = Mode::Hidden;
            state.hide_at = None;
        }
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
        let size = match state.mode {
            Mode::Expanded if state.active_tab == "todo" => {
                let extra = state.data.todo.tasks.len().saturating_sub(TODO_ROWS_FIT) as f32 * TODO_PITCH;
                (EXPANDED.0, (EXPANDED.1 + extra).min(EXPANDED_MAX_H))
            }
            Mode::Expanded => EXPANDED,
            Mode::Hidden => HIDDEN,
            Mode::Compact if front_id.is_some() => COMPACT_BUSY,
            Mode::Compact => COMPACT_IDLE,
        };
        morph_to(ui, state, size);
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
        let music_on_screen = expanded && state.active_tab == "music" && state.music.playing;
        // The lyrics move on more often than the progress bar.
        let page_tick = match (music_on_screen, state.lyrics.as_ref().is_some_and(|l| !l.lines.is_empty())) {
            (true, true) => 250,
            (true, false) => 1_000,
            _ => u64::MAX,
        };
        // The Pomodoro counts down in seconds while its page is on screen.
        let focus_tick = if expanded && state.active_tab == "focus" && state.focus.running() { 1_000 } else { u64::MAX };
        let page_tick = page_tick.min(focus_tick).min(daily_wake(state, now));
        let hide_in = state.hide_at.filter(|_| state.mode == Mode::Compact && !asking).map_or(u64::MAX, |t| t.saturating_sub(now));
        let delay = wake.unwrap_or(u64::MAX).min(local_time::ms_to_next_minute()).min(page_tick).min(hide_in).max(15);
        let weak = ui.as_weak();
        state.timer.start(TimerMode::SingleShot, std::time::Duration::from_millis(delay), move || {
            if let Some(ui) = weak.upgrade() {
                refresh(&ui);
            }
        });
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
        ui.set_mascot_mood(p.frame.into());
        ui.set_mascot_dx(p.dx);
        ui.set_mascot_dy(p.dy);
        ui.set_mascot_left(p.left);
    }
}

/// Clicked, or the island came back: she hops.
fn poke_mascot(ui: &Island) {
    with_state(|state| state.life.poke());
    mascot_tick(ui);
}

fn apply(ui: &Island, event: Event) {
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
        }
    });
    refresh(ui);
}

/// Every live activity whose id starts with `prefix` ends.
fn end_prefix(state: &mut State, prefix: &str, now: u64) {
    let ids: Vec<String> = state.arbiter.ranked().iter().map(|a| a.id.clone()).filter(|id| id.starts_with(prefix)).collect();
    for id in ids {
        state.arbiter.end(&id, now);
        state.pending.remove(&id);
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
            }
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
fn lyric_lines(state: &mut State) -> Option<ModelRc<LyricLine>> {
    let l = state.lyrics.as_ref().filter(|l| l.status == island_lyrics::Status::Found && !l.lines.is_empty())?;
    let key = format!("{}#{}#{}", state.lyrics_key, l.lines.len(), l.lines.iter().map(|x| x.text.len()).sum::<usize>());
    if let Some((k, model)) = &state.lyrics_model {
        if *k == key {
            return Some(model.clone());
        }
    }
    let rows: Vec<LyricLine> = l
        .lines
        .iter()
        .map(|x| LyricLine { text: if x.text.trim().is_empty() { "♪".into() } else { x.text.as_str().into() }, current: false })
        .collect();
    let model = ModelRc::new(VecModel::from(rows));
    state.lyrics_model = Some((key, model.clone()));
    Some(model)
}

/// The extension's report, while it is fresh (it repeats every 15 s) and the
/// music is playing in a browser.
fn ytm_live(state: &State, now: u64) -> Option<island_webhook::YtmState> {
    let (ytm, at) = state.ytm.as_ref()?;
    let browser = matches!(state.music.app.as_str(), "Chrome" | "Edge" | "Brave" | "Opera") || state.music.app.to_lowercase().contains("chrome");
    (browser && now.saturating_sub(*at) < 40_000).then(|| ytm.clone())
}

fn music_view(
    snap: &music::Snapshot,
    art: &Option<slint::Image>,
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
    let shift = lyrics.as_ref().map_or(0.0, |l| island_lyrics::intro_shift(snap.duration, l.duration)) + nudge;
    let sung_at = position - shift;
    let fraction = if snap.duration > 0.0 { (position / snap.duration) as f32 } else { 0.0 };
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
        remaining: format!("-{}", local_time::mm_ss((snap.duration - position).max(0.0))).into(),
        has_art: art.is_some(),
        art: art.clone().unwrap_or_default(),
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
    }
}

/// A new snapshot from the Music module: the page, and the ambient "now playing".
fn on_music(ui: &Island, snap: music::Snapshot) {
    with_state(|state| {
        let now = now_ms(state.started);
        if state.music.art != snap.art {
            state.music_art = snap.art.as_ref().and_then(|rgba| {
                let px = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba, music::ART_PX, music::ART_PX);
                (rgba.len() == (music::ART_PX * music::ART_PX * 4) as usize).then(|| slint::Image::from_rgba8(px))
            });
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
        let query = island_lyrics::Query {
            title: state.music.title.clone(),
            artist: state.music.artist.clone(),
            album: state.music.album.clone(),
            duration: state.music.duration,
        };
        let key = format!("{}\u{1f}{}{}{}", query.key(), sources.lrclib as u8, sources.youtube as u8, sources.romaji as u8);
        if key == state.lyrics_key {
            return None;
        }
        state.lyrics_key = key.clone();
        state.lyrics = None;
        state.lyrics_nudge = 0.0;
        Some((key, query, sources))
    })
    .flatten();
    let Some((key, query, sources)) = job else { return };
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
    );
    with_state(|state| state.notif_watcher = Some(watcher));
}

/// A new notification: into the history, and up on the island if pop-ups are on.
fn on_notification(ui: &Island, n: notif::Notif) {
    with_state(|state| {
        let now = now_ms(state.started);
        if state.settings.notifications.popups {
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
        state.notifications.retain(|old| old.id != n.id);
        state.notifications.push_front(n);
        state.notifications.truncate(NOTIF_HISTORY);
    });
    refresh(ui);
}

/// A change from the System module: shown when its switch is on (and the module is).
fn on_system(ui: &Island, e: system::Event) {
    with_state(|state| {
        let sys = &state.settings.system;
        let (on, id) = match e {
            system::Event::Volume(..) => (sys.volume, "sys:volume"),
            system::Event::CapsLock(_) => (sys.caps_lock, "sys:caps"),
            system::Event::Layout(_) => (sys.keyboard_layout, "sys:layout"),
            system::Event::Power(..) => (sys.charger, "sys:power"),
            system::Event::Network(_) => (sys.network, "sys:network"),
            system::Event::Headphones(..) => (sys.audio_device, "sys:audio"),
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
            format!("Every {} min · {}–{}", plan.every_min, local_time::hh_mm(plan.from_min), local_time::hh_mm(plan.until_min))
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
    wake
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
        Ok(_) => (false, "Connecting adds FubuNotch v2's hooks to Claude Code's settings.json (a backup first).".to_string()),
        Err(e) => (false, e),
    };
    let preview = with_state(|state| state.bg_preview.clone()).flatten();
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
    win.set_s(SettingsView {
        modules: ModelRc::new(VecModel::from(modules)),
        lyrics: s.music.lyrics,
        lyrics_youtube: s.music.lyrics_youtube,
        romaji: s.music.romaji,
        notif_popups: s.notifications.popups,
        notif_list: ModelRc::new(VecModel::from(notif_list)),
        sys_volume: s.system.volume,
        sys_caps: s.system.caps_lock,
        sys_layout: s.system.keyboard_layout,
        sys_charger: s.system.charger,
        sys_network: s.system.network,
        sys_audio: s.system.audio_device,
        autostart: s.autostart,
        mascot: s.look.mascot,
        popups: s.general.popups,
        hide_after: s.general.hide_after_s.clamp(2, 60) as i32,
        theme: s.look.theme().into(),
        has_background: !s.look.background.is_empty(),
        background_name: Path::new(&s.look.background).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into(),
        background_preview: preview.unwrap_or_default(),
        background_dim: (s.look.background_dim.clamp(0.0, 0.9) * 100.0).round() as i32,
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
        // The mascot cheers each change.
        if let Some(p) = state.pet.as_mut() {
            p.cheer();
        }
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
        "look.mascot" => s.look.mascot = on,
        "general.popups" => s.general.popups = on,
        "water.reminders" => s.water.reminders = on,
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
        "water.every_min" => s.water.every_min = u.clamp(10, 480),
        "water.goal_ml" => s.water.goal_ml = u.clamp(100, 10_000),
        "water.glass_ml" => s.water.glass_ml = u.clamp(10, 2000),
        "water.from" => s.water.from = local_time::hh_mm(u.min(1439)),
        "water.until" => s.water.until = local_time::hh_mm(u.min(1439)),
        "focus.focus_min" => s.focus.focus_min = u.clamp(1, 180),
        "focus.short_min" => s.focus.short_min = u.clamp(1, 60),
        "focus.long_min" => s.focus.long_min = u.clamp(1, 120),
        "focus.rounds" => s.focus.rounds = u.clamp(1, 12),
        _ => {}
    }
}

/// The Settings window's room for the mascot, from its height now.
fn pet_room(win: &SettingsWindow) -> pet::Room {
    let h = win.window().size().height as f32 / win.window().scale_factor().max(0.5);
    pet::Room { h: if h > 10.0 { h } else { 600.0 }, wall: 232.0 }
}

/// Moves the mascot one step and draws it; stops when Settings is gone.
fn pet_tick() {
    let win = with_state(|state| state.settings_window.as_ref().map(|w| w.as_weak())).flatten().and_then(|w| w.upgrade());
    let Some(win) = win else {
        with_state(|state| state.pet_timer = None);
        return;
    };
    if !win.window().is_visible() {
        with_state(|state| state.pet_timer = None);
        return;
    }
    let room = pet_room(&win);
    let shown = with_state(|state| {
        let p = state.pet.get_or_insert_with(|| pet::Pet::new(room));
        p.tick(room);
        (p.x, p.y, p.frame, p.left, p.say.clone())
    });
    if let Some((x, y, frame, left, say)) = shown {
        win.set_pet_x(x);
        win.set_pet_y(y);
        win.set_pet_frame(frame.into());
        win.set_pet_left(left);
        win.set_pet_say(say.into());
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
        let r = if state.mode == Mode::Expanded { 28.0 } else { h / 2.0 };
        let p = theme::palette(look.theme());
        let tint = ((p.body >> 16) as u8, (p.body >> 8) as u8, p.body as u8);
        let key = format!("{}|{w}x{h}|{scale}|{}|{}", look.background, look.background_dim, look.theme());
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

fn open_settings(ui: &Island) {
    let existing = with_state(|state| state.settings_window.as_ref().map(|w| w.as_weak())).flatten();
    if let Some(win) = existing.and_then(|w| w.upgrade()) {
        let _ = win.show();
        start_pet();
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
    win.on_theme_picked(move |theme| {
        if let Some(ui) = island.upgrade() {
            change_settings(&ui, |s| s.look.theme = theme.to_string());
            with_state(|state| {
                state.bg_preview = None;
                if let Some(p) = state.pet.as_mut() {
                    p.poke(&theme);
                }
            });
            update_background(&ui);
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
    win.on_action(move |what| {
        let Some(ui) = island.upgrade() else { return };
        match what.as_str() {
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
    win.on_pet_clicked(|| {
        with_state(|state| {
            let theme = state.settings.look.theme();
            if let Some(p) = state.pet.as_mut() {
                p.poke(theme);
            }
        });
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
    start_pet();
}

/// The mascot's clock: 10 ticks a second while Settings is open, nothing otherwise.
fn start_pet() {
    let running = with_state(|state| state.pet_timer.is_some()).unwrap_or(false);
    if running {
        return;
    }
    let timer = Timer::default();
    timer.start(TimerMode::Repeated, std::time::Duration::from_millis(1000 / pet::HZ), pet_tick);
    with_state(|state| state.pet_timer = Some(timer));
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
        store::log("claude hooks: fubunotch-v2-hook.exe is not next to the app");
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
            "Adds FubuNotch v2's hooks for {} to:\n{path}\n\nA backup of the file is saved first (settings.json.bak-fubunotch-v2-<date>). Your other settings and hooks, FubuNotch v1's included, stay as they are. If both v1 and v2 are running, both islands may ask for the same permission.",
            events.join(", ")
        )
    } else {
        format!("Removes FubuNotch v2's hooks from:\n{path}\n\nA backup is saved first. Nothing else in the file changes.")
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
    let pinned = std::env::var_os("FUBUNOTCH_V2_EXPANDED").is_some();
    let first_tab = std::env::var("FUBUNOTCH_V2_TAB").unwrap_or_else(|_| "home".into());
    let mut settings = store::load();
    // FUBUNOTCH_V2_THEME: a theme for this run only (checks of each look).
    if let Ok(t) = std::env::var("FUBUNOTCH_V2_THEME") {
        settings.look.theme = t;
    }
    // FUBUNOTCH_V2_BG: a background picture for this run only.
    if let Ok(b) = std::env::var("FUBUNOTCH_V2_BG") {
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
            life: mascot::Life::default(),
            life_timer: Timer::default(),
            size_to: (0.0, 0.0),
            region_timer: Timer::default(),
            active_tab: first_tab,
            settings,
            music: music::Snapshot::default(),
            music_art: None,
            lyrics_key: String::new(),
            lyrics: None,
            lyrics_model: None,
            ytm: None,
            lyrics_nudge: 0.0,
            notif_watcher: None,
            notifications: VecDeque::new(),
            focus: Focus::new(&focus_plan),
            focus_plan,
            water_plan,
            water_reminded: None,
            focus_started_min: None,
            data: store::load_data(),
            pending: HashMap::new(),
            ask_seq: 0,
            confirm: None,
            drop_hover: false,
            timer: Timer::default(),
            settings_window: None,
            pet: None,
            pet_timer: None,
            bg_key: String::new(),
            bg_image: None,
            bg_preview: None,
        });
    });

    let weak = ui.as_weak();
    ui.on_hover_changed(move |over| {
        let Some(ui) = weak.upgrade() else { return };
        let inside = false;
        let was_hidden = with_state(|state| {
            let was_hidden = state.mode == Mode::Hidden;
            if over || inside {
                state.mode = Mode::Expanded;
                state.hide_at = None;
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
            }
        });
        refresh(&ui);
    });

    let weak = ui.as_weak();
    ui.on_tab_clicked(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| state.active_tab = id.to_string());
        refresh(&ui);
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
        if command.as_str() == "hold" {
            with_state(|state| state.held = !state.held);
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
            if let Some(reply) = state.pending.remove(&id) {
                // Only what the user clicked goes back: "allow" or "deny".
                if action == "allow" || action == "deny" {
                    let _ = reply.0.send(action.to_string());
                }
            }
            state.arbiter.end(&id, now);
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
            state.data.todo.toggle(id as u64);
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
            state.data.todo.clear_done();
            store::save_data(&state.data);
        });
        refresh(&ui);
    });
    let weak = ui.as_weak();
    ui.on_todo_remove(move |id| {
        let Some(ui) = weak.upgrade() else { return };
        with_state(|state| {
            state.data.todo.remove(id as u64);
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
    let weak = ui.as_weak();
    system::start(move |e| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                on_system(&ui, e);
            }
        });
    });

    if with_state(|s| s.settings.is_on("notifications")).unwrap_or(false) {
        *NOTIF_APPS.lock().unwrap_or_else(|e| e.into_inner()) = with_state(|s| s.settings.notifications.filter()).unwrap_or_default();
        start_notifications(&ui);
    }

    if std::env::var_os("FUBUNOTCH_V2_DEMO_WATER").is_some() {
        // A day with some glasses, in memory only.
        with_state(|state| {
            let mut day = island_core::daily::WaterDay { date: local_time::today(), ..Default::default() };
            for m in [9 * 60 + 10, 10 * 60 + 40, 11 * 60 + 5, 13 * 60 + 30, 15 * 60] {
                day.drink(250, m);
            }
            state.data.water = day;
        });
    }
    if std::env::var_os("FUBUNOTCH_V2_DEMO_FOCUS").is_some() {
        // Today's rounds, in memory only.
        with_state(|state| {
            let today = local_time::today();
            state.data.focus_log = [(540, 565, 1500, true), (575, 600, 1500, true), (610, 628, 1080, false)]
                .into_iter()
                .map(|(from_min, to_min, focused_s, full)| daily::FocusRound { date: today.clone(), from_min, to_min, focused_s, full })
                .collect();
        });
    }
    if std::env::var_os("FUBUNOTCH_V2_DEMO_TODO").is_some() {
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
                todo.toggle(ids[i]);
            }
            state.data.todo = todo;
        });
    }
    if std::env::var_os("FUBUNOTCH_V2_DEMO_NOTIFS").is_some() {
        // A few notifications, for checks of the page without real ones.
        with_state(|state| {
            for (id, app, title, body, time) in [
                (3, "Discord", "#general", "The build is green, shipping tonight", "00:55"),
                (2, "WhatsApp", "Mika", "See you at seven?", "00:41"),
                (1, "Discord", "Ren", "", "00:12"),
            ] {
                state.notifications.push_back(notif::Notif { id, app: app.into(), aumid: String::new(), title: title.into(), body: body.into(), time: time.into() });
            }
        });
    }
    if std::env::var_os("FUBUNOTCH_V2_DEMO_MUSIC").is_some() {
        // A song with lyrics, for checks of the Music page without touching the player.
        with_state(|state| {
            state.music = music::Snapshot {
                has_session: true,
                title: "FOXXXROOM".into(),
                artist: "Shirakami Fubuki".into(),
                app: "demo".into(),
                playing: true,
                position: 11.0,
                duration: 207.0,
                at_unix_ms: music::now_unix_ms(),
                shuffle: Some(false),
                repeat: Some(music::Repeat::All),
                ..Default::default()
            };
            state.lyrics = Some(island_lyrics::Lyrics {
                status: island_lyrics::Status::Found,
                lines: ["koko wa kitto yume no naka ne", "", "mezameta no wa uso mitai da, hontou no kimi wa doko ni iru no ka mada wakaranai mama", "kimi no koe ga kikoeru", "tooku made"]
                    .iter()
                    .zip([0.25, 4.78, 10.0, 14.5, 18.0])
                    .map(|(t, s)| island_lyrics::Line { time: s, text: t.to_string() })
                    .collect(),
                duration: 0.0,
            });
        });
    } else if with_state(|s| s.settings.is_on("music")).unwrap_or(false) {
        start_music(&ui);
    }

    // Tray: Settings and Quit.
    // FUBUNOTCH_V2_SNAPSHOT: draw the island (or Settings, with
    // FUBUNOTCH_V2_SETTINGS) into a PNG and stop; nothing is shown.
    if let Some(path) = snapshot {
        // The watcher threads (notifications, music) report in real time.
        std::thread::sleep(std::time::Duration::from_millis(2000));
        refresh(&ui);
        snapshot::advance(1500);
        refresh(&ui);
        snapshot::advance(1500);
        if std::env::var_os("FUBUNOTCH_V2_SETTINGS").is_some() {
            open_settings(&ui);
            if let Some(page) = std::env::var("FUBUNOTCH_V2_SETTINGS").ok().filter(|p| p.len() > 1) {
                if let Some(w) = with_state(|s| s.settings_window.as_ref().map(|w| w.as_weak())).flatten().and_then(|w| w.upgrade()) {
                    w.set_page(page.into());
                }
            }
            snapshot::advance(4000);
            let size = with_state(|s| s.settings_window.as_ref().map(|w| {
                let p = w.window().size();
                (p.width as f32 / snapshot::SCALE, p.height as f32 / snapshot::SCALE)
            }))
            .flatten()
            .filter(|s| s.0 > 10.0)
            .unwrap_or((900.0, 600.0));
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
        let _ = slint::quit_event_loop();
    });

    let token = island_webhook::load_or_create_token(&store::data_dir())?;
    let weak = ui.as_weak();
    island_webhook::spawn(island_webhook::PORT, token, move |event| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                apply(&ui, event);
            }
        });
    })?;

    place(&ui);
    ui.show()?;
    // No taskbar button, whatever the window attributes did: the tray icon is the app's.
    ui.window().with_winit_window(|w: &winit::window::Window| {
        use winit::platform::windows::WindowExtWindows;
        w.set_skip_taskbar(true);
    });
    tray.show()?;
    refresh(&ui);
    // FUBUNOTCH_V2_CYCLE switches tabs on its own (checks of tab changes without the mouse).
    if std::env::var_os("FUBUNOTCH_V2_CYCLE").is_some() {
        let weak = ui.as_weak();
        let n = std::cell::Cell::new(0usize);
        let cycle = Timer::default();
        cycle.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(900), move || {
            const TABS: [&str; 7] = ["home", "music", "notifications", "focus", "water", "todo", "shelf"];
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
    // FUBUNOTCH_V2_SETTINGS opens Settings at start (checks without the mouse).
    let settings_at_start = std::env::var_os("FUBUNOTCH_V2_SETTINGS").is_some();
    Timer::single_shot(std::time::Duration::from_millis(80), move || {
        if let Some(ui) = weak.upgrade() {
            place(&ui);
            // Before the first morph: the island is a popup from the start.
            if let Some(hwnd) = island_hwnd(&ui) {
                popup_style(hwnd);
            }
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
