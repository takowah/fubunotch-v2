// The Music module: what any player on Windows is playing (the system media
// controls, GSMTC: Spotify, browsers, YouTube Music...). Event-driven: Windows
// calls us when the session, the song, the play state or the position changes;
// each time a snapshot goes to the UI thread. Commands (play/pause, skip) run
// on a short-lived thread so the UI never waits on the player.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::Ref;
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Media::MediaPlaybackAutoRepeatMode as RepeatMode;

/// Artwork is scaled to this many pixels a side (shown at 52 px, sharp at 2× scale).
pub const ART_PX: u32 = 112;

/// What is playing, as the UI needs it. Plain data, so it crosses threads.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub has_session: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub app: String,
    pub playing: bool,
    /// Seconds into the song at `at_unix_ms`.
    pub position: f64,
    pub duration: f64,
    pub at_unix_ms: i64,
    /// RGBA, ART_PX × ART_PX, when the player gives a picture.
    pub art: Option<Vec<u8>>,
    /// None when the player doesn't let us change it.
    pub shuffle: Option<bool>,
    /// None when the player doesn't let us change it.
    pub repeat: Option<Repeat>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    One,
    All,
}

impl Snapshot {
    /// Seconds into the song now.
    pub fn position_now(&self) -> f64 {
        let mut p = self.position;
        if self.playing {
            p += (now_unix_ms() - self.at_unix_ms).max(0) as f64 / 1000.0;
        }
        if self.duration > 0.0 {
            p = p.min(self.duration);
        }
        p.max(0.0)
    }
}

pub fn now_unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Windows FILETIME-style 100 ns ticks since 1601 → Unix ms.
fn ticks_to_unix_ms(ticks: i64) -> i64 {
    ticks / 10_000 - 11_644_473_600_000
}

struct Watch {
    manager: Manager,
    session: Option<(Session, [i64; 3])>,
    /// The artwork of the song it was read for, so it is decoded once a song.
    art_for: String,
    art: Option<Vec<u8>>,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);
type Sink = Box<dyn Fn(Snapshot) + Send + Sync>;
static SINK: Mutex<Option<Sink>> = Mutex::new(None);

fn send(snapshot: Snapshot) {
    if let Some(sink) = SINK.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        sink(snapshot);
    }
}

/// Starts watching; `sink` gets every new snapshot (on a Windows thread).
pub fn start(sink: impl Fn(Snapshot) + Send + Sync + 'static) {
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Box::new(sink));
    std::thread::spawn(|| {
        let manager = match Manager::RequestAsync().and_then(|op| op.get()) {
            Ok(m) => m,
            Err(err) => {
                eprintln!("music: no media controls: {err}");
                return;
            }
        };
        let changed = TypedEventHandler::new(|_: Ref<Manager>, _| {
            follow_current_session();
            Ok(())
        });
        let _ = manager.CurrentSessionChanged(&changed);
        *WATCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(Watch { manager, session: None, art_for: String::new(), art: None });
        follow_current_session();
    });
}

/// Stops watching (the module was turned off).
pub fn stop() {
    let mut guard = WATCH.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(watch) = guard.as_mut() {
        if let Some((s, tokens)) = watch.session.take() {
            let _ = s.RemoveMediaPropertiesChanged(tokens[0]);
            let _ = s.RemovePlaybackInfoChanged(tokens[1]);
            let _ = s.RemoveTimelinePropertiesChanged(tokens[2]);
        }
    }
    *guard = None;
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Hooks the events of whichever session Windows calls current now.
fn follow_current_session() {
    let mut guard = WATCH.lock().unwrap_or_else(|e| e.into_inner());
    let Some(watch) = guard.as_mut() else { return };
    if let Some((old, tokens)) = watch.session.take() {
        let _ = old.RemoveMediaPropertiesChanged(tokens[0]);
        let _ = old.RemovePlaybackInfoChanged(tokens[1]);
        let _ = old.RemoveTimelinePropertiesChanged(tokens[2]);
    }
    if let Ok(session) = watch.manager.GetCurrentSession() {
        let on_change = || {
            std::thread::spawn(publish);
        };
        let t1 = session.MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
            on_change();
            Ok(())
        }));
        let t2 = session.PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
            on_change();
            Ok(())
        }));
        let t3 = session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_, _| {
            on_change();
            Ok(())
        }));
        if let (Ok(a), Ok(b), Ok(c)) = (t1, t2, t3) {
            watch.session = Some((session, [a, b, c]));
        }
    }
    drop(guard);
    std::thread::spawn(publish);
}

/// Reads the current session and sends a snapshot.
fn publish() {
    let session = WATCH.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|w| w.session.as_ref().map(|(s, _)| s.clone()));
    let Some(session) = session else {
        send(Snapshot::default());
        return;
    };
    let mut snap = Snapshot { has_session: true, ..Default::default() };
    snap.app = session.SourceAppUserModelId().map(|s| app_name(&s.to_string())).unwrap_or_default();
    let props = session.TryGetMediaPropertiesAsync().and_then(|op| op.get());
    if let Ok(p) = &props {
        snap.title = p.Title().map(|s| s.to_string()).unwrap_or_default();
        snap.artist = p.Artist().map(|s| s.to_string()).unwrap_or_default();
        snap.album = p.AlbumTitle().map(|s| s.to_string()).unwrap_or_default();
    }
    if let Ok(info) = session.GetPlaybackInfo() {
        snap.playing = info.PlaybackStatus().map(|s| s == Status::Playing).unwrap_or(false);
        let controls = info.Controls().ok();
        let can = |f: fn(&windows::Media::Control::GlobalSystemMediaTransportControlsSessionPlaybackControls) -> windows::core::Result<bool>| {
            controls.as_ref().and_then(|c| f(c).ok()).unwrap_or(false)
        };
        if can(|c| c.IsShuffleEnabled()) {
            snap.shuffle = Some(info.IsShuffleActive().and_then(|r| r.Value()).unwrap_or(false));
        }
        if can(|c| c.IsRepeatEnabled()) {
            snap.repeat = Some(match info.AutoRepeatMode().and_then(|r| r.Value()) {
                Ok(RepeatMode::Track) => Repeat::One,
                Ok(RepeatMode::List) => Repeat::All,
                _ => Repeat::Off,
            });
        }
    }
    if let Ok(t) = session.GetTimelineProperties() {
        snap.position = t.Position().map(|d| d.Duration as f64 / 1e7).unwrap_or(0.0);
        snap.duration = t.EndTime().map(|d| d.Duration as f64 / 1e7).unwrap_or(0.0);
        snap.at_unix_ms = t.LastUpdatedTime().map(|d| ticks_to_unix_ms(d.UniversalTime)).unwrap_or_else(|_| now_unix_ms());
    }
    if snap.title.is_empty() {
        snap.has_session = false;
    }

    // Artwork: decoded once a song.
    let key = format!("{}\u{1f}{}\u{1f}{}", snap.title, snap.artist, snap.album);
    let cached = {
        let guard = WATCH.lock().unwrap_or_else(|e| e.into_inner());
        guard.as_ref().filter(|w| w.art_for == key).map(|w| w.art.clone())
    };
    snap.art = match cached {
        Some(art) => art,
        None => {
            let art = props.ok().and_then(|p| p.Thumbnail().ok()).and_then(|t| decode_art(&t).ok());
            if let Some(w) = WATCH.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                w.art_for = key;
                w.art = art.clone();
            }
            art
        }
    };
    send(snap);
}

fn decode_art(thumbnail: &windows::Storage::Streams::IRandomAccessStreamReference) -> windows::core::Result<Vec<u8>> {
    let stream = thumbnail.OpenReadAsync()?.get()?;
    let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
    let transform = BitmapTransform::new()?;
    transform.SetScaledWidth(ART_PX)?;
    transform.SetScaledHeight(ART_PX)?;
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &transform,
            ExifOrientationMode::IgnoreExifOrientation,
            ColorManagementMode::DoNotColorManage,
        )?
        .get()?;
    Ok(pixels.DetachPixelData()?.to_vec())
}

/// "Spotify.exe" / "308046B0AF4A39CB" / "Chrome" → a short name.
fn app_name(aumid: &str) -> String {
    let last = aumid.rsplit(['\\', '!']).next().unwrap_or(aumid);
    let name = last.trim_end_matches(".exe").trim_end_matches(".EXE");
    match name.to_lowercase().as_str() {
        "spotify" => "Spotify".into(),
        "chrome" => "Chrome".into(),
        "msedge" => "Edge".into(),
        "firefox" => "Firefox".into(),
        _ if aumid.contains("Spotify") => "Spotify".into(),
        _ if aumid.contains("ZuneMusic") => "Media Player".into(),
        _ => name.to_string(),
    }
}

/// "play-pause", "next", "previous", "shuffle", "repeat" (off → all → one →
/// off): sent to the current player.
pub fn command(what: &str) {
    let what = what.to_string();
    std::thread::spawn(move || {
        let session = WATCH.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|w| w.session.as_ref().map(|(s, _)| s.clone()));
        let Some(s) = session else { return };
        let _ = match what.as_str() {
            "play-pause" => s.TryTogglePlayPauseAsync().and_then(|op| op.get()),
            "next" => s.TrySkipNextAsync().and_then(|op| op.get()),
            "previous" => s.TrySkipPreviousAsync().and_then(|op| op.get()),
            "shuffle" => {
                let on = s.GetPlaybackInfo().and_then(|i| i.IsShuffleActive()).and_then(|r| r.Value()).unwrap_or(false);
                s.TryChangeShuffleActiveAsync(!on).and_then(|op| op.get())
            }
            "repeat" => {
                let mode = s.GetPlaybackInfo().and_then(|i| i.AutoRepeatMode()).and_then(|r| r.Value()).unwrap_or(RepeatMode::None);
                let next = match mode {
                    RepeatMode::None => RepeatMode::List,
                    RepeatMode::List => RepeatMode::Track,
                    _ => RepeatMode::None,
                };
                s.TryChangeAutoRepeatModeAsync(next).and_then(|op| op.get())
            }
            _ => return,
        };
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_runs_while_playing_and_stops_at_the_end() {
        let at = now_unix_ms() - 2_000;
        let playing = Snapshot { playing: true, position: 10.0, duration: 200.0, at_unix_ms: at, ..Default::default() };
        assert!((playing.position_now() - 12.0).abs() < 0.2);
        let paused = Snapshot { playing: false, ..playing.clone() };
        assert_eq!(paused.position_now(), 10.0);
        let near_end = Snapshot { position: 199.5, ..playing };
        assert_eq!(near_end.position_now(), 200.0);
    }

    #[test]
    fn app_names_are_short() {
        assert_eq!(app_name("Spotify.exe"), "Spotify");
        assert_eq!(app_name("Chrome"), "Chrome");
        assert_eq!(app_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Media Player");
    }

    #[test]
    fn ticks_convert_to_unix_time() {
        // 2026-01-01T00:00:00Z
        assert_eq!(ticks_to_unix_ms(134_116_992_000_000_000), 1_767_225_600_000);
    }
}
