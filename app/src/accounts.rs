// Accounts: the Claude Desktop, Claude Code and Codex accounts, and the switch
// between them. FubuNotch v2 does the switching itself (it used to ask NotchDeck, an
// AccountDeck fork, which is no longer needed); the accounts stay where
// NotchDeck kept them, %USERPROFILE%\.accountdeck (state.json and one folder per
// account), so nothing has to be set up again.
//
// How a switch works (as AccountDeck does it):
//  - Claude Code / Codex: each account has its own config folder. The active one
//    goes in the user variable CLAUDE_CONFIG_DIR / CODEX_HOME (the Main account
//    removes it: %USERPROFILE%\.claude / .codex). New terminals pick it up.
//  - Claude Desktop: its login lives in its data folder (cookie database and
//    config.json). Switching closes Claude, saves those files into the current
//    account's folder, puts the chosen account's back and opens Claude again.
//    "Local State" (the key that unlocks them on this Windows account) stays.
// Nothing is sent anywhere; tokens are never read out of their files.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: String,
    pub service: String,
    pub name: String,
    pub active: bool,
    /// Its config dir (CLAUDE_CONFIG_DIR, CODEX_HOME, Claude Desktop's saved login); empty: the default.
    pub dir: String,
}

/// The services shown, in order, with their titles.
pub const SERVICES: [(&str, &str); 3] = [("desktop", "Claude Desktop"), ("claude", "Claude Code"), ("codex", "Codex")];

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

/// Where the accounts live: %USERPROFILE%\.accountdeck.
fn root() -> PathBuf {
    home().join(".accountdeck")
}

fn state_path() -> PathBuf {
    root().join("state.json")
}

/// FUBUNOTCH_V2_DEMO_ACCOUNTS: made-up accounts, for snapshots of the Accounts tab.
fn demo() -> bool {
    std::env::var_os("FUBUNOTCH_V2_DEMO_ACCOUNTS").is_some()
}

/// The Accounts tab shows once there are accounts to switch between.
pub fn installed() -> bool {
    demo() || state_path().exists()
}

/// The accounts in their saved order; empty when none are set up.
pub fn load() -> Vec<Account> {
    if demo() {
        return parse(DEMO);
    }
    std::fs::read(state_path()).map(|b| parse(&b)).unwrap_or_default()
}

const DEMO: &[u8] = br#"{"accounts":[
    {"id":"desktop-alex","service":"desktop","name":"Alex Max"},
    {"id":"desktop-team","service":"desktop","name":"Team Ultra"},
    {"id":"claude-team","service":"claude","name":"Studio Ultra"},
    {"id":"claude-alex","service":"claude","name":"Alex Max"},
    {"id":"codex-main","service":"codex","name":"Main"}],
    "active":{"desktop":"desktop-team","claude":"claude-team","codex":"codex-main"}}"#;

pub fn parse(bytes: &[u8]) -> Vec<Account> {
    let Ok(v) = serde_json::from_slice::<Value>(bytes) else { return Vec::new() };
    let active = v.get("active");
    let Some(list) = v.get("accounts").and_then(|x| x.as_array()) else { return Vec::new() };
    list.iter()
        .filter_map(|a| {
            let id = a.get("id")?.as_str()?.to_string();
            let service = a.get("service")?.as_str()?.to_string();
            if !SERVICES.iter().any(|(s, _)| *s == service) {
                return None;
            }
            let name = a.get("name").and_then(|x| x.as_str()).unwrap_or(&id).to_string();
            let is_active = active.and_then(|m| m.get(&service)).and_then(|x| x.as_str()) == Some(id.as_str());
            let dir = a.get("dir").and_then(|x| x.as_str()).unwrap_or("").to_string();
            Some(Account { id, service, name, active: is_active, dir })
        })
        .collect()
}

/// A Claude Desktop switch restarts Claude: the island asks for a second click first.
pub fn needs_confirm(id: &str) -> bool {
    load().iter().any(|a| a.id == id && a.service == "desktop" && !a.active)
}

// ── Managing the accounts (Settings > Accounts) ─────────────────────────────
// What NotchDeck's window did: who each slot is signed in as, add, rename,
// remove, sign in. Tokens are never read out, shown or copied: only the
// account's e-mail and id that Claude Code / Codex keep beside them.

/// One account as Settings shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Managed {
    pub id: String,
    pub service: String,
    pub name: String,
    pub active: bool,
    pub is_default: bool,
    /// "someone@x.com", "Saved login", "Not signed in".
    pub signed_in_as: String,
    /// A warning: "Same account as Main" ("" for none).
    pub warning: String,
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// A Claude Code slot's account: (uuid, e-mail) from its `.claude.json`
/// (for the default one, %USERPROFILE%\.claude.json beside the folder).
fn claude_account(dir: &Path, is_default: bool) -> Option<(String, String)> {
    let v = read_json(&dir.join(".claude.json")).filter(|v| v.get("oauthAccount").is_some()).or_else(|| is_default.then(|| read_json(&home().join(".claude.json"))).flatten())?;
    let a = v.get("oauthAccount")?;
    Some((a.get("accountUuid")?.as_str()?.to_string(), a.get("emailAddress").and_then(|x| x.as_str()).unwrap_or("").to_string()))
}

/// A Codex slot's e-mail, from the id token's payload in `auth.json` (the
/// token itself is not kept or shown; an API key says so).
fn codex_account(dir: &Path) -> Option<String> {
    let v = read_json(&dir.join("auth.json"))?;
    if let Some(t) = v.get("tokens").and_then(|t| t.get("id_token")).and_then(|t| t.as_str()) {
        let payload = t.split('.').nth(1)?;
        let bytes = base64url(payload)?;
        let claims: Value = serde_json::from_slice(&bytes).ok()?;
        return claims.get("email").and_then(|e| e.as_str()).map(str::to_string);
    }
    v.get("OPENAI_API_KEY").and_then(|k| k.as_str()).filter(|k| !k.is_empty()).map(|_| "API key".to_string())
}

fn base64url(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            _ => return None,
        } as u32)
    };
    let clean: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut n = 0u32;
        for (i, c) in chunk.iter().enumerate() {
            n |= val(*c)? << (18 - 6 * i);
        }
        out.push((n >> 16) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(n as u8);
        }
    }
    Some(out)
}

/// Every account with who it is signed in as, and a warning when two slots
/// of one app are the same account (two logins of one account: switching
/// between them changes nothing).
pub fn managed() -> Vec<Managed> {
    let Some(state) = read_json(&state_path()) else { return Vec::new() };
    let active = state.get("active").cloned().unwrap_or(Value::Null);
    let list = state.get("accounts").and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let mut out: Vec<Managed> = Vec::new();
    let mut seen: Vec<(String, String, String)> = Vec::new(); // (service, account key, slot name)
    for a in &list {
        let (Some(id), Some(service)) = (a.get("id").and_then(|x| x.as_str()), a.get("service").and_then(|x| x.as_str())) else { continue };
        if !SERVICES.iter().any(|(s, _)| *s == service) {
            continue;
        }
        let name = a.get("name").and_then(|x| x.as_str()).unwrap_or(id).to_string();
        let is_default = a.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false);
        let is_active = active.get(service).and_then(|x| x.as_str()) == Some(id);
        let dir = account_dir(a, id);
        let (signed_in_as, key) = match service {
            "claude" => match claude_account(&dir, is_default) {
                Some((uuid, email)) => (if email.is_empty() { "Signed in".to_string() } else { email }, Some(uuid)),
                None => ("Not signed in".to_string(), None),
            },
            "codex" => match codex_account(&dir) {
                Some(e) => (e.clone(), Some(e)),
                None => ("Not signed in".to_string(), None),
            },
            _ => {
                let cfg = if is_active { desktop::support().join("config.json") } else { dir.join("config.json") };
                let uuid = desktop::account_uuid(&cfg);
                let has = if is_active { desktop::has_login(&desktop::support()) } else { desktop::slot_has_login(&dir) };
                (if has { if is_active { "Open in Claude now".into() } else { "Saved login".into() } } else { "Sign in after switching".to_string() }, uuid)
            }
        };
        let warning = match &key {
            Some(k) => seen.iter().find(|(s, kk, _)| s == service && kk == k).map(|(_, _, other)| format!("Same account as {other}")).unwrap_or_default(),
            None => String::new(),
        };
        if let Some(k) = key {
            seen.push((service.to_string(), k, name.clone()));
        }
        out.push(Managed { id: id.to_string(), service: service.to_string(), name, active: is_active, is_default, signed_in_as, warning });
    }
    // A Claude Desktop slot is known by the Claude Code account with its uuid: show that e-mail.
    let emails: Vec<(String, String)> = list
        .iter()
        .filter(|a| a.get("service").and_then(|x| x.as_str()) == Some("claude"))
        .filter_map(|a| {
            let id = a.get("id")?.as_str()?;
            claude_account(&account_dir(a, id), a.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false))
        })
        .collect();
    for m in out.iter_mut().filter(|m| m.service == "desktop") {
        let a = list.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(m.id.as_str()));
        let dir = a.map(|a| account_dir(a, &m.id)).unwrap_or_default();
        let cfg = if m.active { desktop::support().join("config.json") } else { dir.join("config.json") };
        if let Some(u) = desktop::account_uuid(&cfg) {
            if let Some((_, e)) = emails.iter().find(|(uu, e)| *uu == u && !e.is_empty()) {
                m.signed_in_as = format!("{} · {e}", m.signed_in_as);
            }
        }
    }
    out
}

/// A Claude Code account's folders for Send setup: its config folder and
/// its `.claude.json` (beside the folder for the default one).
pub fn claude_slot(id: &str) -> Option<crate::setup::Slot> {
    let state = read_json(&state_path())?;
    let a = state.get("accounts")?.as_array()?.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(id))?.clone();
    if a.get("service").and_then(|x| x.as_str()) != Some("claude") {
        return None;
    }
    let is_default = a.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false);
    let dir = account_dir(&a, id);
    let state_file = if is_default { home().join(".claude.json") } else { dir.join(".claude.json") };
    Some(crate::setup::Slot { dir, state: state_file })
}

/// Changes the state file: `f` gets its accounts list and active map.
fn edit_state(f: impl FnOnce(&mut Vec<Value>, &mut serde_json::Map<String, Value>) -> Result<(), String>) -> Result<(), String> {
    let path = state_path();
    let mut state: Value = read_json(&path).unwrap_or_else(|| serde_json::json!({ "accounts": [], "active": {} }));
    let mut list = state.get("accounts").and_then(|l| l.as_array()).cloned().unwrap_or_default();
    let mut active = state.get("active").and_then(|a| a.as_object()).cloned().unwrap_or_default();
    f(&mut list, &mut active)?;
    state["accounts"] = Value::Array(list);
    state["active"] = Value::Object(active);
    std::fs::create_dir_all(root()).map_err(|e| e.to_string())?;
    write_state(&path, &state)
}

pub fn rename(id: &str, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a name, please".into());
    }
    edit_state(|list, _| {
        let a = list.iter_mut().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(id)).ok_or("no such account")?;
        a["name"] = Value::String(name.to_string());
        Ok(())
    })
}

/// A new slot for `service` named `name`: its own folder; returns its id.
/// Claude Code / Codex then sign in (`sign_in`); Claude Desktop signs in
/// after switching to it.
pub fn add(service: &str, name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || !SERVICES.iter().any(|(s, _)| *s == service) {
        return Err("a name, please".into());
    }
    let slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let slug = if slug.is_empty() { "account".to_string() } else { slug };
    let mut new_id = String::new();
    edit_state(|list, _| {
        let taken = |id: &str| list.iter().any(|a| a.get("id").and_then(|x| x.as_str()) == Some(id));
        let mut id = format!("{service}-{slug}");
        let mut n = 2;
        while taken(&id) {
            id = format!("{service}-{slug}-{n}");
            n += 1;
        }
        let dir = root().join(service).join(&id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        list.push(serde_json::json!({ "id": id, "service": service, "name": name, "isDefault": false, "dir": dir.to_string_lossy() }));
        new_id = id;
        Ok(())
    })?;
    Ok(new_id)
}

/// Takes a slot off the list. Its folder is moved aside to
/// .accountdeck\_removed (not deleted), so a login is never lost by a click.
/// Not the Main ones, nor Claude Desktop's slot in use.
pub fn remove(id: &str) -> Result<(), String> {
    let mut gone: Option<PathBuf> = None;
    edit_state(|list, active| {
        let i = list.iter().position(|a| a.get("id").and_then(|x| x.as_str()) == Some(id)).ok_or("no such account")?;
        let a = list[i].clone();
        if a.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false) {
            return Err("the Main account stays".into());
        }
        let service = a.get("service").and_then(|x| x.as_str()).unwrap_or("").to_string();
        if service == "desktop" && active.get("desktop").and_then(|x| x.as_str()) == Some(id) {
            return Err("switch Claude to another account first".into());
        }
        gone = Some(account_dir(&a, id));
        list.remove(i);
        if active.get(&service).and_then(|x| x.as_str()) == Some(id) {
            if let Some(main) = list.iter().find(|x| x.get("service").and_then(|s| s.as_str()) == Some(service.as_str()) && x.get("isDefault").and_then(|d| d.as_bool()).unwrap_or(false)) {
                active.insert(service.clone(), main.get("id").cloned().unwrap_or(Value::Null));
            }
        }
        Ok(())
    })?;
    if let Some(dir) = gone.filter(|d| d.exists() && d.starts_with(root())) {
        let aside = root().join("_removed");
        let _ = std::fs::create_dir_all(&aside);
        let stamp = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let _ = std::fs::rename(&dir, aside.join(format!("{}-{stamp}", dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())));
    }
    Ok(())
}

/// Opens a terminal signed in as nobody yet, its config folder set to the
/// slot's, running the app's own login (Claude Code: /login, Codex: login).
/// The user finishes it there; nothing of it passes through FubuNotch v2.
/// It starts in `cwd` (Settings → Accounts → Terminal folder), else the user's folder.
pub fn sign_in(id: &str, cwd: Option<&Path>) -> Result<(), String> {
    let state = read_json(&state_path()).ok_or("no accounts set up")?;
    let a = state.get("accounts").and_then(|l| l.as_array()).and_then(|l| l.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(id))).cloned().ok_or("no such account")?;
    let service = a.get("service").and_then(|x| x.as_str()).unwrap_or("");
    let is_default = a.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false);
    let dir = account_dir(&a, id);
    let (var, run) = match service {
        "claude" => ("CLAUDE_CONFIG_DIR", "claude /login"),
        "codex" => ("CODEX_HOME", "codex login"),
        _ => return Err("Claude Desktop signs in after switching to it".into()),
    };
    let name = a.get("name").and_then(|x| x.as_str()).unwrap_or(id).replace(['"', '&', '|', '<', '>', '^', '%'], " ");
    let mut cmd = std::process::Command::new("cmd.exe");
    cmd.args(["/k", &format!("title Sign in: {name} & echo Sign in as \"{name}\", then type /exit. & {run}")]);
    if is_default {
        cmd.env_remove(var);
    } else {
        cmd.env(var, &dir);
    }
    use std::os::windows::process::CommandExt;
    let cwd = cwd.filter(|p| p.is_dir()).map(Path::to_path_buf).unwrap_or_else(home);
    cmd.creation_flags(0x0000_0010) // CREATE_NEW_CONSOLE
        .current_dir(cwd)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Switches to the account. Blocking (a Claude Desktop switch takes several
/// seconds): call it off the UI thread. Returns a short line for the island.
pub fn switch(id: &str) -> Result<String, String> {
    if demo() {
        return Err("demo accounts".into());
    }
    let path = state_path();
    let mut state: Value = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("no accounts set up")?;
    let acc = state
        .get("accounts")
        .and_then(|l| l.as_array())
        .and_then(|l| l.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(id)))
        .cloned()
        .ok_or_else(|| format!("no account {id}"))?;
    let service = acc.get("service").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let name = acc.get("name").and_then(|x| x.as_str()).unwrap_or(id).to_string();
    let current_id = state.get("active").and_then(|m| m.get(&service)).and_then(|x| x.as_str()).map(str::to_string);
    if current_id.as_deref() == Some(id) {
        return Ok(format!("Already {name}"));
    }
    match service.as_str() {
        "claude" | "codex" => {
            let var = if service == "claude" { "CLAUDE_CONFIG_DIR" } else { "CODEX_HOME" };
            let is_default = acc.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false);
            let dir = acc.get("dir").and_then(|x| x.as_str()).unwrap_or("");
            set_user_env(var, if is_default || dir.is_empty() { None } else { Some(dir) })?;
            set_active(&mut state, &service, id);
            write_state(&path, &state)?;
            Ok(format!("{name}: new terminals use it"))
        }
        "desktop" => {
            let current = current_id.as_deref().and_then(|cid| {
                state.get("accounts")?.as_array()?.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(cid)).cloned()
            });
            let carry = std::fs::read(root().join("settings.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|s| s.get("carryChat").and_then(|x| x.as_bool()))
                .unwrap_or(true);
            let target_dir = account_dir(&acc, id);
            let current_dir = current.as_ref().and_then(|c| c.get("id").and_then(|x| x.as_str()).map(|cid| account_dir(c, cid)));
            let last_exe = desktop::quit()?;
            let sup = desktop::support();
            let from_uuid = desktop::account_uuid(&sup.join("config.json"));
            desktop::swap(current_dir.as_deref(), &target_dir, &sup, &root())?;
            set_active(&mut state, "desktop", id);
            write_state(&path, &state)?;
            let carried = if carry {
                desktop::carry_last_chat(from_uuid.as_deref(), desktop::account_uuid(&sup.join("config.json")).as_deref(), &sup, &root())
            } else {
                None
            };
            if !desktop::launch(last_exe.as_deref()) {
                return Ok(format!("Claude is {name}: open it from Start"));
            }
            Ok(match carried {
                Some(t) => format!("Claude is {name}, with \"{t}\""),
                None if desktop::slot_has_login(&target_dir) || desktop::has_login(&sup) => format!("Claude is {name}"),
                None => format!("Sign in to Claude as {name}"),
            })
        }
        _ => Err(format!("can't switch {service}")),
    }
}

/// An account's folder: "dir" as saved, else %USERPROFILE%\.accountdeck\<service>\<id>.
fn account_dir(acc: &Value, id: &str) -> PathBuf {
    match acc.get("dir").and_then(|x| x.as_str()) {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => root().join(acc.get("service").and_then(|x| x.as_str()).unwrap_or("desktop")).join(id),
    }
}

fn set_active(state: &mut Value, service: &str, id: &str) {
    if !state.get("active").is_some_and(|a| a.is_object()) {
        state["active"] = Value::Object(Default::default());
    }
    state["active"][service] = Value::String(id.to_string());
}

/// Written beside and renamed over, so a crash never leaves half a file.
fn write_state(path: &Path, state: &Value) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(state).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Sets (None: removes) a per-user environment variable in HKCU\Environment and
/// tells Windows, so Explorer and terminals opened after this see it.
fn set_user_env(name: &str, value: Option<&str>) -> Result<(), String> {
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};
    let key = w!("Environment");
    let n = HSTRING::from(name);
    unsafe {
        match value {
            Some(v) => {
                let v = HSTRING::from(v);
                let bytes = std::slice::from_raw_parts(v.as_ptr() as *const u8, (v.len() + 1) * 2);
                RegSetKeyValueW(HKEY_CURRENT_USER, key, &n, REG_SZ.0, Some(bytes.as_ptr() as *const _), bytes.len() as u32)
                    .ok()
                    .map_err(|e| e.to_string())?;
            }
            None => {
                let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, &n);
                if !(r.is_ok() || r.0 == 2) {
                    return Err(format!("{r:?}"));
                }
            }
        }
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(w!("Environment").as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
    }
    Ok(())
}

mod desktop {
    use super::*;

    /// The files that hold Claude Desktop's login. Current versions keep the
    /// cookie database in Network\, older ones at the top level: both handled.
    pub const FILES: [&str; 5] = ["Cookies", "Cookies-journal", "Network\\Cookies", "Network\\Cookies-journal", "config.json"];
    const COOKIES: [&str; 2] = ["Cookies", "Network\\Cookies"];

    fn env_dir(var: &str) -> PathBuf {
        std::env::var_os(var).map(PathBuf::from).unwrap_or_default()
    }

    fn mtime(p: &Path) -> u64 {
        std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
    }

    fn dirs(p: &Path) -> Vec<(String, PathBuf)> {
        std::fs::read_dir(p)
            .map(|r| r.flatten().filter(|e| e.path().is_dir()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).collect())
            .unwrap_or_default()
    }

    /// Claude Desktop's data folder. Direct installer: %APPDATA%\Claude. Store /
    /// MSIX: %LOCALAPPDATA%\Claude-Data\claude_<publisher> (5.x) or the package's
    /// LocalCache. When several exist, the one used most recently wins.
    pub fn support() -> PathBuf {
        let local = env_dir("LOCALAPPDATA");
        let mut c: Vec<PathBuf> = Vec::new();
        for (name, p) in dirs(&local.join("Claude-Data")) {
            if name.to_ascii_lowercase().starts_with("claude") {
                c.push(p);
            }
        }
        for (name, p) in dirs(&local.join("Packages")) {
            let l = name.to_ascii_lowercase();
            if l.starts_with("claude_") || l.starts_with("anthropic.claude_") {
                for sub in ["LocalCache\\Roaming\\Claude", "LocalCache\\Local\\Claude"] {
                    if p.join(sub).is_dir() {
                        c.push(p.join(sub));
                    }
                }
            }
        }
        let direct = env_dir("APPDATA").join("Claude");
        if direct.is_dir() {
            c.push(direct.clone());
        }
        let score = |d: &PathBuf| mtime(&d.join("config.json")).max(mtime(&d.join("Network\\Cookies"))).max(mtime(&d.join("Cookies")));
        c.into_iter().max_by_key(score).unwrap_or(direct)
    }

    pub fn has_login(sup: &Path) -> bool {
        COOKIES.iter().any(|f| sup.join(f).exists())
    }

    pub fn slot_has_login(dir: &Path) -> bool {
        has_login(dir)
    }

    pub fn account_uuid(config: &Path) -> Option<String> {
        let v: Value = serde_json::from_slice(&std::fs::read(config).ok()?).ok()?;
        v.get("lastKnownAccountUuid")?.as_str().map(str::to_string)
    }

    fn copy(from: &Path, to: &Path) -> Result<(), String> {
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("{}: {e}", to.display()))
    }

    fn rm(p: &Path) -> Result<(), String> {
        match std::fs::remove_file(p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", p.display())),
            _ => Ok(()),
        }
    }

    /// Saves the login in Claude now into `current`'s folder and puts `target`'s
    /// in place (a new account: signed out, the app's own settings kept).
    pub fn swap(current: Option<&Path>, target: &Path, sup: &Path, base: &Path) -> Result<(), String> {
        // One-time safety copy of the login there was before any switch.
        let backup = base.join("desktop").join("_original-backup");
        if !backup.exists() {
            std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
            for f in FILES {
                if sup.join(f).exists() {
                    copy(&sup.join(f), &backup.join(f))?;
                }
            }
        }
        if let Some(cur) = current {
            std::fs::create_dir_all(cur).map_err(|e| e.to_string())?;
            for f in FILES {
                rm(&cur.join(f))?;
                if sup.join(f).exists() {
                    copy(&sup.join(f), &cur.join(f))?;
                }
            }
        }
        if slot_has_login(target) {
            for f in FILES {
                rm(&sup.join(f))?;
                if target.join(f).exists() {
                    copy(&target.join(f), &sup.join(f))?;
                }
            }
        } else {
            for f in FILES.iter().filter(|f| **f != "config.json") {
                rm(&sup.join(f))?;
            }
            let cfg = sup.join("config.json");
            if let Some(mut v) = std::fs::read(&cfg).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
                if let Some(o) = v.as_object_mut() {
                    o.retain(|k, _| !k.starts_with("oauth:") && k != "lastKnownAccountUuid");
                }
                std::fs::write(&cfg, serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    /// Moves the most recently opened Code-tab chat into the new account's list
    /// (claude-code-sessions\<account>\<org>\local_*.json), a copy kept in
    /// .accountdeck\moved-chats. Returns its title.
    pub fn carry_last_chat(from: Option<&str>, to: Option<&str>, sup: &Path, base: &Path) -> Option<String> {
        let (from, to) = (from?, to?);
        if from == to {
            return None;
        }
        let sessions = sup.join("claude-code-sessions");
        let mut last: Option<(PathBuf, Value, f64)> = None;
        for (_, org) in dirs(&sessions.join(from)) {
            for e in std::fs::read_dir(&org).ok()?.flatten() {
                let f = e.file_name().to_string_lossy().into_owned();
                if !(f.starts_with("local_") && f.ends_with(".json")) {
                    continue;
                }
                let Some(j) = std::fs::read(e.path()).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else { continue };
                if j.get("isArchived").and_then(|x| x.as_bool()) == Some(true) {
                    continue;
                }
                let t = j.get("lastFocusedAt").and_then(|x| x.as_f64()).unwrap_or(0.0);
                if last.as_ref().is_none_or(|l| t > l.2) {
                    last = Some((e.path(), j, t));
                }
            }
        }
        let (path, mut json, _) = last?;
        let to_dir = sessions.join(to);
        let newest = |org: &Path| {
            std::fs::read_dir(org)
                .map(|r| r.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("local_")).map(|e| mtime(&e.path())).max().unwrap_or(0))
                .unwrap_or(0)
        };
        let org = dirs(&to_dir).into_iter().map(|(_, p)| p).max_by_key(|p| newest(p))?;
        let name = path.file_name()?.to_owned();
        let dest = org.join(&name);
        if dest.exists() {
            return None;
        }
        let keep = base.join("moved-chats");
        let _ = std::fs::create_dir_all(&keep);
        let _ = std::fs::copy(&path, keep.join(&name));
        let now = std::time::SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        json["lastFocusedAt"] = Value::from(now);
        std::fs::write(&dest, serde_json::to_string(&json).ok()?).ok()?;
        let _ = std::fs::remove_file(&path);
        Some(json.get("title").and_then(|x| x.as_str()).unwrap_or("your last chat").to_string())
    }

    /// Runs a PowerShell script without a window; its output, trimmed.
    fn ps(script: &str) -> Option<String> {
        use std::os::windows::process::CommandExt;
        let wide: Vec<u8> = script.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let out = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64(&wide)])
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    pub(super) fn base64(b: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
        for c in b.chunks(3) {
            let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
            for i in 0..4 {
                s.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        s
    }

    // Claude Desktop is claude.exe, and so can Claude Code's CLI be: matched by install folder, never by name alone.
    const FILTER: &str = r"function Get-Desk { Get-Process -Name claude -ErrorAction SilentlyContinue | Where-Object { $_.Path -and ($_.Path -like '*\AnthropicClaude\*' -or $_.Path -like '*\WindowsApps\Claude_*' -or $_.Path -like '*\WindowsApps\Anthropic*' -or $_.Path -like '*\Programs\Claude\*' -or $_.Path -like '*\Anthropic\Claude\*') } }";

    /// Closes Claude Desktop: politely first, then ends it. Returns the exe it
    /// ran from (to open the same one again), or an error when it won't close.
    pub fn quit() -> Result<Option<String>, String> {
        let script = format!(
            r"{FILTER}
$p = @(Get-Desk)
if ($p.Count -eq 0) {{ 'exe:'; 'gone'; exit }}
'exe:' + $p[0].Path
$p | ForEach-Object {{ try {{ [void]$_.CloseMainWindow() }} catch {{}} }}
for ($i = 0; $i -lt 8; $i++) {{ Start-Sleep -Milliseconds 500; if (@(Get-Desk).Count -eq 0) {{ Start-Sleep -Milliseconds 1000; 'gone'; exit }} }}
Get-Desk | Stop-Process -Force -ErrorAction SilentlyContinue
for ($i = 0; $i -lt 40; $i++) {{ Start-Sleep -Milliseconds 500; if (@(Get-Desk).Count -eq 0) {{ Start-Sleep -Milliseconds 1200; 'gone'; exit }} }}
'still'"
        );
        let out = ps(&script).ok_or("couldn't run PowerShell")?;
        let exe = out.lines().find_map(|l| l.strip_prefix("exe:")).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        if out.lines().any(|l| l.trim() == "gone") {
            Ok(exe)
        } else {
            Err("Claude didn't close: quit it from its tray icon".into())
        }
    }

    fn start_app(app_id: &str) -> bool {
        std::process::Command::new("explorer.exe").arg(format!("shell:AppsFolder\\{app_id}")).spawn().is_ok()
    }

    /// Opens Claude Desktop again: the Store app through its AppID, or the installed exe.
    pub fn launch(last_exe: Option<&str>) -> bool {
        let store = last_exe.is_none_or(|e| e.to_ascii_lowercase().contains("\\windowsapps\\"));
        if store {
            if let Some(id) = ps("(Get-StartApps | Where-Object { $_.AppID -like 'Claude_*!*' -or $_.AppID -like 'Anthropic*Claude*!*' } | Select-Object -First 1).AppID").filter(|s| !s.is_empty()) {
                return start_app(&id);
            }
        }
        let local = env_dir("LOCALAPPDATA");
        let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Program Files"));
        let mut list = vec![local.join("AnthropicClaude\\claude.exe"), local.join("Programs\\Claude\\Claude.exe"), pf.join("Claude\\Claude.exe")];
        if let Some(e) = last_exe {
            list.push(PathBuf::from(e));
        }
        if let Some(exe) = list.into_iter().find(|p| p.exists()) {
            return std::process::Command::new(&exe).current_dir(exe.parent().unwrap_or(Path::new("."))).spawn().is_ok();
        }
        match ps("(Get-StartApps | Where-Object { $_.Name -eq 'Claude' } | Select-Object -First 1).AppID").filter(|s| !s.is_empty()) {
            Some(id) => start_app(&id),
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_accounts_and_the_active_one() {
        let s = br#"{"accounts":[
            {"id":"desktop-default","service":"desktop","name":"Alex Max","isDefault":true},
            {"id":"claude-default","service":"claude","name":"Partner"},
            {"id":"claude-alex","service":"claude","name":"Alex"},
            {"id":"chatgpt-x","service":"chatgpt","name":"Web"}],
            "active":{"desktop":"desktop-default","claude":"claude-alex"}}"#;
        let a = parse(s);
        assert_eq!(a.len(), 3, "chatgpt is left out");
        assert!(a[0].active);
        assert!(!a[1].active);
        assert!(a[2].active);
        assert_eq!(a[2].name, "Alex");
    }

    #[test]
    fn garbage_gives_nothing() {
        assert!(parse(b"").is_empty());
        assert!(parse(b"{}").is_empty());
        assert!(parse(b"[1,2]").is_empty());
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(desktop_b64(b""), "");
        assert_eq!(desktop_b64(b"f"), "Zg==");
        assert_eq!(desktop_b64(b"fo"), "Zm8=");
        assert_eq!(desktop_b64(b"foo"), "Zm9v");
        assert_eq!(desktop_b64(b"foobar"), "Zm9vYmFy");
    }

    fn desktop_b64(b: &[u8]) -> String {
        desktop::base64(b)
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("fubunotch-accounts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn desktop_swap_saves_the_current_login_and_restores_the_target() {
        let base = scratch("swap");
        let sup = base.join("sup");
        let cur = base.join("desktop").join("a");
        let tgt = base.join("desktop").join("b");
        std::fs::create_dir_all(sup.join("Network")).unwrap();
        std::fs::write(sup.join("Network\\Cookies"), "A-cookies").unwrap();
        std::fs::write(sup.join("config.json"), r#"{"lastKnownAccountUuid":"ua","theme":"dark"}"#).unwrap();
        std::fs::create_dir_all(tgt.join("Network")).unwrap();
        std::fs::write(tgt.join("Network\\Cookies"), "B-cookies").unwrap();
        std::fs::write(tgt.join("config.json"), r#"{"lastKnownAccountUuid":"ub"}"#).unwrap();

        desktop::swap(Some(&cur), &tgt, &sup, &base).unwrap();

        assert_eq!(std::fs::read_to_string(cur.join("Network\\Cookies")).unwrap(), "A-cookies");
        assert_eq!(std::fs::read_to_string(sup.join("Network\\Cookies")).unwrap(), "B-cookies");
        assert_eq!(desktop::account_uuid(&sup.join("config.json")).as_deref(), Some("ub"));
        assert!(base.join("desktop\\_original-backup\\Network\\Cookies").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn desktop_swap_to_a_new_account_signs_out_and_keeps_settings() {
        let base = scratch("new");
        let sup = base.join("sup");
        let tgt = base.join("desktop").join("fresh");
        std::fs::create_dir_all(&sup).unwrap();
        std::fs::write(sup.join("Cookies"), "x").unwrap();
        std::fs::write(sup.join("config.json"), r#"{"oauth:tokenCacheV2":"t","lastKnownAccountUuid":"u","theme":"dark"}"#).unwrap();

        desktop::swap(None, &tgt, &sup, &base).unwrap();

        assert!(!sup.join("Cookies").exists());
        let v: Value = serde_json::from_str(&std::fs::read_to_string(sup.join("config.json")).unwrap()).unwrap();
        assert!(v.get("oauth:tokenCacheV2").is_none());
        assert!(v.get("lastKnownAccountUuid").is_none());
        assert_eq!(v["theme"], "dark");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_last_chat_moves_to_the_new_account() {
        let base = scratch("chat");
        let sup = base.join("sup");
        let from = sup.join("claude-code-sessions\\ua\\org1");
        let to = sup.join("claude-code-sessions\\ub\\org2");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("local_old.json"), r#"{"title":"old","lastFocusedAt":1}"#).unwrap();
        std::fs::write(from.join("local_new.json"), r#"{"title":"new one","lastFocusedAt":5}"#).unwrap();
        std::fs::write(from.join("local_arch.json"), r#"{"title":"arch","lastFocusedAt":9,"isArchived":true}"#).unwrap();

        let t = desktop::carry_last_chat(Some("ua"), Some("ub"), &sup, &base);

        assert_eq!(t.as_deref(), Some("new one"));
        assert!(to.join("local_new.json").exists());
        assert!(!from.join("local_new.json").exists());
        assert!(base.join("moved-chats\\local_new.json").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// Live, on this PC's real accounts: Claude Code to its first other
    /// account and back to Main, checking the user variable and state.json.
    /// Run outside any app container: `cargo test -p fubunotch-v2 live_claude -- --ignored`.
    #[test]
    #[ignore]
    fn live_claude_code_round_trip() {
        use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
        let read_var = || -> Option<String> {
            let mut buf = vec![0u16; 1024];
            let mut len = (buf.len() * 2) as u32;
            let r = unsafe {
                RegGetValueW(HKEY_CURRENT_USER, windows::core::w!("Environment"), windows::core::w!("CLAUDE_CONFIG_DIR"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len))
            };
            r.is_ok().then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
        };
        let list = load();
        let main = list.iter().find(|a| a.service == "claude" && a.id == "claude-default").expect("Main account").clone();
        let other = list.iter().find(|a| a.service == "claude" && a.id != main.id).expect("a second Claude Code account").clone();
        assert!(main.active, "start from Main");
        println!("before: {:?}", read_var());
        println!("{}", switch(&other.id).unwrap());
        assert_eq!(read_var().as_deref(), Some(other.dir.as_str()));
        assert!(load().iter().any(|a| a.id == other.id && a.active));
        println!("{}", switch(&main.id).unwrap());
        assert_eq!(read_var(), None, "Main removes the variable");
        assert!(load().iter().any(|a| a.id == main.id && a.active));
        println!("round trip ok");
    }

    /// Add, rename, remove and the same-account warning, in a made-up home
    /// (USERPROFILE pointed at a scratch folder; run alone:
    /// cargo test -p fubunotch-v2 manage_in_a_scratch_home -- --ignored --test-threads=1).
    #[test]
    #[ignore]
    fn manage_in_a_scratch_home() {
        let home = scratch("home");
        let old = std::env::var_os("USERPROFILE");
        std::env::set_var("USERPROFILE", &home);
        let main_dir = home.join(".claude");
        std::fs::create_dir_all(&main_dir).unwrap();
        std::fs::write(home.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"u1","emailAddress":"a@x.com"}}"#).unwrap();
        std::fs::create_dir_all(home.join(".accountdeck")).unwrap();
        let state = serde_json::json!({ "accounts": [
            { "id": "claude-default", "service": "claude", "name": "Main", "isDefault": true, "dir": main_dir.to_string_lossy() } ],
            "active": { "claude": "claude-default" } });
        std::fs::write(home.join(".accountdeck\\state.json"), state.to_string()).unwrap();

        let id = add("claude", "Second One").unwrap();
        assert_eq!(id, "claude-second-one");
        let dir = home.join(".accountdeck\\claude\\claude-second-one");
        assert!(dir.is_dir());
        // Signed in to the same account as Main: warned.
        std::fs::write(dir.join(".claude.json"), r#"{"oauthAccount":{"accountUuid":"u1","emailAddress":"a@x.com"}}"#).unwrap();
        let m = managed();
        let second = m.iter().find(|x| x.id == id).unwrap();
        assert_eq!(second.signed_in_as, "a@x.com");
        assert_eq!(second.warning, "Same account as Main");
        rename(&id, "Renamed").unwrap();
        assert!(load().iter().any(|a| a.id == id && a.name == "Renamed"));
        assert!(remove("claude-default").is_err(), "Main stays");
        remove(&id).unwrap();
        assert!(!load().iter().any(|a| a.id == id));
        assert!(!dir.exists(), "moved aside");
        assert_eq!(std::fs::read_dir(home.join(".accountdeck\\_removed")).unwrap().count(), 1, "kept in _removed");

        match old {
            Some(v) => std::env::set_var("USERPROFILE", v),
            None => std::env::remove_var("USERPROFILE"),
        }
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn base64url_reads_jwt_payloads() {
        assert_eq!(base64url("eyJlbWFpbCI6ImFAeC5jb20ifQ").unwrap(), br#"{"email":"a@x.com"}"#);
    }

    #[test]
    fn active_is_set_even_when_missing() {
        let mut v: Value = serde_json::from_str(r#"{"accounts":[]}"#).unwrap();
        set_active(&mut v, "claude", "claude-x");
        assert_eq!(v["active"]["claude"], "claude-x");
    }
}

/// Where an account keeps its chats: a Claude Code account's `projects`
/// folder, or the Claude Desktop account's uuid (its Code-tab chats sit in
/// Claude Desktop's `claude-code-sessions\<uuid>`).
pub enum ChatHome {
    Code(PathBuf),
    Desktop { sessions: PathBuf, uuid: String },
}

pub fn chat_home(id: &str) -> Option<ChatHome> {
    let state = read_json(&state_path())?;
    let active = state.get("active").cloned().unwrap_or(Value::Null);
    let a = state.get("accounts")?.as_array()?.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(id))?.clone();
    let service = a.get("service").and_then(|x| x.as_str())?;
    let dir = account_dir(&a, id);
    match service {
        "claude" => Some(ChatHome::Code(dir.join("projects"))),
        "desktop" => {
            let is_active = active.get("desktop").and_then(|x| x.as_str()) == Some(id);
            let cfg = if is_active { desktop::support().join("config.json") } else { dir.join("config.json") };
            let uuid = desktop::account_uuid(&cfg)?;
            Some(ChatHome::Desktop { sessions: desktop::support().join("claude-code-sessions"), uuid })
        }
        _ => None,
    }
}

/// (service, config dir, is default) of an account.
fn slot_of(id: &str) -> Result<(String, PathBuf, bool, String), String> {
    let state = read_json(&state_path()).ok_or("no accounts set up")?;
    let a = state.get("accounts").and_then(|l| l.as_array()).and_then(|l| l.iter().find(|a| a.get("id").and_then(|x| x.as_str()) == Some(id))).cloned().ok_or("no such account")?;
    let service = a.get("service").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let is_default = a.get("isDefault").and_then(|x| x.as_bool()).unwrap_or(false);
    let name = a.get("name").and_then(|x| x.as_str()).unwrap_or(id).replace(['"', '&', '|', '<', '>', '^', '%'], " ");
    let dir = if is_default {
        match service.as_str() {
            "claude" => home().join(".claude"),
            "codex" => home().join(".codex"),
            _ => account_dir(&a, id),
        }
    } else {
        account_dir(&a, id)
    };
    Ok((service, dir, is_default, name))
}

/// A terminal for the account (its config folder set), in `cwd`, running
/// `run` when given (e.g. `claude --resume <id>`), as NotchDeck's.
pub fn terminal(id: &str, cwd: Option<&Path>, run: Option<&str>) -> Result<(), String> {
    let (service, dir, is_default, name) = slot_of(id)?;
    let var = match service.as_str() {
        "claude" => "CLAUDE_CONFIG_DIR",
        "codex" => "CODEX_HOME",
        _ => return Err("Claude Desktop has no terminal".into()),
    };
    let mut line = format!("title {name}");
    if let Some(r) = run {
        line.push_str(&format!(" & {r}"));
    }
    let mut cmd = std::process::Command::new("cmd.exe");
    cmd.args(["/k", &line]);
    if is_default {
        cmd.env_remove(var);
    } else {
        cmd.env(var, &dir);
    }
    let cwd = cwd.filter(|p| p.is_dir()).map(Path::to_path_buf).unwrap_or_else(home);
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0000_0010).current_dir(cwd).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Opens the account's folder in Explorer.
pub fn reveal(id: &str) -> Result<(), String> {
    let (_, dir, _, _) = slot_of(id)?;
    std::process::Command::new("explorer.exe").arg(&dir).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Claude Desktop's Code-tab chat folder (titles of Code chats it ran).
pub fn chat_sessions_dir() -> Option<PathBuf> {
    Some(desktop::support().join("claude-code-sessions"))
}

/// Claude Desktop's MCP config (shared by every account on this PC).
pub fn desktop_config() -> PathBuf {
    desktop::support().join("claude_desktop_config.json")
}
