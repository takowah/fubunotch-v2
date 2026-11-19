// Usage: each account's plan and how much of its limits is used,
// shown under its chip on the Accounts tab (18 Nov 2026: "account
// ama AI usage itu harusnya 1 tab").
//
// - Claude Code: the plan from the account's `.credentials.json`; the usage
//   from api.anthropic.com/api/oauth/usage with its OAuth token, only while
//   the token is still valid. The token is never refreshed, written, logged
//   or kept: an expired one shows the last usage kept with "open Claude Code
//   to refresh". Only the usage (no token) is kept, in %APPDATA%\FubuNotch v2\usage.json.
// - Claude Desktop: its tokens stay in Electron's storage, untouched. Its
//   `config.json` names the account it last signed in with
//   (`lastKnownAccountUuid`); the Claude Code account with the same uuid
//   (`oauthAccount.accountUuid` in its `.claude.json`) lends its plan and usage.
// - Codex: the `rate_limits` Codex writes into its newest session log
//   (`<CODEX_HOME>\sessions\**\*.jsonl`), read from the end of that file.
//
// A sweep runs when the Accounts tab opens (a background thread; no timer),
// and asks the network for an account at most once every five minutes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::accounts::Account;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// The network is asked about an account at most this often.
const FETCH_EVERY_S: i64 = 5 * 60;
/// Usage older than this says how old it is.
const STALE_S: i64 = 5 * 60;
/// Codex's session logs are read from the end, at most this much.
const CODEX_TAIL: u64 = 512 * 1024;
/// Rows shown per account (the page must fit Mini Island's height).
pub const WINDOWS_MAX: usize = 3;

/// One limit window: "5-hour", 32 % used, resets at (unix seconds).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub label: String,
    pub used: f32,
    pub resets_at: Option<i64>,
}

/// An account's plan and limits; `updated` is when the numbers were true (unix s, 0: never).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub plan: String,
    pub windows: Vec<Window>,
    pub updated: i64,
    /// The account these numbers are of (Claude Code's accountUuid). A slot
    /// signed in again as someone else drops them: never one account's usage
    /// under another's name (19 Nov 2026: "misleading").
    pub account: String,
}

#[derive(Default)]
struct Store {
    /// By account id (Claude Code and Codex accounts).
    usage: HashMap<String, Usage>,
    /// Claude Code accounts whose token has run out.
    expired: HashMap<String, bool>,
    /// Claude Desktop account id → the Claude Code account id of the same login.
    linked: HashMap<String, String>,
    /// When the network was last asked about an account (unix s).
    asked: HashMap<String, i64>,
    loaded: bool,
    running: bool,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|e| e.into_inner());
    let s = g.get_or_insert_with(Store::default);
    if !s.loaded {
        s.loaded = true;
        if let Ok(b) = std::fs::read(cache_path()) {
            if let Ok(m) = serde_json::from_slice::<HashMap<String, Usage>>(&b) {
                s.usage = m;
            }
        }
    }
    f(s)
}

fn cache_path() -> PathBuf {
    crate::store::data_dir().join("usage.json")
}

pub fn now_s() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

fn demo() -> bool {
    std::env::var_os("FUBUNOTCH_V2_DEMO_USAGE").is_some()
}

// ── What the page shows ─────────────────────────────────────────────────────

/// One row on the page: "5-hour", 0.32, "32% used", "resets 22:00".
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub label: String,
    pub used: f32,
    pub used_text: String,
    pub resets: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub plan: String,
    pub rows: Vec<Row>,
    /// "Updated 18 min ago", a line per `\n`; empty when fresh.
    pub note: String,
}

/// What to show under an account's chip.
pub fn view(acc: &Account) -> View {
    if demo() {
        return demo_view(&acc.id);
    }
    let now = now_s();
    let offset = (crate::local_time::utc_offset_hours() * 3600.0) as i64;
    with_store(|s| {
        let id = if acc.service == "desktop" { s.linked.get(&acc.id).cloned().unwrap_or_default() } else { acc.id.clone() };
        let Some(u) = s.usage.get(&id) else { return View::default() };
        let expired = s.expired.get(&id).copied().unwrap_or(false);
        let codex = acc.service == "codex";
        make_view(u, expired, codex, now, offset)
    })
}

fn make_view(u: &Usage, expired: bool, codex: bool, now: i64, offset: i64) -> View {
    let rows = u.windows.iter().take(WINDOWS_MAX).map(|w| row(w, now, offset)).collect::<Vec<_>>();
    let age = now - u.updated;
    let mut note = String::new();
    if u.updated > 0 && (age >= STALE_S || expired) {
        note = format!("Updated {}", ago(age));
    }
    if expired && !codex {
        let hint = if u.windows.is_empty() { "Open Claude Code to see usage" } else { "Open Claude Code to refresh" };
        note = if note.is_empty() { hint.to_string() } else { format!("{note}\n{hint}") };
    }
    View { plan: u.plan.clone(), rows, note }
}

fn row(w: &Window, now: i64, offset: i64) -> Row {
    // Past its reset the window starts again from nothing.
    let reset_passed = w.resets_at.is_some_and(|r| r <= now);
    let used = if reset_passed { 0.0 } else { w.used.clamp(0.0, 100.0) };
    let resets = match w.resets_at {
        Some(_) if reset_passed => "reset".to_string(),
        Some(r) => format!("resets {}", when(r, now, offset)),
        None => String::new(),
    };
    Row { label: w.label.clone(), used: used / 100.0, used_text: format!("{}% used", used.round() as i64), resets }
}

/// "22:00" today, "Thu 22:00" within the week, "24 Nov" later (local time, 24-hour).
pub fn when(at: i64, now: i64, offset: i64) -> String {
    const DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let local = at + offset;
    let day = local.div_euclid(86_400);
    let today = (now + offset).div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let hm = format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60);
    if day == today {
        hm
    } else if day > today && day - today < 7 {
        format!("{} {hm}", DAYS[day.rem_euclid(7) as usize])
    } else {
        let (_, m, d) = crate::local_time::add_days(1970, 1, 1, day);
        format!("{d} {}", MONTHS[(m as usize).saturating_sub(1) % 12])
    }
}

/// "just now", "18 min ago", "3 h ago", "2 days ago".
pub fn ago(secs: i64) -> String {
    let m = secs.max(0) / 60;
    match m {
        0 => "just now".into(),
        1..=59 => format!("{m} min ago"),
        60..=2879 => format!("{} h ago", m / 60),
        _ => format!("{} days ago", m / 1440),
    }
}

// ── Reading the sources ─────────────────────────────────────────────────────

/// Claude Code's `rateLimitTier` / `subscriptionType` as a badge: "Max 20x", "Max 5x", "Pro".
pub fn claude_plan(tier: &str, subscription: &str) -> String {
    // A Team or Enterprise seat says so (Claude Desktop shows "Team"), even
    // when its rate-limit tier reads like a Max one.
    let sub = subscription.to_ascii_lowercase();
    if sub == "team" || sub == "enterprise" {
        return title_case(subscription);
    }
    let t = tier.to_ascii_lowercase();
    if t.contains("max_20x") {
        return "Max 20x".into();
    }
    if t.contains("max_5x") {
        return "Max 5x".into();
    }
    title_case(subscription)
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

fn claude_label(key: &str) -> String {
    match key {
        "five_hour" => "5-hour".into(),
        "seven_day" => "Week · all models".into(),
        "seven_day_opus" => "Week · Opus".into(),
        "seven_day_sonnet" => "Week · Sonnet".into(),
        k => match k.strip_prefix("seven_day_") {
            Some(rest) => format!("Week · {}", title_case(&rest.replace('_', " "))),
            None => title_case(&k.replace('_', " ")),
        },
    }
}

/// The usage endpoint's answer: every object with a numeric `utilization`
/// (5-hour first, then the week, then the rest by name); nulls skipped.
pub fn parse_claude_usage(v: &Value) -> Vec<Window> {
    let Some(map) = v.as_object() else { return Vec::new() };
    let mut keys: Vec<&String> = map.keys().collect();
    let rank = |k: &str| match k {
        "five_hour" => 0,
        "seven_day" => 1,
        "seven_day_opus" => 2,
        "seven_day_sonnet" => 3,
        _ => 4,
    };
    keys.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.cmp(b)));
    keys.into_iter()
        .filter_map(|k| {
            let o = map.get(k)?.as_object()?;
            let used = o.get("utilization")?.as_f64()? as f32;
            let resets_at = o.get("resets_at").and_then(|x| x.as_str()).and_then(parse_iso);
            Some(Window { label: claude_label(k), used, resets_at })
        })
        .collect()
}

/// "2026-11-20T15:00:00.123+00:00" / "...Z" → unix seconds.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once('T').or_else(|| s.split_once(' '))?;
    let mut d = date.split('-');
    let (y, mo, da): (i32, u32, u32) = (d.next()?.parse().ok()?, d.next()?.parse().ok()?, d.next()?.parse().ok()?);
    // The zone: Z, +hh:mm or -hh:mm after the time.
    let (time, zone) = match rest.find(['Z', 'z', '+', '-']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().map(|x| x.split('.').next().unwrap_or("0").parse().unwrap_or(0)).unwrap_or(0);
    let off = match zone.chars().next() {
        Some(c @ ('+' | '-')) => {
            let z = &zone[1..];
            let (zh, zm) = z.split_once(':').unwrap_or((z.get(..2)?, z.get(2..).unwrap_or("0")));
            let o = zh.parse::<i64>().ok()? * 3600 + zm.parse::<i64>().unwrap_or(0) * 60;
            if c == '+' { o } else { -o }
        }
        _ => 0,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) {
        return None;
    }
    Some(crate::local_time::days_from_civil(y, mo, da) * 86_400 + h * 3600 + mi * 60 + sec - off)
}

/// A Codex window's length as a label: "5-hour", "Weekly", "30-day".
pub fn codex_label(minutes: i64) -> String {
    match minutes {
        10_080 => "Weekly".into(),
        m if m > 0 && m % 1440 == 0 => format!("{}-day", m / 1440),
        m if m > 0 && m % 60 == 0 => format!("{}-hour", m / 60),
        m => format!("{m}-min"),
    }
}

/// Codex's `rate_limits` object → plan and windows (primary, then secondary).
pub fn parse_codex_limits(rl: &Value) -> Option<Usage> {
    let rl = rl.as_object()?;
    let plan = rl.get("plan_type").and_then(|x| x.as_str()).map(title_case).unwrap_or_default();
    let windows = ["primary", "secondary"]
        .iter()
        .filter_map(|k| {
            let w = rl.get(*k)?.as_object()?;
            let used = w.get("used_percent")?.as_f64()? as f32;
            let minutes = w.get("window_minutes").and_then(|x| x.as_i64()).unwrap_or(0);
            let resets_at = w.get("resets_at").and_then(|x| x.as_i64());
            Some(Window { label: codex_label(minutes), used, resets_at })
        })
        .collect();
    Some(Usage { plan, windows, updated: 0, ..Usage::default() })
}

fn find_rate_limits(v: &Value) -> Option<&Value> {
    match v {
        Value::Object(m) => {
            if let Some(rl) = m.get("rate_limits").filter(|x| x.is_object()) {
                return Some(rl);
            }
            m.values().find_map(find_rate_limits)
        }
        _ => None,
    }
}

/// The last `rate_limits` in a piece of a session log (lines, the first maybe cut).
pub fn last_codex_limits(text: &str) -> Option<Usage> {
    text.lines().rev().filter(|l| l.contains("\"rate_limits\"")).find_map(|l| {
        let v: Value = serde_json::from_str(l).ok()?;
        parse_codex_limits(find_rate_limits(&v)?)
    })
}

/// The newest file in `<codex home>\sessions\YYYY\MM\DD\*.jsonl`: the last
/// year, month and day folders by name, then the newest file there.
fn newest_session(home: &Path) -> Option<PathBuf> {
    fn last_dir(p: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        v.sort();
        v.reverse();
        v
    }
    for y in last_dir(&home.join("sessions")) {
        for m in last_dir(&y) {
            for d in last_dir(&m) {
                let newest = std::fs::read_dir(&d)
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
                    .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                    .max();
                if let Some((_, p)) = newest {
                    return Some(p);
                }
            }
        }
    }
    None
}

fn read_codex(home: &Path) -> Option<Usage> {
    use std::io::{Read, Seek, SeekFrom};
    let path = newest_session(home)?;
    let mut f = std::fs::File::open(&path).ok()?;
    let meta = f.metadata().ok()?;
    let len = meta.len();
    let start = len.saturating_sub(CODEX_TAIL);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.take(CODEX_TAIL).read_to_end(&mut buf).ok()?;
    let mut u = last_codex_limits(&String::from_utf8_lossy(&buf))?;
    u.updated = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some(u)
}

/// A Claude Code account's uuid, from `oauthAccount` in its `.claude.json`:
/// inside the config dir, or, for the default one (%USERPROFILE%\.claude),
/// next to it (%USERPROFILE%\.claude.json; the one inside has no account).
fn account_uuid(dir: &Path) -> Option<String> {
    let read = |p: PathBuf| -> Option<String> {
        let v: Value = serde_json::from_slice(&std::fs::read(p).ok()?).ok()?;
        v.get("oauthAccount")?.get("accountUuid")?.as_str().map(str::to_string)
    };
    read(dir.join(".claude.json")).or_else(|| (dir == home().join(".claude")).then(|| read(home().join(".claude.json"))).flatten())
}

/// Claude Desktop's `config.json`: only `lastKnownAccountUuid` is taken.
fn desktop_uuid(dir: &Path) -> Option<String> {
    let b = std::fs::read(dir.join("config.json")).ok()?;
    let v: Value = serde_json::from_slice(&b).ok()?;
    v.get("lastKnownAccountUuid")?.as_str().map(str::to_string)
}

/// What `.credentials.json` holds that the island needs. The token lives
/// only here, for the one request.
struct Creds {
    plan: String,
    token: String,
    expired: bool,
}

fn read_creds(dir: &Path, now: i64) -> Option<Creds> {
    let b = std::fs::read(dir.join(".credentials.json")).ok()?;
    let v: Value = serde_json::from_slice(&b).ok()?;
    let o = v.get("claudeAiOauth")?;
    let s = |k: &str| o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let expires_ms = o.get("expiresAt").and_then(|x| x.as_i64()).unwrap_or(0);
    Some(Creds { plan: claude_plan(&s("rateLimitTier"), &s("subscriptionType")), token: s("accessToken"), expired: expires_ms / 1000 <= now + 30 })
}

/// GET the usage with the account's token. Errors never carry the token.
fn fetch(token: &str) -> Result<Value, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .user_agent("fubunotch-v2")
        .http_status_as_error(false)
        .build()
        .into();
    let mut r = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let status = r.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("HTTP {status}"));
    }
    let body = r.body_mut().with_config().limit(256 * 1024).read_to_string().map_err(|e| format!("read failed: {e}"))?;
    serde_json::from_str(&body).map_err(|_| "not JSON".to_string())
}

fn dir_of(a: &Account) -> PathBuf {
    if !a.dir.is_empty() {
        return PathBuf::from(&a.dir);
    }
    match a.service.as_str() {
        "claude" => home().join(".claude"),
        "codex" => home().join(".codex"),
        _ => PathBuf::new(),
    }
}

/// Reads every account's plan and usage; asks the network (when `online`)
/// for the Claude Code accounts whose token is valid and whose usage is
/// five minutes old. Blocking: call it off the UI thread (a snapshot calls
/// it inline, offline).
pub fn sweep(accounts: &[Account], online: bool) {
    if demo() {
        return;
    }
    let now = now_s();
    let mut uuids: HashMap<String, String> = HashMap::new();
    let mut changed = false;
    for a in accounts {
        let dir = dir_of(a);
        match a.service.as_str() {
            "claude" => {
                let who = account_uuid(&dir).unwrap_or_default();
                if !who.is_empty() {
                    uuids.insert(who.clone(), a.id.clone());
                }
                let Some(c) = read_creds(&dir, now) else {
                    // Signed out: nothing of whoever was here before.
                    with_store(|s| s.usage.remove(&a.id));
                    continue;
                };
                let (mut due, mut usage) = with_store(|s| {
                    s.expired.insert(a.id.clone(), c.expired);
                    let u = s.usage.get(&a.id).cloned().unwrap_or_default();
                    let asked = s.asked.get(&a.id).copied().unwrap_or(0);
                    (now - u.updated >= FETCH_EVERY_S && now - asked >= FETCH_EVERY_S, u)
                });
                // Another account in this slot now (signed in again): the old
                // numbers go, and the new account is asked at once.
                if usage.account != who {
                    usage = Usage { account: who.clone(), ..Usage::default() };
                    with_store(|s| s.asked.remove(&a.id));
                    due = true;
                    changed = true;
                }
                if usage.plan != c.plan {
                    usage.plan = c.plan.clone();
                    changed = true;
                }
                // An expired token is left alone: Claude Code refreshes its own
                // login, and nothing here writes into another app's files.
                if online && due && !c.expired && !c.token.is_empty() {
                    with_store(|s| s.asked.insert(a.id.clone(), now));
                    match fetch(&c.token) {
                        Ok(v) => {
                            usage.windows = parse_claude_usage(&v);
                            usage.updated = now;
                            changed = true;
                        }
                        Err(e) => crate::store::log(&format!("usage: {} {e}", a.id)),
                    }
                }
                with_store(|s| s.usage.insert(a.id.clone(), usage));
            }
            "codex" => {
                if let Some(u) = read_codex(&dir) {
                    with_store(|s| s.usage.insert(a.id.clone(), u));
                }
            }
            _ => {}
        }
    }
    for a in accounts.iter().filter(|a| a.service == "desktop") {
        if let Some(id) = desktop_uuid(&dir_of(a)).and_then(|u| uuids.get(&u).cloned()) {
            with_store(|s| s.linked.insert(a.id.clone(), id));
        }
    }
    if changed && online {
        save();
    }
}

/// Keeps the Claude Code usage (no tokens; Codex is read from its logs each time).
fn save() {
    let map: HashMap<String, Usage> = with_store(|s| s.usage.iter().filter(|(id, _)| s.expired.contains_key(*id)).map(|(k, v)| (k.clone(), v.clone())).collect());
    let dir = crate::store::data_dir();
    let _ = std::fs::create_dir_all(&dir);
    let tmp = dir.join(format!("usage.json.{}.tmp", std::process::id()));
    if let Ok(b) = serde_json::to_vec_pretty(&map) {
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, cache_path());
        }
    }
}

/// A sweep on a background thread unless one is running; `done` runs after it.
pub fn sweep_in_background(done: impl FnOnce() + Send + 'static) {
    if demo() || with_store(|s| std::mem::replace(&mut s.running, true)) {
        return;
    }
    std::thread::spawn(move || {
        sweep(&crate::accounts::load(), true);
        with_store(|s| s.running = false);
        done();
    });
}

// ── FUBUNOTCH_V2_DEMO_USAGE ──────────────────────────────────────────────────────

fn demo_view(id: &str) -> View {
    let now = now_s();
    let offset = (crate::local_time::utc_offset_hours() * 3600.0) as i64;
    let w = |label: &str, used: f32, in_h: i64| Window { label: label.into(), used, resets_at: Some(now + in_h * 3600 + 1800) };
    let (u, expired) = match id {
        "desktop-team" | "claude-team" => (
            Usage { plan: "Max 20x".into(), windows: vec![w("5-hour", 32.0, 2), w("Week · all models", 58.0, 50), w("Week · Opus", 91.0, 50)], updated: now, ..Usage::default() },
            false,
        ),
        "desktop-alex" | "claude-alex" => (
            Usage { plan: "Max 5x".into(), windows: vec![w("5-hour", 4.0, 4), w("Week · all models", 77.0, 98)], updated: now - 18 * 60, ..Usage::default() },
            true,
        ),
        "codex-main" => (Usage { plan: "Plus".into(), windows: vec![w("5-hour", 12.0, 3), w("Weekly", 35.0, 120)], updated: now - 18 * 60, ..Usage::default() }, false),
        _ => return View::default(),
    };
    make_view(&u, expired, id.starts_with("codex"), now, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_usage_shows_what_is_there() {
        let v: Value = serde_json::from_str(
            r#"{"five_hour":{"utilization":32.0,"resets_at":"2026-11-18T15:00:00.123456+00:00"},
                "seven_day":{"utilization":58,"resets_at":"2026-11-20T15:00:00Z"},
                "seven_day_oauth_apps":null,
                "seven_day_opus":{"utilization":0.0,"resets_at":null},
                "extra_usage":{"is_enabled":false,"monthly_limit":null}}"#,
        )
        .unwrap();
        let w = parse_claude_usage(&v);
        assert_eq!(w.len(), 3);
        assert_eq!(w[0], Window { label: "5-hour".into(), used: 32.0, resets_at: Some(parse_iso("2026-11-18T15:00:00Z").unwrap()) });
        assert_eq!(w[1].label, "Week · all models");
        assert_eq!(w[2].label, "Week · Opus");
        assert_eq!(w[2].resets_at, None);
        assert!(parse_claude_usage(&Value::Null).is_empty());
    }

    #[test]
    fn iso_times() {
        let z = parse_iso("2026-11-18T15:00:00Z").unwrap();
        assert_eq!(z, crate::local_time::days_from_civil(2026, 11, 18) * 86_400 + 15 * 3600);
        assert_eq!(parse_iso("2026-11-18T22:00:00+07:00"), Some(z));
        assert_eq!(parse_iso("2026-11-18T10:00:00.5-05:00"), Some(z));
        assert_eq!(parse_iso("nope"), None);
    }

    #[test]
    fn plans() {
        assert_eq!(claude_plan("default_claude_max_20x", "max"), "Max 20x");
        assert_eq!(claude_plan("default_claude_max_5x", "team"), "Team", "a Team seat, as Claude Desktop says");
        assert_eq!(claude_plan("default_claude_ai", "pro"), "Pro");
    }

    #[test]
    fn codex_line() {
        let log = "{\"cut off\n{\"timestamp\":\"x\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"rate_limits\":{\"limit_id\":\"codex\",\"primary\":{\"used_percent\":35.0,\"window_minutes\":43200,\"resets_at\":1795590968},\"secondary\":{\"used_percent\":3.0,\"window_minutes\":300,\"resets_at\":1795500000},\"plan_type\":\"free\"}}}\n{\"type\":\"other\"}\n";
        let u = last_codex_limits(log).unwrap();
        assert_eq!(u.plan, "Free");
        assert_eq!(u.windows, vec![
            Window { label: "30-day".into(), used: 35.0, resets_at: Some(1795590968) },
            Window { label: "5-hour".into(), used: 3.0, resets_at: Some(1795500000) },
        ]);
        assert_eq!(codex_label(10_080), "Weekly");
        assert!(last_codex_limits("{\"rate_limits\":null}").is_none());
    }

    #[test]
    fn times_read_in_local_time() {
        let now = parse_iso("2026-11-18T08:00:00Z").unwrap(); // Wed 15:00 WIB
        let wib = 7 * 3600;
        assert_eq!(when(parse_iso("2026-11-18T15:00:00Z").unwrap(), now, wib), "22:00");
        assert_eq!(when(parse_iso("2026-11-20T15:00:00Z").unwrap(), now, wib), "Fri 22:00");
        assert_eq!(when(parse_iso("2026-11-30T15:00:00Z").unwrap(), now, wib), "30 Nov");
        assert_eq!(ago(18 * 60 + 5), "18 min ago");
        assert_eq!(ago(3 * 3600), "3 h ago");
    }

    #[test]
    fn stale_and_expired_notes() {
        let u = Usage { plan: "Max 20x".into(), windows: vec![Window { label: "5-hour".into(), used: 32.4, resets_at: Some(10_000) }], updated: 1000, ..Usage::default() };
        let v = make_view(&u, false, false, 1000 + 60, 0);
        assert_eq!(v.note, "");
        assert_eq!(v.rows[0].used_text, "32% used");
        let v = make_view(&u, true, false, 1000 + 18 * 60, 0);
        assert_eq!(v.note, "Updated 18 min ago\nOpen Claude Code to refresh");
        let v = make_view(&u, false, false, 20_000, 0);
        assert_eq!(v.rows[0].used, 0.0, "past the reset");
        assert_eq!(v.rows[0].resets, "reset");
        let none = Usage { plan: "Max 5x".into(), windows: vec![], updated: 0, ..Usage::default() };
        assert_eq!(make_view(&none, true, false, 1, 0).note, "Open Claude Code to see usage");
    }
}
