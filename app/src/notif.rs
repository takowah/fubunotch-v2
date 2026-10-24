// The Notifications module: Windows notifications of the allowed apps, shown
// in the island and kept in a short history. Read with UserNotificationListener.
// An unpackaged app gets no "new notification" event, so a thread looks every
// POLL at the notification database's files (two stats) and reads the list
// only when they changed, or once a minute: the one poll in the app, as in v1.
// A click brings the app forward; a packaged app (WhatsApp) gets the toast's
// own launch arguments, so it opens the chat.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{s, w, HSTRING};
use windows::UI::Notifications::Management::{UserNotificationListener, UserNotificationListenerAccessStatus};
use windows::UI::Notifications::{KnownNotificationBindings, NotificationKinds, UserNotification};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Shell::{ApplicationActivationManager, IApplicationActivationManager, AO_NONE};
use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

const POLL: Duration = Duration::from_secs(2);
const HEARTBEAT: Duration = Duration::from_secs(60);
pub const TITLE_MAX: usize = 120;
pub const BODY_MAX: usize = 240;

/// One mirrored notification. Plain data, so it crosses threads.
#[derive(Clone, Debug, PartialEq)]
pub struct Notif {
    pub id: u32,
    pub app: String,
    pub aumid: String,
    pub title: String,
    pub body: String,
    /// "14:05", when it arrived.
    pub time: String,
}

/// Whether `app` / `aumid` is on the list ("*" = every app; an entry matches
/// when it is part of the name or of the AUMID, ignoring case). "!Name"
/// turns one app off (its whole name or AUMID): the per-app switches.
pub fn allowed(list: &[String], app: &str, aumid: &str) -> bool {
    let (app, aumid) = (app.to_lowercase(), aumid.to_lowercase());
    let entries: Vec<String> = list.iter().map(|e| e.trim().to_lowercase()).collect();
    if entries.iter().filter_map(|e| e.strip_prefix('!')).any(|off| off == app || off == aumid) {
        return false;
    }
    entries.iter().filter(|e| !e.starts_with('!')).any(|e| e == "*" || !e.is_empty() && (app.contains(e.as_str()) || aumid.contains(e.as_str())))
}

/// Every app seen sending a notification since the start (name → AUMID),
/// allowed or not: what the per-app switches list.
static SEEN: std::sync::Mutex<std::collections::BTreeMap<String, String>> = std::sync::Mutex::new(std::collections::BTreeMap::new());

pub fn seen_apps() -> std::collections::BTreeMap<String, String> {
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn note(n: &Notif) {
    if !n.app.is_empty() {
        SEEN.lock().unwrap_or_else(|e| e.into_inner()).insert(n.app.clone(), n.aumid.clone());
    }
}

fn cut(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max - 1).collect();
    out.push('…');
    out
}

pub struct Watcher {
    stop: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn database() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Microsoft\\Windows\\Notifications\\wpndatabase.db"))
}

fn fingerprint(db: &Option<PathBuf>) -> Option<[(u64, u128); 2]> {
    let db = db.as_ref()?;
    let stat = |p: PathBuf| -> Option<(u64, u128)> {
        let m = std::fs::metadata(p).ok()?;
        Some((m.len(), m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos()))
    };
    let mut wal = db.clone().into_os_string();
    wal.push("-wal");
    Some([stat(wal.into())?, stat(db.clone())?])
}

fn toasts(listener: &UserNotificationListener) -> windows::core::Result<Vec<UserNotification>> {
    let list = listener.GetNotificationsAsync(NotificationKinds::Toast)?.get()?;
    (0..list.Size()?).map(|i| list.GetAt(i)).collect()
}

fn read(n: &UserNotification) -> Option<Notif> {
    let info = n.AppInfo().ok()?;
    let app = info.DisplayInfo().and_then(|d| d.DisplayName()).map(|s| s.to_string()).unwrap_or_default();
    let aumid = info.AppUserModelId().map(|s| s.to_string()).unwrap_or_default();
    let binding = n.Notification().ok()?.Visual().ok()?.GetBinding(&KnownNotificationBindings::ToastGeneric().ok()?).ok()?;
    let texts = binding.GetTextElements().ok()?;
    let mut lines = Vec::new();
    for i in 0..texts.Size().ok()? {
        lines.push(texts.GetAt(i).ok()?.Text().ok()?.to_string());
    }
    let title = lines.first().cloned().unwrap_or_default();
    let body = lines.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
    Some(Notif {
        id: n.Id().ok()?,
        app: if app.is_empty() { aumid.clone() } else { app },
        aumid,
        title: cut(&title, TITLE_MAX),
        body: cut(&body, BODY_MAX),
        time: crate::local_time::clock_text(),
    })
}

/// Starts watching; `allowed_now` is asked for the current app list each time,
/// `sink` gets each new allowed notification. Dropping the Watcher stops it.
pub fn start(
    allowed_now: impl Fn() -> Vec<String> + Send + 'static,
    sink: impl Fn(Notif) + Send + 'static,
) -> Watcher {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("fubunotch-notifications".into())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            run(&flag, allowed_now, sink);
            if com.is_ok() {
                unsafe { CoUninitialize() };
            }
        })
        .ok();
    Watcher { stop }
}

fn run(stop: &AtomicBool, allowed_now: impl Fn() -> Vec<String>, sink: impl Fn(Notif)) {
    let Ok(listener) = UserNotificationListener::Current() else { return };
    let access = listener.GetAccessStatus().unwrap_or(UserNotificationListenerAccessStatus::Unspecified);
    if access != UserNotificationListenerAccessStatus::Allowed {
        // Asking works from any thread for an unpackaged app; Windows shows its prompt once.
        let asked = listener.RequestAccessAsync().and_then(|op| op.get());
        if asked != Ok(UserNotificationListenerAccessStatus::Allowed) {
            crate::store::log("notifications: no access (Settings → Privacy → Notifications)");
            return;
        }
    }
    let first = toasts(&listener).unwrap_or_default();
    for n in &first {
        if let Some(x) = read(n) {
            note(&x);
        }
    }
    let mut seen: HashSet<u32> = first.iter().filter_map(|n| n.Id().ok()).collect();
    let db = database();
    let mut last = fingerprint(&db);
    let mut read_at = Instant::now();
    crate::store::log(&format!("notifications: watching ({} there at start)", seen.len()));
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let now = fingerprint(&db);
        if now == last && now.is_some() && read_at.elapsed() < HEARTBEAT {
            continue;
        }
        last = now;
        read_at = Instant::now();
        let Ok(list) = toasts(&listener) else { continue };
        let ids: HashSet<u32> = list.iter().filter_map(|n| n.Id().ok()).collect();
        let apps = allowed_now();
        for n in &list {
            let Ok(id) = n.Id() else { continue };
            if seen.contains(&id) {
                continue;
            }
            if let Some(notif) = read(n) {
                note(&notif);
                if allowed(&apps, &notif.app, &notif.aumid) {
                    sink(notif);
                }
            }
        }
        seen = ids;
    }
}

// ── Opening the app ────────────────────────────────────────────────────────────

/// The `launch` attribute of a toast's XML, unescaped.
pub fn launch_of(payload: &str) -> Option<String> {
    let start = payload.find("<toast")?;
    let tag_end = start + payload[start..].find('>')?;
    let tag = &payload[start..tag_end];
    let at = tag.find(" launch=\"")? + " launch=\"".len();
    let value = &tag[at..at + tag[at..].find('"')?];
    let v = value.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    (!v.is_empty()).then_some(v)
}

/// The launch arguments of toast `id`, from the notification database, read
/// with the SQLite Windows ships (winsqlite3.dll). Only on a click.
fn toast_launch(id: u32) -> Option<String> {
    use std::ffi::{c_char, c_int, c_void, CString};
    type Open = unsafe extern "system" fn(*const c_char, *mut *mut c_void, c_int, *const c_char) -> c_int;
    type Prepare = unsafe extern "system" fn(*mut c_void, *const c_char, c_int, *mut *mut c_void, *mut *const c_char) -> c_int;
    type BindInt64 = unsafe extern "system" fn(*mut c_void, c_int, i64) -> c_int;
    type Step = unsafe extern "system" fn(*mut c_void) -> c_int;
    type Blob = unsafe extern "system" fn(*mut c_void, c_int) -> *const u8;
    type Bytes = unsafe extern "system" fn(*mut c_void, c_int) -> c_int;
    type Done = unsafe extern "system" fn(*mut c_void) -> c_int;

    let path = CString::new(database()?.to_str()?.to_string()).ok()?;
    unsafe {
        let lib = LoadLibraryW(w!("winsqlite3.dll")).ok()?;
        let open: Open = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_open_v2"))?);
        let prepare: Prepare = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_prepare_v2"))?);
        let bind: BindInt64 = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_bind_int64"))?);
        let step: Step = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_step"))?);
        let blob: Blob = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_column_blob"))?);
        let bytes: Bytes = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_column_bytes"))?);
        let finalize: Done = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_finalize"))?);
        let close: Done = std::mem::transmute(GetProcAddress(lib, s!("sqlite3_close"))?);
        let mut db: *mut c_void = std::ptr::null_mut();
        // 1 = SQLITE_OPEN_READONLY
        if open(path.as_ptr(), &mut db, 1, std::ptr::null()) != 0 {
            close(db);
            return None;
        }
        let sql = c"SELECT Payload FROM Notification WHERE Id = ?1 AND Type = 'toast'";
        let mut stmt: *mut c_void = std::ptr::null_mut();
        let mut payload = None;
        if prepare(db, sql.as_ptr(), -1, &mut stmt, std::ptr::null_mut()) == 0 {
            // 100 = SQLITE_ROW
            if bind(stmt, 1, id as i64) == 0 && step(stmt) == 100 {
                let (data, len) = (blob(stmt, 0), bytes(stmt, 0));
                if !data.is_null() && len > 0 {
                    payload = Some(std::slice::from_raw_parts(data, len as usize).to_vec());
                }
            }
            finalize(stmt);
        }
        close(db);
        launch_of(&String::from_utf8_lossy(&payload?))
    }
}

/// A packaged app's AUMID: "Family_hash!App".
fn is_packaged(aumid: &str) -> bool {
    aumid.split_once('!').is_some_and(|(family, app)| family.contains('_') && !app.is_empty() && !family.contains('\\'))
}

/// Brings the app of a notification forward (its chat, for WhatsApp).
pub fn open(n: &Notif) {
    let n = n.clone();
    std::thread::spawn(move || unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        let com = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut done = false;
        if is_packaged(&n.aumid) {
            let args = toast_launch(n.id).unwrap_or_default();
            if let Ok(manager) = CoCreateInstance::<_, IApplicationActivationManager>(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) {
                done = manager.ActivateApplication(&HSTRING::from(n.aumid.as_str()), &HSTRING::from(args), AO_NONE).is_ok();
            }
        }
        // Anything else (Discord): Windows starts it by its AUMID, which brings
        // a running app forward.
        if !done && !n.aumid.is_empty() && n.aumid.chars().all(|c| !c.is_control() && c != '"') {
            let _ = std::process::Command::new("explorer.exe").arg(format!("shell:AppsFolder\\{}", n.aumid)).spawn();
        }
        if com.is_ok() {
            CoUninitialize();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_allow_list() {
        let list = vec!["Discord".to_string(), "whatsapp".to_string()];
        assert!(allowed(&list, "Discord", "com.squirrel.Discord.Discord"));
        assert!(allowed(&list, "", "5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"));
        assert!(!allowed(&list, "Slack", "com.squirrel.slack.slack"));
        assert!(allowed(&["*".to_string()], "Slack", "x"));
        assert!(!allowed(&[" ".to_string()], "Slack", "x"));
        let all_but = vec!["*".to_string(), "!Discord".to_string()];
        assert!(!allowed(&all_but, "Discord", "com.squirrel.Discord.Discord"), "switched off");
        assert!(allowed(&all_but, "Discord PTB", "x"), "only that app, by its whole name");
        assert!(allowed(&all_but, "WhatsApp", "x"));
    }

    #[test]
    fn launch_arguments_come_out_unescaped() {
        let wa = r#"<?xml version="1.0"?><toast launch="action=Click&amp;tag=1%40lid"><visual/></toast>"#;
        assert_eq!(launch_of(wa).as_deref(), Some("action=Click&tag=1%40lid"));
        assert_eq!(launch_of("<toast><visual/></toast>"), None);
    }

    #[test]
    fn packaged_or_not() {
        assert!(is_packaged("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App"));
        assert!(!is_packaged("com.squirrel.Discord.Discord"));
        assert!(!is_packaged("{1AC14E77}\\WindowsPowerShell\\v1.0\\powershell.exe"));
    }
}
