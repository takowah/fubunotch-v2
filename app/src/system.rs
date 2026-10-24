// The System module: short status pop-ups for volume, Caps Lock, the keyboard
// layout, the charger, the network and the audio output. All event-driven:
// - volume: the default output's IAudioEndpointVolumeCallback;
// - audio: IMMNotificationClient: the default output changing (the volume
//   follows it) and devices coming or going, of which headphones and headsets
//   (wired or Bluetooth) are told, with a Bluetooth battery when Windows knows
//   it — as v1 did;
// - charger: PowerManager.PowerSupplyStatusChanged;
// - network: NetworkInformation.NetworkStatusChanged;
// - Caps Lock and layout: a low-level keyboard hook on a thread of its own,
//   which looks only at Caps Lock and at the layout after a key: it never
//   keeps or sends what is typed.
// Every event becomes an `Event` for the UI thread, which checks the user's
// switches before showing it.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::Mutex;

use windows::core::{implement, PCWSTR};
use windows::Networking::Connectivity::NetworkInformation;
use windows::System::Power::{PowerManager, PowerSupplyStatus};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::Globalization::{GetLocaleInfoEx, LCIDToLocaleName, LOCALE_ALLOW_NEUTRAL_NAMES, LOCALE_SLOCALIZEDDISPLAYNAME};
use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl};
use windows::Win32::Media::Audio::{
    eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator,
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_NOTPRESENT, DEVICE_STATE_UNPLUGGED,
    IMMEndpoint, PKEY_AudioEndpoint_FormFactor,
};
use windows::core::Interface;
use windows::Win32::Devices::FunctionDiscovery::{PKEY_DeviceInterface_FriendlyName, PKEY_Device_ContainerId};
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToGUID, PropVariantToStringAlloc, PropVariantToUInt32};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, GetKeyboardLayout, VK_CAPITAL};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// 0..1, and muted.
    Volume(f32, bool),
    CapsLock(bool),
    /// The layout's display name: "English (United States)".
    Layout(String),
    /// Charging, and the battery's percent when there is one.
    Power(bool, Option<i32>),
    /// The connection's name, or None when offline.
    Network(Option<String>),
    /// Headphones or a headset came or went: its name ("" for the computer's
    /// own jack), connected, and a Bluetooth battery when Windows knows it.
    Headphones(String, bool, Option<u8>),
}

type Sink = Box<dyn Fn(Event) + Send + Sync>;
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

fn send(e: Event) {
    if let Some(sink) = SINK.lock().unwrap_or_else(|x| x.into_inner()).as_ref() {
        sink(e);
    }
}

// ── Audio ─────────────────────────────────────────────────────────────────────

#[implement(IAudioEndpointVolumeCallback)]
struct VolumeWatch;

impl IAudioEndpointVolumeCallback_Impl for VolumeWatch_Impl {
    fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
        if let Some(d) = unsafe { data.as_ref() } {
            send(Event::Volume(d.fMasterVolume, d.bMuted.as_bool()));
        }
        Ok(())
    }
}

#[implement(IMMNotificationClient)]
struct DeviceWatch;

impl IMMNotificationClient_Impl for DeviceWatch_Impl {
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> windows::core::Result<()> {
        // Core Audio must not be called back from in here: look on a thread.
        let id = unsafe { id.to_string() }.unwrap_or_default();
        std::thread::spawn(move || device_changed(id, state));
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> windows::core::Result<()> {
        if flow == eRender && role == eMultimedia {
            // The volume callback belongs to the old device: follow the new one.
            std::thread::spawn(watch_volume);
        }
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
        Ok(())
    }
}

struct Audio {
    enumerator: IMMDeviceEnumerator,
    endpoint: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    _devices: IMMNotificationClient,
}

// The COM objects are used from whichever thread a callback lands on (MTA).
unsafe impl Send for Audio {}
static AUDIO: Mutex<Option<Audio>> = Mutex::new(None);

/// (Re)registers the volume callback on the current default output.
fn watch_volume() {
    let mut guard = AUDIO.lock().unwrap_or_else(|x| x.into_inner());
    let Some(audio) = guard.as_mut() else { return };
    unsafe {
        if let Some((old, cb)) = audio.endpoint.take() {
            let _ = old.UnregisterControlChangeNotify(&cb);
        }
        let Ok(device) = audio.enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else { return };
        let Ok(volume) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) else { return };
        let callback: IAudioEndpointVolumeCallback = VolumeWatch.into();
        if volume.RegisterControlChangeNotify(&callback).is_ok() {
            audio.endpoint = Some((volume, callback));
        }
    }
}

// ── Headphones (after v1's devicestatus) ──────────────────────────────────────

/// Headphones (3) and headsets (5), in Core Audio's EndpointFormFactor.
const HEADPHONES: u32 = 3;
const HEADSET: u32 = 5;
/// The computer's own devices share this container id.
const OWN_CONTAINER: windows::core::GUID = windows::core::GUID::from_u128(0x00000000_0000_0000_ffff_ffffffffffff);
/// A Bluetooth headset is two outputs (stereo and hands-free) that come and go
/// together: one notice per device and direction within this long.
const SAME_NEWS: std::time::Duration = std::time::Duration::from_secs(4);
static RECENT: Mutex<Vec<(String, bool, std::time::Instant)>> = Mutex::new(Vec::new());

/// True when `key` going `connected` is news.
fn fresh(key: &str, connected: bool) -> bool {
    let now = std::time::Instant::now();
    let mut seen = RECENT.lock().unwrap_or_else(|x| x.into_inner());
    seen.retain(|(_, _, at)| now.duration_since(*at) < SAME_NEWS);
    if seen.iter().any(|(k, c, _)| k == key && *c == connected) {
        return false;
    }
    seen.push((key.to_string(), connected, now));
    true
}

fn device_changed(id: String, state: DEVICE_STATE) {
    let connected = state == DEVICE_STATE_ACTIVE;
    if !connected && state != DEVICE_STATE_UNPLUGGED && state != DEVICE_STATE_NOTPRESENT {
        return; // turned off by the user: not a connection
    }
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let Some((name, key, container)) = headphones(&enumerator, &id) else { return };
        if !fresh(&key, connected) {
            return;
        }
        let battery = if connected { container.and_then(bluetooth_battery) } else { None };
        send(Event::Headphones(name, connected, battery));
    }
}

/// The output `id` if it is headphones or a headset: its name ("" for the
/// computer's own jack), a key for one notice per device, its container.
unsafe fn headphones(enumerator: &IMMDeviceEnumerator, id: &str) -> Option<(String, String, Option<windows::core::GUID>)> {
    unsafe {
        let device = enumerator.GetDevice(&windows::core::HSTRING::from(id)).ok()?;
        if device.cast::<IMMEndpoint>().ok()?.GetDataFlow().ok()? != eRender {
            return None;
        }
        let store = device.OpenPropertyStore(STGM_READ).ok()?;
        let form = prop_u32(&store, &PKEY_AudioEndpoint_FormFactor)?;
        if form != HEADPHONES && form != HEADSET {
            return None;
        }
        let container = prop_guid(&store, &PKEY_Device_ContainerId).filter(|c| *c != OWN_CONTAINER);
        // A Bluetooth or USB device's interface is named after it; the
        // computer's jack after the sound card, so it is just "Headphones".
        let name = match container {
            Some(_) => prop_string(&store, &PKEY_DeviceInterface_FriendlyName).unwrap_or_default(),
            None => String::new(),
        };
        let key = match container {
            Some(c) => format!("{c:?}"),
            None => id.to_string(),
        };
        Some((name, key, container))
    }
}

/// The battery Windows shows for a Bluetooth device, by its container (the
/// device property {104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2). Best effort:
/// many devices never report one.
fn bluetooth_battery(c: windows::core::GUID) -> Option<u8> {
    use windows::Devices::Enumeration::{DeviceInformation, DeviceInformationKind};
    use windows::Foundation::IReference;
    use windows::core::{Interface, HSTRING};
    const BATTERY: &str = "{104EA319-6EE2-4701-BD47-8DDBF425BBE5} 2";
    let id = format!(
        "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        c.data1, c.data2, c.data3, c.data4[0], c.data4[1], c.data4[2], c.data4[3], c.data4[4], c.data4[5], c.data4[6], c.data4[7]
    );
    let aqs = HSTRING::from(format!("System.Devices.ContainerId:=\"{{{id}}}\""));
    let wanted: windows_collections::IIterable<HSTRING> = vec![HSTRING::from(BATTERY)].into();
    let list = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(&aqs, &wanted, DeviceInformationKind::Device)
        .ok()?
        .get()
        .ok()?;
    let key = HSTRING::from(BATTERY);
    for i in 0..list.Size().unwrap_or(0) {
        let Ok(info) = list.GetAt(i) else { continue };
        let Ok(props) = info.Properties() else { continue };
        let Ok(value) = props.Lookup(&key) else { continue };
        if let Ok(level) = value.cast::<IReference<u8>>().and_then(|r| r.Value()) {
            return Some(level.min(100));
        }
    }
    None
}

unsafe fn prop_u32(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<u32> {
    unsafe {
        let mut v = store.GetValue(key).ok()?;
        let out = PropVariantToUInt32(&v).ok();
        let _ = PropVariantClear(&mut v);
        out
    }
}

unsafe fn prop_guid(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<windows::core::GUID> {
    unsafe {
        let mut v = store.GetValue(key).ok()?;
        let out = PropVariantToGUID(&v).ok();
        let _ = PropVariantClear(&mut v);
        out
    }
}

unsafe fn prop_string(store: &IPropertyStore, key: &PROPERTYKEY) -> Option<String> {
    unsafe {
        let mut v = store.GetValue(key).ok()?;
        let out = PropVariantToStringAlloc(&v).ok().map(|p| {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        });
        let _ = PropVariantClear(&mut v);
        out
    }
}

fn start_audio() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else { return };
        let devices: IMMNotificationClient = DeviceWatch.into();
        let _ = enumerator.RegisterEndpointNotificationCallback(&devices);
        *AUDIO.lock().unwrap_or_else(|x| x.into_inner()) = Some(Audio { enumerator, endpoint: None, _devices: devices });
    }
    watch_volume();
}

// ── Power and network ─────────────────────────────────────────────────────────

fn power_now() -> Event {
    let charging = PowerManager::PowerSupplyStatus().map(|s| s != PowerSupplyStatus::NotPresent).unwrap_or(false);
    let percent = PowerManager::RemainingChargePercent().ok().filter(|p| (0..=100).contains(p));
    Event::Power(charging, percent)
}

fn network_now() -> Event {
    let profile = NetworkInformation::GetInternetConnectionProfile().ok();
    Event::Network(profile.and_then(|p| p.ProfileName().ok()).map(|n| n.to_string()))
}

fn start_power_and_network() {
    let _ = PowerManager::PowerSupplyStatusChanged(&windows::Foundation::EventHandler::new(|_, _| {
        send(power_now());
        Ok(())
    }));
    let _ = NetworkInformation::NetworkStatusChanged(&windows::Networking::Connectivity::NetworkStatusChangedEventHandler::new(|_| {
        send(network_now());
        Ok(())
    }));
}

// ── Keyboard: Caps Lock and layout ────────────────────────────────────────────

static LAST_LAYOUT: AtomicIsize = AtomicIsize::new(0);

fn layout_name(hkl: isize) -> String {
    let lcid = (hkl as usize & 0xffff) as u32;
    let mut locale = [0u16; 85];
    unsafe {
        let n = LCIDToLocaleName(lcid, Some(&mut locale), LOCALE_ALLOW_NEUTRAL_NAMES);
        if n == 0 {
            return format!("{lcid:04x}");
        }
        let mut name = [0u16; 128];
        let m = GetLocaleInfoEx(PCWSTR(locale.as_ptr()), LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut name));
        if m <= 1 {
            return String::from_utf16_lossy(&locale[..(n as usize).saturating_sub(1)]);
        }
        String::from_utf16_lossy(&name[..m as usize - 1])
    }
}

fn foreground_layout() -> isize {
    unsafe {
        let thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        GetKeyboardLayout(thread).0 as isize
    }
}

/// Caps Lock as we last knew it, and whether the key is held (auto-repeat).
static CAPS_ON: AtomicBool = AtomicBool::new(false);
static CAPS_DOWN: AtomicBool = AtomicBool::new(false);

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let key = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        let msg = wparam.0 as u32;
        let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
        if key.vkCode == VK_CAPITAL.0 as u32 {
            // The hook thread's own key state isn't reliable: count the presses.
            if down && !CAPS_DOWN.swap(true, Ordering::Relaxed) {
                let on = !CAPS_ON.fetch_xor(true, Ordering::Relaxed);
                send(Event::CapsLock(on));
            } else if !down {
                CAPS_DOWN.store(false, Ordering::Relaxed);
            }
        }
    }
    if code >= 0 && wparam.0 as u32 == WM_KEYUP {
        // A shortcut (Win+Space, Alt+Shift) may have switched the layout.
        let layout = foreground_layout();
        if layout != 0 && LAST_LAYOUT.swap(layout, Ordering::Relaxed) != layout {
            send(Event::Layout(layout_name(layout)));
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

fn start_keyboard() {
    std::thread::Builder::new()
        .name("fubunotch-keyboard".into())
        .spawn(|| unsafe {
            LAST_LAYOUT.store(foreground_layout(), Ordering::Relaxed);
            CAPS_ON.store(GetKeyState(VK_CAPITAL.0 as i32) & 1 == 1, Ordering::Relaxed);
            let Ok(_hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) else { return };
            // A low-level hook needs its thread to pump messages.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        })
        .ok();
}

/// Starts every watcher; `sink` gets each change (on some Windows thread).
pub fn start(sink: impl Fn(Event) + Send + Sync + 'static) {
    *SINK.lock().unwrap_or_else(|x| x.into_inner()) = Some(Box::new(sink));
    std::thread::spawn(|| {
        start_audio();
        start_power_and_network();
    });
    start_keyboard();
}

/// The words the island shows for an event, and a 0..1 bar for the volume.
pub fn describe(e: &Event) -> (String, Option<f32>) {
    match e {
        Event::Volume(_, true) => ("Muted".into(), Some(0.0)),
        Event::Volume(v, false) => (format!("Volume {}%", (v * 100.0).round() as i32), Some(*v)),
        Event::CapsLock(on) => (format!("Caps Lock {}", if *on { "on" } else { "off" }), None),
        Event::Layout(name) => (format!("Keyboard · {name}"), None),
        Event::Power(true, Some(p)) => (format!("Charging · {p}%"), None),
        Event::Power(true, None) => ("Charging".into(), None),
        Event::Power(false, Some(p)) => (format!("On battery · {p}%"), None),
        Event::Power(false, None) => ("On battery".into(), None),
        Event::Network(Some(name)) => (format!("Connected · {name}"), None),
        Event::Network(None) => ("No internet".into(), None),
        Event::Headphones(name, connected, battery) => (headphones_text(name, *connected, *battery), None),
    }
}

/// v1's words: "AirBuds · Connected · 80%" / "Headphones · Disconnected".
pub fn headphones_text(name: &str, connected: bool, battery: Option<u8>) -> String {
    let name = if name.trim().is_empty() { "Headphones" } else { name.trim() };
    match (connected, battery) {
        (true, Some(p)) => format!("{name} · Connected · {p}%"),
        (true, None) => format!("{name} · Connected"),
        (false, _) => format!("{name} · Disconnected"),
    }
}

/// The icon the capsule shows beside an event (island.slint's SysIcon).
pub fn icon(e: &Event) -> &'static str {
    match e {
        Event::Volume(_, true) => "volume-off",
        Event::Volume(v, false) if *v <= 0.0 => "volume-off",
        Event::Volume(..) => "volume",
        Event::CapsLock(true) => "caps-on",
        Event::CapsLock(false) => "caps-off",
        Event::Layout(_) => "keyboard",
        Event::Power(true, _) => "bolt",
        Event::Power(false, _) => "battery",
        Event::Network(Some(_)) => "wifi",
        Event::Network(None) => "wifi-off",
        Event::Headphones(_, true, _) => "headphones",
        Event::Headphones(_, false, _) => "headphones-off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headset_is_told_once_per_direction() {
        assert!(fresh("bt-1", true), "first time: news");
        assert!(!fresh("bt-1", true), "its second output: not news");
        assert!(fresh("bt-1", false), "the other direction is news");
    }

    #[test]
    fn events_read_as_short_lines() {
        assert_eq!(describe(&Event::Volume(0.456, false)), ("Volume 46%".into(), Some(0.456)));
        assert_eq!(describe(&Event::Volume(0.4, true)).0, "Muted");
        assert_eq!(describe(&Event::CapsLock(true)).0, "Caps Lock on");
        assert_eq!(describe(&Event::Power(true, Some(70))).0, "Charging · 70%");
        assert_eq!(describe(&Event::Network(None)).0, "No internet");
        assert_eq!(headphones_text("AirBuds Pro", true, Some(80)), "AirBuds Pro · Connected · 80%");
        assert_eq!(headphones_text("", true, None), "Headphones · Connected");
        assert_eq!(headphones_text("AirBuds Pro", false, Some(80)), "AirBuds Pro · Disconnected");
        assert_eq!(icon(&Event::Headphones(String::new(), false, None)), "headphones-off");
    }

    #[test]
    fn a_layout_has_a_readable_name() {
        // 0x0409 = English (United States).
        assert!(layout_name(0x0409).contains("English"), "{}", layout_name(0x0409));
    }
}
