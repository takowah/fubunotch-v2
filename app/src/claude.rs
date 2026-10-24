// The Claude Code module: hook events (relayed by fubunotch-v2-hook through the
// webhook) become activities, and the hooks are installed into
// ~/.claude/settings.json only after the user saw what changes and agreed:
// a dated backup first, the user's own entries (v1's included) never touched.
//
// Event → activity:
//   UserPromptSubmit, PreToolUse  → progress "claude:<session>" (what it does)
//   PermissionRequest             → ask with Allow / Deny, answered for real
//   Notification                  → alert
//   Stop                          → alert "finished", the progress ends
//   SessionEnd                    → everything of the session ends

use serde_json::{json, Map, Value};

pub const MARKER: &str = "fubunotch-v2-hook";

/// The hook events installed, with Claude Code's timeout for each (seconds).
/// PermissionRequest waits for a human: longer than the relay's own wait.
pub const EVENTS: &[(&str, u64)] = &[
    ("SessionStart", 10),
    ("SessionEnd", 10),
    ("UserPromptSubmit", 10),
    ("PreToolUse", 10),
    ("PermissionRequest", 120),
    ("Notification", 10),
    ("Stop", 10),
];

fn cut(text: &str, max: usize) -> String {
    island_core::activity::cut(text, max)
}

/// "fubunotch-v2" for "C:\Users\x\fubunotch-v2".
pub fn project(payload: &Value) -> String {
    let cwd = payload.get("cwd").and_then(Value::as_str).unwrap_or("");
    let name = cwd.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or("");
    if name.is_empty() { "Claude Code".into() } else { name.into() }
}

/// What a tool call is, in a few words: "Bash · npm run build", "Edit · main.rs".
pub fn tool_line(payload: &Value) -> String {
    let tool = payload.get("tool_name").and_then(Value::as_str).unwrap_or("tool");
    let input = payload.get("tool_input").cloned().unwrap_or(Value::Null);
    let detail = ["command", "file_path", "pattern", "url", "description", "prompt"]
        .iter()
        .find_map(|k| input.get(*k).and_then(Value::as_str))
        .map(|d| {
            // A path shows as its file name.
            if d.contains(['\\', '/']) && !d.contains(' ') {
                d.rsplit(['\\', '/']).next().unwrap_or(d).to_string()
            } else {
                d.to_string()
            }
        });
    match detail {
        Some(d) if !d.is_empty() => cut(&format!("{tool} · {d}"), 110),
        _ => tool.to_string(),
    }
}

pub fn session(payload: &Value) -> String {
    let id = payload.get("session_id").and_then(Value::as_str).unwrap_or("default");
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(64).collect()
}

/// What the island should do for one hook event.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Activities to show (JSON for island-core's Incoming).
    Show(Vec<Value>),
    /// Activities to end: by id, or every one starting with a prefix.
    End(Vec<String>, Option<String>),
    /// Both, in that order: end first.
    EndThenShow(Vec<String>, Vec<Value>),
    Nothing,
}

/// The plan for `event`. `ask_id` names a permission request's activity.
pub fn plan(event: &str, payload: &Value, ask_id: &str) -> Plan {
    let sid = session(payload);
    let project = project(payload);
    let work = format!("claude:{sid}");
    match event {
        "UserPromptSubmit" => {
            let prompt = payload.get("prompt").and_then(Value::as_str).unwrap_or("");
            Plan::Show(vec![json!({ "id": work, "source": "Claude Code", "kind": "progress", "title": format!("{project} · working"), "body": cut(prompt, 160) })])
        }
        "PreToolUse" => Plan::Show(vec![json!({ "id": work, "source": "Claude Code", "kind": "progress", "title": format!("{project} · {}", tool_line(payload)) })]),
        "PermissionRequest" => Plan::Show(vec![json!({
            "id": ask_id,
            "source": "Claude Code",
            "kind": "ask",
            "title": format!("Allow {}?", payload.get("tool_name").and_then(Value::as_str).unwrap_or("this")),
            "body": format!("{project}\n{}", tool_line(payload)),
            "ttl_ms": 110_000,
            "actions": [{ "id": "allow", "label": "Allow" }, { "id": "deny", "label": "Deny" }],
        })]),
        "Notification" => {
            let msg = payload.get("message").and_then(Value::as_str).unwrap_or("Claude Code needs you");
            Plan::Show(vec![json!({ "id": format!("{work}:note"), "source": "Claude Code", "kind": "alert", "title": project, "body": cut(msg, 200) })])
        }
        "Stop" => Plan::EndThenShow(
            vec![work.clone()],
            vec![json!({ "id": format!("{work}:done"), "source": "Claude Code", "kind": "alert", "title": format!("{project} · finished") })],
        ),
        "SessionEnd" => Plan::End(vec![], Some(work)),
        _ => Plan::Nothing,
    }
}

// ── Installing the hooks ──────────────────────────────────────────────────────

pub fn settings_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join(".claude").join("settings.json")
}

/// The relay next to this exe, as Claude Code (Git Bash on Windows) runs it:
/// forward slashes, quoted (the path may hold spaces).
pub fn relay_command(event: &str) -> Option<String> {
    let exe = std::env::current_exe().ok()?.with_file_name("fubunotch-v2-hook.exe");
    exe.exists().then(|| format!("\"{}\" {event}", exe.display().to_string().replace('\\', "/")))
}

fn is_ours(entry: &Value) -> bool {
    entry.get("hooks").and_then(Value::as_array).is_some_and(|hooks| {
        hooks.iter().any(|h| h.get("command").and_then(Value::as_str).is_some_and(|c| c.contains(MARKER)))
    })
}

/// True when every event has our entry.
pub fn installed(settings: &Value) -> bool {
    EVENTS.iter().all(|(event, _)| {
        settings.get("hooks").and_then(|h| h.get(*event)).and_then(Value::as_array).is_some_and(|list| list.iter().any(is_ours))
    })
}

/// `settings` with our hooks added (install) or removed; everything else as it
/// was. Err when "hooks" isn't what Claude Code documents (refused, not replaced).
pub fn merged(settings: &Value, install: bool, command: impl Fn(&str) -> String) -> Result<Value, String> {
    let mut root: Map<String, Value> = match settings {
        Value::Object(m) => m.clone(),
        Value::Null => Map::new(),
        _ => return Err("settings.json is not a JSON object".into()),
    };
    let mut hooks = match root.get("hooks") {
        None => Map::new(),
        Some(Value::Object(h)) => h.clone(),
        Some(_) => return Err("\"hooks\" in settings.json is not an object".into()),
    };
    for (event, timeout) in EVENTS {
        let mut list = match hooks.get(*event) {
            None => Vec::new(),
            Some(Value::Array(l)) => l.clone(),
            Some(_) => return Err(format!("\"hooks\".\"{event}\" is not a list")),
        };
        list.retain(|e| !is_ours(e));
        if install {
            list.push(json!({ "hooks": [{ "type": "command", "command": command(event), "timeout": timeout }] }));
        }
        if list.is_empty() {
            hooks.remove(*event);
        } else {
            hooks.insert((*event).to_string(), Value::Array(list));
        }
    }
    if hooks.is_empty() {
        root.remove("hooks");
    } else {
        root.insert("hooks".into(), Value::Object(hooks));
    }
    Ok(Value::Object(root))
}

/// Reads ~/.claude/settings.json (None when it doesn't exist yet).
pub fn read_settings() -> Result<Value, String> {
    match std::fs::read(settings_path()) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| format!("settings.json can't be read: {e}"))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Value::Null),
        Err(e) => Err(e.to_string()),
    }
}

/// Backs the file up (settings.json.bak-fubunotch-v2-<stamp>), then writes `new`.
/// Returns the backup's path ("" when there was no file).
pub fn write_settings(new: &Value, stamp: &str) -> Result<String, String> {
    let path = settings_path();
    let mut backup = String::new();
    if path.exists() {
        let b = path.with_file_name(format!("settings.json.bak-fubunotch-v2-{stamp}"));
        std::fs::copy(&path, &b).map_err(|e| format!("backup failed: {e}"))?;
        backup = b.display().to_string();
    } else if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(new).map_err(|e| e.to_string())?;
    let tmp = path.with_file_name("settings.json.fubunotch-v2.tmp");
    std::fs::write(&tmp, text + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(e: &str) -> String {
        format!("\"C:/x/fubunotch-v2-hook.exe\" {e}")
    }

    #[test]
    fn a_tool_call_in_a_few_words() {
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": "npm run build" }, "cwd": "C:\\work\\fubunotch-v2" });
        assert_eq!(tool_line(&p), "Bash · npm run build");
        assert_eq!(project(&p), "fubunotch-v2");
        let e = json!({ "tool_name": "Edit", "tool_input": { "file_path": "C:\\work\\app\\main.rs" } });
        assert_eq!(tool_line(&e), "Edit · main.rs");
    }

    #[test]
    fn a_permission_request_is_an_ask_with_allow_and_deny() {
        let p = json!({ "session_id": "s1", "tool_name": "Bash", "tool_input": { "command": "rm -rf build" }, "cwd": "C:\\w\\proj" });
        let Plan::Show(v) = plan("PermissionRequest", &p, "claude:s1:ask:1") else { panic!() };
        assert_eq!(v[0]["kind"], "ask");
        assert_eq!(v[0]["title"], "Allow Bash?");
        assert_eq!(v[0]["actions"][0]["id"], "allow");
        assert!(island_core::Incoming::parse(v[0].to_string().as_bytes()).unwrap().into_activity(0).is_ok(), "a valid activity");
    }

    #[test]
    fn stop_ends_the_work_and_says_finished() {
        let p = json!({ "session_id": "s1", "cwd": "C:\\w\\proj" });
        assert!(matches!(plan("Stop", &p, ""), Plan::EndThenShow(end, show) if end == ["claude:s1"] && show[0]["title"] == "proj · finished"));
        assert_eq!(plan("SessionEnd", &p, ""), Plan::End(vec![], Some("claude:s1".into())));
        assert_eq!(plan("SubagentStop", &p, ""), Plan::Nothing);
    }

    #[test]
    fn install_adds_ours_and_keeps_everything_else() {
        let user = json!({
            "outputStyle": "Fubuki",
            "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": "\"C:/v1/fubunotch-hook.exe\" Stop" }] }] }
        });
        let new = merged(&user, true, cmd).unwrap();
        assert_eq!(new["outputStyle"], "Fubuki");
        assert_eq!(new["hooks"]["Stop"].as_array().unwrap().len(), 2, "v1's entry kept, ours added");
        assert!(installed(&new));
        assert_eq!(merged(&new, true, cmd).unwrap(), new, "installing twice changes nothing");
        let removed = merged(&new, false, cmd).unwrap();
        assert_eq!(removed, user, "uninstall gives the user's file back");
        assert!(!installed(&removed));
    }

    #[test]
    fn a_strange_hooks_value_is_refused() {
        assert!(merged(&json!({ "hooks": [1] }), true, cmd).is_err());
        assert!(merged(&json!({ "hooks": { "Stop": "x" } }), true, cmd).is_err());
        assert!(merged(&Value::Null, true, cmd).is_ok(), "no file yet");
    }
}
