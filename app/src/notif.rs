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

pub fn note(n: &Notif) {
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

/// An app's logo as Windows shows it (its tile), LOGO_PX square RGBA, kept
/// per app (by AUMID) the first time one of its notifications is read, so the
/// island draws the real icon instead of a letter. None when Windows has none
/// (some unpackaged apps). Read big (SRC_PX), the empty margin cut off (a
/// tile logo like WhatsApp's sits small in the middle of its square), then
/// averaged down to LOGO_PX; main.rs averages it again to the exact size each
/// place draws it (26-34 px, up to 51 physical at 1.5×).
pub const LOGO_PX: u32 = 128;
const SRC_PX: u32 = 256;

/// A square RGBA picture (side `n`): its transparent margin cut off, centred
/// on a square, and averaged down to LOGO_PX. None when it is all transparent.
fn fit_logo(px: &[u8], n: u32) -> Option<Vec<u8>> {
    if px.len() != (n * n * 4) as usize {
        return None;
    }
    let alpha = |x: u32, y: u32| px[((y * n + x) * 4 + 3) as usize];
    let (mut x0, mut y0, mut x1, mut y1) = (n, n, 0u32, 0u32);
    for y in 0..n {
        for x in 0..n {
            if alpha(x, y) > 16 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x + 1);
                y1 = y1.max(y + 1);
            }
        }
    }
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let side = (x1 - x0).max(y1 - y0) as f32;
    let left = (x0 + x1) as f32 / 2.0 - side / 2.0;
    let top = (y0 + y1) as f32 / 2.0 - side / 2.0;
    let out_n = LOGO_PX;
    let step = side / out_n as f32;
    let mut out = vec![0u8; (out_n * out_n * 4) as usize];
    for oy in 0..out_n {
        let (sy0, sy1) = (top + oy as f32 * step, top + (oy + 1) as f32 * step);
        for ox in 0..out_n {
            let (sx0, sx1) = (left + ox as f32 * step, left + (ox + 1) as f32 * step);
            // Area average in premultiplied colour; outside the picture is transparent.
            let (mut r, mut g, mut b, mut a, mut wsum) = (0f32, 0f32, 0f32, 0f32, 0f32);
            let mut y = sy0.floor();
            while y < sy1 {
                let wy = (sy1.min(y + 1.0) - sy0.max(y)).max(0.0);
                let mut x = sx0.floor();
                while x < sx1 {
                    let wx = (sx1.min(x + 1.0) - sx0.max(x)).max(0.0);
                    let w = wx * wy;
                    if x >= 0.0 && y >= 0.0 && (x as u32) < n && (y as u32) < n {
                        let i = ((y as u32 * n + x as u32) * 4) as usize;
                        let pa = px[i + 3] as f32 / 255.0;
                        r += px[i] as f32 * pa * w;
                        g += px[i + 1] as f32 * pa * w;
                        b += px[i + 2] as f32 * pa * w;
                        a += pa * w;
                    }
                    wsum += w;
                    x += 1.0;
                }
                y += 1.0;
            }
            let o = ((oy * out_n + ox) * 4) as usize;
            if a > 0.0 && wsum > 0.0 {
                out[o] = (r / a).round().min(255.0) as u8;
                out[o + 1] = (g / a).round().min(255.0) as u8;
                out[o + 2] = (b / a).round().min(255.0) as u8;
                out[o + 3] = (a / wsum * 255.0).round().min(255.0) as u8;
            }
        }
    }
    Some(out)
}
static LOGOS: std::sync::Mutex<Option<std::collections::HashMap<String, Option<Arc<Vec<u8>>>>>> = std::sync::Mutex::new(None);

pub fn logo(aumid: &str) -> Option<Arc<Vec<u8>>> {
    LOGOS.lock().unwrap_or_else(|e| e.into_inner()).as_ref()?.get(aumid).cloned().flatten()
}

/// The Claude desktop app: Claude Code's questions and notices wear its logo.
pub const CLAUDE_AUMID: &str = "Claude_pzs8sxrjxfjjc!Claude";

/// An app's logo by AUMID even before it sent a notification: kept when it is
/// known, else read from the icon Start shows for it (and kept). Needs COM on
/// the calling thread and touches the shell, so not on the UI thread: see `preload`.
/// A miss is not kept, so a later notification can still bring the logo.
pub fn ensure_logo(aumid: &str) -> Option<Arc<Vec<u8>>> {
    if aumid.is_empty() {
        return None;
    }
    if let Some(px) = logo(aumid) {
        return Some(px);
    }
    let px = Arc::new(shell_icon(aumid)?);
    LOGOS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(aumid.to_string(), Some(px.clone()));
    Some(px)
}

/// Keeps a logo made elsewhere (the demo's stand-ins in snapshots).
pub fn put_logo(aumid: &str, px: Vec<u8>) {
    if px.len() == (LOGO_PX * LOGO_PX * 4) as usize {
        LOGOS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(aumid.to_string(), Some(Arc::new(px)));
    }
}

/// Reads the logos of `aumids` on a thread of its own (with COM), then calls
/// `done`. Join the handle to wait for them (snapshots).
pub fn preload(aumids: Vec<String>, done: impl FnOnce() + Send + 'static) -> Option<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("kanmuri-logos".into())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            for a in &aumids {
                let _ = ensure_logo(a);
            }
            if com.is_ok() {
                unsafe { CoUninitialize() };
            }
            done();
        })
        .ok()
}

fn keep_logo(aumid: &str, info: &windows::ApplicationModel::AppInfo) {
    if aumid.is_empty() {
        return;
    }
    let mut map = LOGOS.lock().unwrap_or_else(|e| e.into_inner());
    if map.as_ref().is_some_and(|m| m.contains_key(aumid)) {
        return;
    }
    drop(map);
    // A packaged app's tile first; else the icon Start shows for it (unpackaged apps).
    let px = decode_logo(info).ok().or_else(|| shell_icon(aumid)).map(Arc::new);
    map = LOGOS.lock().unwrap_or_else(|e| e.into_inner());
    map.get_or_insert_with(Default::default).insert(aumid.to_string(), px);
}

/// The AUMID slot of an app that sends no Windows notification (flash.rs):
/// "flash:<its exe>". A click starts that exe (it brings its window forward),
/// and Windows' banner settings are never touched for it.
pub const FLASH_PREFIX: &str = "flash:";

/// Keeps the icon of a file (an app's exe) as the logo of `key`. Needs COM
/// on the calling thread.
pub fn keep_file_logo(key: &str, path: &str) {
    if let Some(px) = item_icon(path) {
        LOGOS.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(Default::default).insert(key.to_string(), Some(Arc::new(px)));
    }
}

/// The icon Windows' app list (shell:AppsFolder) shows for an AUMID, as RGBA.
fn shell_icon(aumid: &str) -> Option<Vec<u8>> {
    item_icon(&format!("shell:AppsFolder\\{aumid}"))
}

/// The icon the shell shows for a parsing name (an AppsFolder entry, a file), as RGBA.
fn item_icon(name: &str) -> Option<Vec<u8>> {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::SIZE;
    use windows::Win32::Graphics::Gdi::{DeleteObject, GetDC, GetDIBits, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS};
    use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY};
    unsafe {
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(name), None).ok()?;
        let n = SRC_PX as i32;
        let bmp = factory.GetImage(SIZE { cx: n, cy: n }, SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK).ok()?;
        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: n,
            biHeight: -n,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut px = vec![0u8; (n * n * 4) as usize];
        let dc = GetDC(None);
        let lines = GetDIBits(dc, bmp, 0, n as u32, Some(px.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        ReleaseDC(None, dc);
        let _ = DeleteObject(bmp.into());
        if lines == 0 {
            return None;
        }
        // BGRA, premultiplied → RGBA, straight.
        for c in px.chunks_exact_mut(4) {
            let a = c[3] as u32;
            let (b, g, r) = (c[0] as u32, c[1] as u32, c[2] as u32);
            let un = |v: u32| if a == 0 { 0 } else { (v * 255 / a).min(255) as u8 };
            c[0] = un(r);
            c[1] = un(g);
            c[2] = un(b);
        }
        fit_logo(&px, SRC_PX)
    }
}

fn decode_logo(info: &windows::ApplicationModel::AppInfo) -> windows::core::Result<Vec<u8>> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Graphics::Imaging::BitmapInterpolationMode;
    let size = windows::Foundation::Size { Width: SRC_PX as f32, Height: SRC_PX as f32 };
    let stream = info.DisplayInfo()?.GetLogo(size)?.OpenReadAsync()?.get()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(SRC_PX)?;
    transform.SetScaledHeight(SRC_PX)?;
    // The default is nearest neighbour: jagged edges.
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()?;
    fit_logo(&pixels.DetachPixelData()?.to_vec(), SRC_PX).ok_or_else(|| windows::core::Error::from(windows::Win32::Foundation::E_FAIL))
}

fn read(n: &UserNotification) -> Option<Notif> {
    let info = n.AppInfo().ok()?;
    let app = info.DisplayInfo().and_then(|d| d.DisplayName()).map(|s| s.to_string()).unwrap_or_default();
    let aumid = info.AppUserModelId().map(|s| s.to_string()).unwrap_or_default();
    keep_logo(&aumid, &info);
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
/// Whether Windows lets Kanmuri read notifications (known once the watcher ran).
pub fn has_access() -> bool {
    ACCESS.load(Ordering::Relaxed)
}
static ACCESS: AtomicBool = AtomicBool::new(false);

/// `ready`: called once the watcher reads notifications (access granted).
pub fn start(
    allowed_now: impl Fn() -> Vec<String> + Send + 'static,
    sink: impl Fn(Notif) + Send + 'static,
    ready: impl Fn() + Send + 'static,
) -> Watcher {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::Builder::new()
        .name("kanmuri-notifications".into())
        .spawn(move || {
            let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            run(&flag, allowed_now, sink, ready);
            if com.is_ok() {
                unsafe { CoUninitialize() };
            }
        })
        .ok();
    Watcher { stop }
}

fn run(stop: &AtomicBool, allowed_now: impl Fn() -> Vec<String>, sink: impl Fn(Notif), ready: impl Fn()) {
    let Ok(listener) = UserNotificationListener::Current() else { return };
    let access = listener.GetAccessStatus().unwrap_or(UserNotificationListenerAccessStatus::Unspecified);
    if access != UserNotificationListenerAccessStatus::Allowed {
        // Asking works from any thread for an unpackaged app; Windows shows its prompt once.
        let asked = listener.RequestAccessAsync().and_then(|op| op.get());
        if asked != Ok(UserNotificationListenerAccessStatus::Allowed) {
            crate::store::log("notifications: no access (Settings → Privacy → Notifications)");
            ACCESS.store(false, Ordering::Relaxed);
            return;
        }
    }
    ACCESS.store(true, Ordering::Relaxed);
    ready();
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
        // An app without Windows notifications (WeChat): start its exe again,
        // which brings the running one forward.
        if let Some(exe) = n.aumid.strip_prefix(FLASH_PREFIX).filter(|e| e.to_lowercase().ends_with(".exe")) {
            done = std::process::Command::new(exe).spawn().is_ok();
        }
        if !done && is_packaged(&n.aumid) {
            let args = toast_launch(n.id).unwrap_or_default();
            if let Ok(manager) = CoCreateInstance::<_, IApplicationActivationManager>(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER) {
                done = manager.ActivateApplication(&HSTRING::from(n.aumid.as_str()), &HSTRING::from(args), AO_NONE).is_ok();
            }
        }
        // Anything else (Discord): Windows starts it by its AUMID, which brings
        // a running app forward.
        if !done && !n.aumid.is_empty() && !n.aumid.starts_with(FLASH_PREFIX) && n.aumid.chars().all(|c| !c.is_control() && c != '"') {
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
    fn a_small_logo_in_a_big_margin_fills_the_icon() {
        // A 64 px green square in the middle of a 256 px transparent tile.
        let n = 256u32;
        let mut px = vec![0u8; (n * n * 4) as usize];
        for y in 96..160 {
            for x in 96..160 {
                let i = ((y * n + x) * 4) as usize;
                px[i..i + 4].copy_from_slice(&[0, 200, 80, 255]);
            }
        }
        let out = fit_logo(&px, n).unwrap();
        assert_eq!(out.len(), (LOGO_PX * LOGO_PX * 4) as usize);
        // Corners and centre are now the logo itself, solid and the same colour.
        for &(x, y) in &[(0, 0), (LOGO_PX - 1, LOGO_PX - 1), (LOGO_PX / 2, LOGO_PX / 2)] {
            let i = ((y * LOGO_PX + x) * 4) as usize;
            assert_eq!(&out[i..i + 4], &[0, 200, 80, 255], "at {x},{y}");
        }
        assert!(fit_logo(&vec![0u8; (n * n * 4) as usize], n).is_none(), "all transparent");
    }

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

#[cfg(test)]
mod logo_probe {
    /// The logos of the apps that have notifications now (needs a desktop session).
    #[test]
    #[ignore]
    fn logos_of_current_notifications() {
        use windows::UI::Notifications::Management::UserNotificationListener;
        use windows::UI::Notifications::NotificationKinds;
        let l = UserNotificationListener::Current().unwrap();
        let list = l.GetNotificationsAsync(NotificationKinds::Toast).unwrap().get().unwrap();
        for i in 0..list.Size().unwrap() {
            let n = list.GetAt(i).unwrap();
            if let Some(x) = super::read(&n) {
                println!("{} | {} | logo bytes {}", x.app, x.aumid, super::logo(&x.aumid).map(|p| p.len()).unwrap_or(0));
            }
        }
    }
}
