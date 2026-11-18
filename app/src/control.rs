// Control (as Poni's, like Windows' Quick Settings): the speakers' volume and
// mute, the microphone's mute, which output and microphone are in use, the
// screen's brightness, and the Wi-Fi and Bluetooth radios. Everything here
// blocks a little (COM, WinRT, WMI): call it off the UI thread (main.rs
// `control_job`), never while the island is hidden.
// IPolicyConfig's methods keep Windows' names (its vtable, below).
#![allow(non_snake_case)]

use windows::core::{Interface, GUID, HRESULT, HSTRING, PCWSTR};
use windows::Devices::Radios::{Radio, RadioKind, RadioState};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{eCapture, eCommunications, eConsole, eMultimedia, eRender, EDataFlow, ERole, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE};
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ};

/// An output or microphone Windows has.
#[derive(Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub default: bool,
}

/// What the Control page shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// 0..1, and muted; None without an output.
    pub volume: Option<(f32, bool)>,
    /// The default microphone muted; None without one.
    pub mic_muted: Option<bool>,
    pub outputs: Vec<Device>,
    pub mics: Vec<Device>,
    /// 0..100; None when Windows can't set it (an external screen).
    pub brightness: Option<u32>,
    /// None: no such radio.
    pub wifi: Option<bool>,
    pub bluetooth: Option<bool>,
}

/// A change asked for from the page.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Volume(f32),
    Mute(bool),
    MicMute(bool),
    Output(String),
    Mic(String),
    Brightness(u32),
    Wifi(bool),
    Bluetooth(bool),
}

fn com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL).ok() }
}

fn endpoint(e: &IMMDeviceEnumerator, flow: EDataFlow) -> Option<IAudioEndpointVolume> {
    unsafe { e.GetDefaultAudioEndpoint(flow, eConsole).ok()?.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok() }
}

fn devices(e: &IMMDeviceEnumerator, flow: EDataFlow) -> Vec<Device> {
    unsafe {
        let default = e.GetDefaultAudioEndpoint(flow, eConsole).ok().and_then(|d| d.GetId().ok()).map(|p| {
            let s = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        });
        let Ok(list) = e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        let n = list.GetCount().unwrap_or(0);
        let mut out = Vec::new();
        for i in 0..n {
            let Ok(d) = list.Item(i) else { continue };
            let Ok(p) = d.GetId() else { continue };
            let id = p.to_string().unwrap_or_default();
            CoTaskMemFree(Some(p.0 as *const _));
            let name = d
                .OpenPropertyStore(STGM_READ)
                .ok()
                .and_then(|store| store.GetValue(&PKEY_Device_FriendlyName).ok())
                .and_then(|v| PropVariantToStringAlloc(&v).ok())
                .map(|p| {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const _));
                    s
                })
                .unwrap_or_else(|| "Unknown device".into());
            out.push(Device { default: default.as_deref() == Some(id.as_str()), id, name });
        }
        out
    }
}

fn radio(kind: RadioKind) -> Option<Radio> {
    let list = Radio::GetRadiosAsync().ok()?.get().ok()?;
    (0..list.Size().ok()?).filter_map(|i| list.GetAt(i).ok()).find(|r| r.Kind().ok() == Some(kind))
}

fn radio_on(kind: RadioKind) -> Option<bool> {
    radio(kind).and_then(|r| r.State().ok()).map(|s| s == RadioState::On)
}

/// Runs a short PowerShell script without a window; its trimmed output.
fn ps(script: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The built-in screen's brightness (WMI); None on a desktop's external screen.
fn brightness() -> Option<u32> {
    ps("(Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue | Select-Object -First 1).CurrentBrightness")?.parse().ok()
}

/// Everything the page shows, read now.
pub fn read() -> Snapshot {
    read_with(true)
}

/// As `read`; without the brightness (a WMI call that takes a moment) when
/// `brightness` is false: it comes back None.
pub fn read_with(brightness_too: bool) -> Snapshot {
    com();
    let mut s = Snapshot::default();
    if let Some(e) = enumerator() {
        unsafe {
            if let Some(v) = endpoint(&e, eRender) {
                if let (Ok(level), Ok(mute)) = (v.GetMasterVolumeLevelScalar(), v.GetMute()) {
                    s.volume = Some((level, mute.as_bool()));
                }
            }
            if let Some(v) = endpoint(&e, eCapture) {
                s.mic_muted = v.GetMute().ok().map(|m| m.as_bool());
            }
        }
        s.outputs = devices(&e, eRender);
        s.mics = devices(&e, eCapture);
    }
    s.wifi = radio_on(RadioKind::WiFi);
    s.bluetooth = radio_on(RadioKind::Bluetooth);
    if brightness_too {
        s.brightness = brightness();
    }
    s
}

/// Does what the page asked. Err: a line for the page.
pub fn apply(c: &Command) -> Result<(), String> {
    com();
    let e = || enumerator().ok_or("no audio".to_string());
    unsafe {
        match c {
            Command::Volume(v) => endpoint(&e()?, eRender).ok_or("no output")?.SetMasterVolumeLevelScalar(v.clamp(0.0, 1.0), std::ptr::null()).map_err(|x| x.to_string()),
            Command::Mute(m) => endpoint(&e()?, eRender).ok_or("no output")?.SetMute(*m, std::ptr::null()).map_err(|x| x.to_string()),
            Command::MicMute(m) => endpoint(&e()?, eCapture).ok_or("no microphone")?.SetMute(*m, std::ptr::null()).map_err(|x| x.to_string()),
            Command::Output(id) | Command::Mic(id) => set_default(id),
            Command::Brightness(b) => {
                let b = (*b).min(100);
                ps(&format!(
                    "Get-CimInstance -Namespace root/WMI -ClassName WmiMonitorBrightnessMethods -ErrorAction Stop | Select-Object -First 1 | Invoke-CimMethod -MethodName WmiSetBrightness -Arguments @{{Timeout=0;Brightness=[byte]{b}}} | Out-Null; 'ok'"
                ))
                .filter(|o| o.ends_with("ok"))
                .map(|_| ())
                .ok_or("this screen's brightness can't be set here".into())
            }
            Command::Wifi(on) => set_radio(RadioKind::WiFi, *on),
            Command::Bluetooth(on) => set_radio(RadioKind::Bluetooth, *on),
        }
    }
}

fn set_radio(kind: RadioKind, on: bool) -> Result<(), String> {
    let r = radio(kind).ok_or("no such radio")?;
    let res = r.SetStateAsync(if on { RadioState::On } else { RadioState::Off }).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())?;
    if res == windows::Devices::Radios::RadioAccessStatus::Allowed {
        Ok(())
    } else {
        Err("Windows didn't allow it (Settings > Privacy > Radios)".into())
    }
}

// Windows' own way to pick the default output / microphone (what the volume
// flyout uses): the undocumented IPolicyConfig of CPolicyConfigClient, used
// by every audio switcher. Only SetDefaultEndpoint is called; the methods
// before it are listed only to keep the vtable in order.
#[windows::core::interface("f8679f50-850a-41cf-9c72-430f290290c8")]
unsafe trait IPolicyConfig: windows::core::IUnknown {
    fn GetMixFormat(&self, id: PCWSTR, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn GetDeviceFormat(&self, id: PCWSTR, default: i32, format: *mut *mut core::ffi::c_void) -> HRESULT;
    fn ResetDeviceFormat(&self, id: PCWSTR) -> HRESULT;
    fn SetDeviceFormat(&self, id: PCWSTR, endpoint: *mut core::ffi::c_void, mix: *mut core::ffi::c_void) -> HRESULT;
    fn GetProcessingPeriod(&self, id: PCWSTR, default: i32, a: *mut i64, b: *mut i64) -> HRESULT;
    fn SetProcessingPeriod(&self, id: PCWSTR, period: *mut i64) -> HRESULT;
    fn GetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn SetShareMode(&self, id: PCWSTR, mode: *mut core::ffi::c_void) -> HRESULT;
    fn GetPropertyValue(&self, id: PCWSTR, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> HRESULT;
    fn SetPropertyValue(&self, id: PCWSTR, key: *const core::ffi::c_void, value: *mut core::ffi::c_void) -> HRESULT;
    fn SetDefaultEndpoint(&self, id: PCWSTR, role: ERole) -> HRESULT;
}

const CPOLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);

/// Makes `id` the default for every role (console, multimedia, communications).
fn set_default(id: &str) -> Result<(), String> {
    unsafe {
        let unk: windows::core::IUnknown = CoCreateInstance(&CPOLICY_CONFIG_CLIENT, None, CLSCTX_ALL).map_err(|e| e.to_string())?;
        let policy: IPolicyConfig = unk.cast().map_err(|e| e.to_string())?;
        let h = HSTRING::from(id);
        for role in [eConsole, eMultimedia, eCommunications] {
            policy.SetDefaultEndpoint(PCWSTR(h.as_ptr()), role).ok().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the real PC (no change): there is a snapshot, and whatever
    /// default output it lists is one of the outputs.
    #[test]
    fn reading_changes_nothing_and_lists_the_default() {
        let s = read();
        if let Some((v, _)) = s.volume {
            assert!((0.0..=1.0).contains(&v));
        }
        assert!(s.outputs.iter().filter(|d| d.default).count() <= 1);
    }

    /// Live and put back: the volume up 2 points and back, the microphone's
    /// mute flipped and back, the default output set to itself (the
    /// IPolicyConfig path, no change heard). Never the radios: Wi-Fi off
    /// would cut the network.
    #[test]
    #[ignore]
    fn control_live_round_trip() {
        let before = read_with(false);
        let (v, _) = before.volume.expect("an output");
        let up = (v + 0.02).min(1.0);
        apply(&Command::Volume(up)).unwrap();
        let mid = read_with(false).volume.unwrap().0;
        apply(&Command::Volume(v)).unwrap();
        assert!((mid - up).abs() < 0.011, "volume set: {mid} vs {up}");
        assert!((read_with(false).volume.unwrap().0 - v).abs() < 0.011, "volume back");
        if let Some(m) = before.mic_muted {
            apply(&Command::MicMute(!m)).unwrap();
            let flipped = read_with(false).mic_muted;
            apply(&Command::MicMute(m)).unwrap();
            assert_eq!(flipped, Some(!m), "mic mute flipped");
            assert_eq!(read_with(false).mic_muted, Some(m), "mic mute back");
        }
        let current = before.outputs.iter().find(|d| d.default).expect("a default output").id.clone();
        apply(&Command::Output(current.clone())).unwrap();
        assert_eq!(read_with(false).outputs.iter().find(|d| d.default).map(|d| d.id.clone()), Some(current));
        if let Some(b) = brightness() {
            let to = if b < 100 { b + 1 } else { b - 1 };
            apply(&Command::Brightness(to)).unwrap();
            let mid = brightness();
            apply(&Command::Brightness(b)).unwrap();
            assert_eq!(mid, Some(to), "brightness set");
            assert_eq!(brightness(), Some(b), "brightness back");
            println!("brightness {b} -> {to} -> {b}");
        }
        println!("round trip ok: volume {v} -> {up} -> {v}, mic {:?}", before.mic_muted);
    }

    /// Prints what this PC has (cargo test -p kanmuri control_probe -- --ignored --nocapture).
    #[test]
    #[ignore]
    fn control_probe() {
        println!("{:#?}", read());
    }
}
