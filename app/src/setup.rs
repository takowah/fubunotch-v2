// "Send setup to another account" (what NotchDeck / AccountDeck's Tools did,
// 19 Nov 2026: "kek sync sync skill gitu"): parts of one Claude
// Code account's setup copied into another. The same rules as NotchDeck's
// data.js envCopy: merge, never remove; what the destination already has
// stays (the model is the one exception: it is replaced); every file changed
// is backed up first; logins are never touched.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The parts, in order, with their names.
pub const PARTS: [(&str, &str); 8] = [
    ("model", "Model"),
    ("permissions", "Permissions"),
    ("mcp", "MCP servers"),
    ("plugins", "Plugins"),
    ("skills", "Skills"),
    ("hooks", "Hooks"),
    ("memory", "Memory"),
    ("env", "Environment"),
];

/// A Claude Code account's folders: its config folder and its state file
/// (`.claude.json`: inside the folder, or beside it for the default one).
#[derive(Clone, Debug)]
pub struct Slot {
    pub dir: PathBuf,
    pub state: PathBuf,
}

fn read(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

fn obj(v: Option<&Value>) -> Map<String, Value> {
    v.and_then(|x| x.as_object()).cloned().unwrap_or_default()
}

/// Writes `v` to `path`, a dated copy of what was there first.
fn write_with_backup(v: &Value, path: &Path) -> Result<(), String> {
    if path.exists() {
        // "2026-11-19-00-41-07": no ":" (not allowed in a Windows file name).
        let stamp = crate::local_time::stamp().replace([' ', ':'], "-");
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let _ = std::fs::copy(path, path.with_file_name(format!("{name}.bak-{stamp}")));
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("fubunotch-v2.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(v).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Copies a file or folder unless the destination exists; true when copied.
fn copy_no_overwrite(from: &Path, to: &Path) -> std::io::Result<bool> {
    if to.exists() {
        return Ok(false);
    }
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            copy_no_overwrite(&e.path(), &to.join(e.file_name()))?;
        }
    } else {
        if let Some(d) = to.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::copy(from, to)?;
    }
    Ok(true)
}

/// Keys of `src` missing in `dst` added; how many.
fn merge_keys(dst: &mut Map<String, Value>, src: &Map<String, Value>) -> usize {
    let mut n = 0;
    for (k, v) in src {
        if !dst.contains_key(k) {
            dst.insert(k.clone(), v.clone());
            n += 1;
        }
    }
    n
}

fn sort_deep(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.iter().map(sort_deep).collect()),
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let mut m = Map::new();
            for k in keys {
                m.insert(k.clone(), sort_deep(&o[k]));
            }
            Value::Object(m)
        }
        _ => v.clone(),
    }
}

fn plural(n: usize, w: &str) -> String {
    format!("{n} {w}{}", if n == 1 { "" } else { "s" })
}

fn listed(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| !n.starts_with('.')).collect()).unwrap_or_default();
    v.sort();
    v
}

/// What an account has, a line for each part: "12 skills", "CLAUDE.md".
pub fn summary(s: &Slot) -> Vec<(String, String)> {
    let settings = read(&s.dir.join("settings.json")).unwrap_or(Value::Null);
    let state = read(&s.state).unwrap_or(Value::Null);
    let perms = settings.get("permissions");
    let allow = perms.and_then(|p| p.get("allow")).and_then(|a| a.as_array()).map_or(0, |a| a.len());
    let plugins = obj(settings.get("enabledPlugins")).values().filter(|v| v.as_bool() == Some(true)).count();
    let line = |part: &str| -> String {
        match part {
            "model" => settings.get("model").and_then(|m| m.as_str()).unwrap_or("Default").to_string(),
            "permissions" => plural(allow, "allow rule"),
            "mcp" => plural(obj(state.get("mcpServers")).len(), "server"),
            "plugins" => plural(plugins, "plugin"),
            "skills" => plural(listed(&s.dir.join("skills")).len(), "skill"),
            "hooks" => plural(obj(settings.get("hooks")).len(), "event"),
            "memory" => if s.dir.join("CLAUDE.md").exists() { "CLAUDE.md".into() } else { "No CLAUDE.md".into() },
            "env" => plural(obj(settings.get("env")).len(), "variable"),
            _ => String::new(),
        }
    };
    PARTS.iter().map(|(id, name)| (name.to_string(), line(id))).collect()
}

/// Copies `parts` of `from`'s setup into `to`; what happened, part by part.
pub fn send(parts: &[&str], from: &Slot, to: &Slot) -> Result<Vec<String>, String> {
    if from.dir == to.dir {
        return Err("that is the same account".into());
    }
    let src = read(&from.dir.join("settings.json")).unwrap_or(Value::Object(Map::new()));
    let mut dst = read(&to.dir.join("settings.json")).unwrap_or(Value::Object(Map::new()));
    if !dst.is_object() {
        dst = Value::Object(Map::new());
    }
    let mut changed = false;
    let mut report = Vec::new();
    for (part, _) in PARTS {
        if !parts.contains(&part) {
            continue;
        }
        match part {
            "model" => match src.get("model") {
                Some(m) if !m.is_null() => {
                    dst["model"] = m.clone();
                    changed = true;
                    report.push("model".to_string());
                }
                _ => report.push("no model to send".into()),
            },
            "permissions" => {
                let mut p = obj(dst.get("permissions"));
                let sp = obj(src.get("permissions"));
                let mut added = 0;
                for key in ["allow", "deny", "ask", "additionalDirectories"] {
                    let mut have: Vec<Value> = p.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let more: Vec<Value> = sp.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default().into_iter().filter(|x| !have.contains(x)).collect();
                    if !more.is_empty() {
                        added += more.len();
                        have.extend(more);
                        p.insert(key.into(), Value::Array(have));
                    }
                }
                if !p.contains_key("defaultMode") {
                    if let Some(m) = sp.get("defaultMode") {
                        p.insert("defaultMode".into(), m.clone());
                        added += 1;
                    }
                }
                if added > 0 {
                    dst["permissions"] = Value::Object(p);
                    changed = true;
                }
                report.push(plural(added, "permission rule"));
            }
            "plugins" => {
                let mut ep = obj(dst.get("enabledPlugins"));
                let n = merge_keys(&mut ep, &obj(src.get("enabledPlugins")));
                let mut mk = obj(dst.get("extraKnownMarketplaces"));
                let n2 = merge_keys(&mut mk, &obj(src.get("extraKnownMarketplaces")));
                if n > 0 {
                    dst["enabledPlugins"] = Value::Object(ep);
                }
                if n2 > 0 {
                    dst["extraKnownMarketplaces"] = Value::Object(mk);
                }
                changed |= n + n2 > 0;
                report.push(plural(n, "plugin"));
            }
            "hooks" => {
                let mut h = obj(dst.get("hooks"));
                let mut added = 0;
                for (event, list) in obj(src.get("hooks")) {
                    let mut have: Vec<Value> = h.get(&event).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                    let seen: Vec<Value> = have.iter().map(sort_deep).collect();
                    for entry in list.as_array().cloned().unwrap_or_default() {
                        if !seen.contains(&sort_deep(&entry)) {
                            have.push(entry);
                            added += 1;
                        }
                    }
                    h.insert(event, Value::Array(have));
                }
                if added > 0 {
                    dst["hooks"] = Value::Object(h);
                    changed = true;
                }
                report.push(plural(added, "hook"));
            }
            "env" => {
                let mut e = obj(dst.get("env"));
                let n = merge_keys(&mut e, &obj(src.get("env")));
                if n > 0 {
                    dst["env"] = Value::Object(e);
                    changed = true;
                }
                report.push(plural(n, "variable"));
            }
            "mcp" => {
                let mut state = read(&to.state).filter(|v| v.is_object()).unwrap_or(Value::Object(Map::new()));
                let mut m = obj(state.get("mcpServers"));
                let n = merge_keys(&mut m, &obj(read(&from.state).as_ref().and_then(|v| v.get("mcpServers"))));
                if n > 0 {
                    state["mcpServers"] = Value::Object(m);
                    write_with_backup(&state, &to.state)?;
                }
                report.push(plural(n, "MCP server"));
            }
            "skills" => {
                let (f, t) = (from.dir.join("skills"), to.dir.join("skills"));
                let mut n = 0;
                for name in listed(&f) {
                    if copy_no_overwrite(&f.join(&name), &t.join(&name)).unwrap_or(false) {
                        n += 1;
                    }
                }
                report.push(plural(n, "skill"));
            }
            "memory" => {
                let (s, d) = (from.dir.join("CLAUDE.md"), to.dir.join("CLAUDE.md"));
                if !s.exists() {
                    report.push("no CLAUDE.md to send".into());
                } else if d.exists() {
                    report.push("CLAUDE.md kept (it has its own)".into());
                } else {
                    std::fs::create_dir_all(&to.dir).map_err(|e| e.to_string())?;
                    std::fs::copy(&s, &d).map_err(|e| e.to_string())?;
                    report.push("CLAUDE.md".into());
                }
            }
            _ => {}
        }
    }
    if changed {
        write_with_backup(&dst, &to.dir.join("settings.json"))?;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(base: &Path, name: &str) -> Slot {
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        Slot { state: dir.join(".claude.json"), dir }
    }

    #[test]
    fn sending_merges_and_never_overwrites() {
        let base = std::env::temp_dir().join(format!("fubunotch-setup-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (slot(&base, "a"), slot(&base, "b"));
        std::fs::write(a.dir.join("settings.json"), r#"{"model":"opus","permissions":{"allow":["Bash(ls)","Read"]},"env":{"X":"1","Y":"2"},
            "hooks":{"Stop":[{"hooks":[{"type":"command","command":"a"}]}]},"enabledPlugins":{"p@m":true}}"#).unwrap();
        std::fs::write(b.dir.join("settings.json"), r#"{"permissions":{"allow":["Read"]},"env":{"X":"keep"},
            "hooks":{"Stop":[{"hooks":[{"command":"a","type":"command"}]}]}}"#).unwrap();
        std::fs::write(&a.state, r#"{"mcpServers":{"one":{"command":"x"},"two":{"url":"https://y"}}}"#).unwrap();
        std::fs::write(&b.state, r#"{"mcpServers":{"one":{"command":"mine"}},"oauthAccount":{"accountUuid":"keep"}}"#).unwrap();
        std::fs::create_dir_all(a.dir.join("skills/s1")).unwrap();
        std::fs::write(a.dir.join("skills/s1/SKILL.md"), "a").unwrap();
        std::fs::create_dir_all(b.dir.join("skills/s2")).unwrap();
        std::fs::write(a.dir.join("CLAUDE.md"), "mem").unwrap();

        let all: Vec<&str> = PARTS.iter().map(|p| p.0).collect();
        let r = send(&all, &a, &b).unwrap();
        assert_eq!(r, ["model", "1 permission rule", "1 MCP server", "1 plugin", "1 skill", "0 hooks", "CLAUDE.md", "1 variable"]);

        let s: Value = read(&b.dir.join("settings.json")).unwrap();
        assert_eq!(s["model"], "opus", "the model is replaced");
        assert_eq!(s["permissions"]["allow"], serde_json::json!(["Read", "Bash(ls)"]));
        assert_eq!(s["env"]["X"], "keep", "kept");
        assert_eq!(s["env"]["Y"], "2");
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1, "the same hook (keys in another order) not doubled");
        let st: Value = read(&b.state).unwrap();
        assert_eq!(st["mcpServers"]["one"]["command"], "mine", "kept");
        assert_eq!(st["mcpServers"]["two"]["url"], "https://y");
        assert_eq!(st["oauthAccount"]["accountUuid"], "keep", "the login untouched");
        assert!(b.dir.join("skills/s1/SKILL.md").exists() && b.dir.join("skills/s2").exists());
        assert!(std::fs::read_dir(&b.dir).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("settings.json.bak-")), "backed up");
        // Again: nothing more.
        let r = send(&["skills", "mcp", "memory"], &a, &b).unwrap();
        assert_eq!(r, ["0 MCP servers", "0 skills", "CLAUDE.md kept (it has its own)"]);
        assert!(send(&all, &a, &a).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }
}

/// One MCP connection an account can send (NotchDeck's MCP list): its own
/// ("user"), a project's, or Claude Desktop's (shared on this PC).
#[derive(Clone, Debug)]
pub struct McpEntry {
    pub id: String,
    pub name: String,
    /// "user" | "project" | "desktop".
    pub scope: String,
    pub project: String,
    /// "Local", "HTTP", "SSE".
    pub kind: String,
    /// The host, or the command and its first argument.
    pub summary: String,
}

fn mcp_kind(c: &Value) -> String {
    match c.get("type").and_then(|t| t.as_str()) {
        Some("stdio") => "Local".into(),
        Some(t) => t.to_uppercase(),
        None if c.get("url").is_some() => "HTTP".into(),
        None => "Local".into(),
    }
}

fn mcp_summary(c: &Value) -> String {
    if let Some(u) = c.get("url").and_then(|u| u.as_str()) {
        return u.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(u).to_string();
    }
    let base = |s: &str| s.rsplit(['\\', '/']).next().unwrap_or(s).to_string();
    let cmd = c.get("command").and_then(|x| x.as_str()).map(base).unwrap_or_default();
    let arg = c.get("args").and_then(|a| a.as_array()).and_then(|a| a.iter().filter_map(|x| x.as_str()).find(|x| !x.starts_with('-'))).map(base).unwrap_or_default();
    [cmd, arg].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn sorted_keys(m: &Map<String, Value>) -> Vec<String> {
    let mut k: Vec<String> = m.keys().cloned().collect();
    k.sort_by_key(|a| a.to_lowercase());
    k
}

/// Every MCP connection `s` has: its own, its projects', and Claude Desktop's.
pub fn mcp_entries(s: &Slot) -> Vec<McpEntry> {
    let mut out = Vec::new();
    let mut add = |name: &str, scope: &str, project: &str, c: &Value| {
        let id = if scope == "project" { format!("project:{project}/{name}") } else { format!("{scope}/{name}") };
        out.push(McpEntry { id, name: name.into(), scope: scope.into(), project: project.into(), kind: mcp_kind(c), summary: mcp_summary(c) });
    };
    let st = read(&s.state).unwrap_or(Value::Null);
    let user = obj(st.get("mcpServers"));
    for k in sorted_keys(&user) {
        add(&k, "user", "", &user[&k]);
    }
    let projects = obj(st.get("projects"));
    let mut pk: Vec<&String> = projects.keys().collect();
    pk.sort();
    for p in pk {
        let servers = obj(projects[p].get("mcpServers"));
        for k in sorted_keys(&servers) {
            add(&k, "project", p, &servers[&k]);
        }
    }
    let d = read(&crate::accounts::desktop_config()).unwrap_or(Value::Null);
    let desk = obj(d.get("mcpServers"));
    for k in sorted_keys(&desk) {
        add(&k, "desktop", "", &desk[&k]);
    }
    out
}

/// Adds the connections `ids` (from `mcp_entries(from)`) to `to`; ones it
/// already has by that name stay. (added, skipped). Backed up first.
pub fn mcp_send(ids: &[String], from: &Slot, to: &Slot) -> Result<(usize, usize), String> {
    let src = read(&from.state).unwrap_or(Value::Null);
    let desk = read(&crate::accounts::desktop_config()).unwrap_or(Value::Null);
    let mut dst = read(&to.state).unwrap_or(Value::Object(Map::new()));
    if !dst.is_object() {
        dst = Value::Object(Map::new());
    }
    let mut user = obj(dst.get("mcpServers"));
    let mut projects = obj(dst.get("projects"));
    let (mut added, mut skipped) = (0, 0);
    for e in mcp_entries(from).into_iter().filter(|e| ids.contains(&e.id)) {
        let cfg = match e.scope.as_str() {
            "user" => src.get("mcpServers").and_then(|m| m.get(&e.name)).cloned(),
            "project" => src.get("projects").and_then(|p| p.get(&e.project)).and_then(|p| p.get("mcpServers")).and_then(|m| m.get(&e.name)).cloned(),
            _ => desk.get("mcpServers").and_then(|m| m.get(&e.name)).cloned(),
        };
        let Some(cfg) = cfg else {
            skipped += 1;
            continue;
        };
        if e.scope == "project" {
            let mut proj = obj(projects.get(&e.project));
            let mut servers = obj(proj.get("mcpServers"));
            if servers.contains_key(&e.name) {
                skipped += 1;
                continue;
            }
            servers.insert(e.name.clone(), cfg);
            proj.insert("mcpServers".into(), Value::Object(servers));
            projects.insert(e.project.clone(), Value::Object(proj));
        } else {
            if user.contains_key(&e.name) {
                skipped += 1;
                continue;
            }
            user.insert(e.name.clone(), cfg);
        }
        added += 1;
    }
    if added > 0 {
        dst["mcpServers"] = Value::Object(user);
        if !projects.is_empty() {
            dst["projects"] = Value::Object(projects);
        }
        write_with_backup(&dst, &to.state)?;
    }
    Ok((added, skipped))
}

#[cfg(test)]
mod mcp_tests {
    use super::*;

    #[test]
    fn sends_picked_servers_and_keeps_the_ones_there() {
        let root = std::env::temp_dir().join(format!("fubunotch-mcp-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let from = Slot { dir: root.join("a"), state: root.join("a.json") };
        let to = Slot { dir: root.join("b"), state: root.join("b.json") };
        std::fs::write(&from.state, r#"{"mcpServers":{"one":{"url":"https://x.io/mcp"},"two":{"command":"node","args":["s.js"]}},"projects":{"C:/p":{"mcpServers":{"three":{"command":"uvx","args":["t"]}}}}}"#).unwrap();
        std::fs::write(&to.state, r#"{"mcpServers":{"two":{"command":"mine"}},"oauthAccount":{"k":1}}"#).unwrap();
        let ids: Vec<String> = mcp_entries(&from).into_iter().filter(|e| e.scope != "desktop").map(|e| e.id).collect();
        assert_eq!(ids.len(), 3);
        let (added, skipped) = mcp_send(&ids, &from, &to).unwrap();
        assert_eq!((added, skipped), (2, 1));
        let v: Value = serde_json::from_slice(&std::fs::read(&to.state).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["two"]["command"], "mine");
        assert_eq!(v["mcpServers"]["one"]["url"], "https://x.io/mcp");
        assert_eq!(v["projects"]["C:/p"]["mcpServers"]["three"]["command"], "uvx");
        assert_eq!(v["oauthAccount"]["k"], 1);
        let _ = std::fs::remove_dir_all(root);
    }
}

/// How much of each part `send` would still add to `to` (0 = synced), by the
/// same rules as `send`: the model differs, rules/plugins/hooks/variables/
/// servers/skills it lacks, a CLAUDE.md where it has none.
pub fn missing(from: &Slot, to: &Slot) -> Vec<(String, usize)> {
    let src = read(&from.dir.join("settings.json")).unwrap_or(Value::Null);
    let dst = read(&to.dir.join("settings.json")).unwrap_or(Value::Null);
    let keys_missing = |a: Map<String, Value>, b: Map<String, Value>| a.keys().filter(|k| !b.contains_key(*k)).count();
    PARTS
        .iter()
        .map(|(part, _)| {
            let n = match *part {
                "model" => usize::from(src.get("model").is_some_and(|m| !m.is_null() && dst.get("model") != Some(m))),
                "permissions" => {
                    let (sp, dp) = (obj(src.get("permissions")), obj(dst.get("permissions")));
                    let mut n = 0;
                    for key in ["allow", "deny", "ask", "additionalDirectories"] {
                        let have: Vec<Value> = dp.get(key).and_then(|v| v.as_array()).cloned().unwrap_or_default();
                        n += sp.get(key).and_then(|v| v.as_array()).map_or(0, |a| a.iter().filter(|x| !have.contains(x)).count());
                    }
                    n + usize::from(sp.contains_key("defaultMode") && !dp.contains_key("defaultMode"))
                }
                "plugins" => keys_missing(obj(src.get("enabledPlugins")), obj(dst.get("enabledPlugins"))) + keys_missing(obj(src.get("extraKnownMarketplaces")), obj(dst.get("extraKnownMarketplaces"))),
                "hooks" => {
                    let dh = obj(dst.get("hooks"));
                    obj(src.get("hooks"))
                        .iter()
                        .map(|(event, list)| {
                            let seen: Vec<Value> = dh.get(event).and_then(|v| v.as_array()).map(|a| a.iter().map(sort_deep).collect()).unwrap_or_default();
                            list.as_array().map_or(0, |l| l.iter().filter(|e| !seen.contains(&sort_deep(e))).count())
                        })
                        .sum()
                }
                "env" => keys_missing(obj(src.get("env")), obj(dst.get("env"))),
                "mcp" => keys_missing(obj(read(&from.state).as_ref().and_then(|v| v.get("mcpServers"))), obj(read(&to.state).as_ref().and_then(|v| v.get("mcpServers")))),
                "skills" => listed(&from.dir.join("skills")).iter().filter(|n| !to.dir.join("skills").join(n).exists()).count(),
                "memory" => usize::from(from.dir.join("CLAUDE.md").exists() && !to.dir.join("CLAUDE.md").exists()),
                _ => 0,
            };
            (part.to_string(), n)
        })
        .collect()
}

/// Whether `to` already has the MCP entry (by its name, where a send puts it).
pub fn mcp_there(e: &McpEntry, to: &Slot) -> bool {
    let st = read(&to.state).unwrap_or(Value::Null);
    if e.scope == "project" {
        st.get("projects").and_then(|p| p.get(&e.project)).and_then(|p| p.get("mcpServers")).and_then(|m| m.get(&e.name)).is_some()
    } else {
        st.get("mcpServers").and_then(|m| m.get(&e.name)).is_some()
    }
}

#[cfg(test)]
mod missing_tests {
    use super::*;

    #[test]
    fn nothing_missing_after_a_send() {
        let base = std::env::temp_dir().join(format!("fubunotch-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let mk = |n: &str| {
            let dir = base.join(n);
            std::fs::create_dir_all(&dir).unwrap();
            Slot { state: dir.join(".claude.json"), dir }
        };
        let (a, b) = (mk("a"), mk("b"));
        std::fs::write(a.dir.join("settings.json"), r#"{"model":"opus","permissions":{"allow":["Read","Bash(ls)"]},"env":{"X":"1"},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"a"}]}]},"enabledPlugins":{"p@m":true}}"#).unwrap();
        std::fs::write(&a.state, r#"{"mcpServers":{"one":{"command":"x"}}}"#).unwrap();
        std::fs::create_dir_all(a.dir.join("skills/s1")).unwrap();
        std::fs::write(a.dir.join("CLAUDE.md"), "m").unwrap();
        let before: usize = missing(&a, &b).iter().map(|x| x.1).sum();
        assert_eq!(before, 1 + 2 + 1 + 1 + 1 + 1 + 1 + 1);
        let all: Vec<&str> = PARTS.iter().map(|p| p.0).collect();
        send(&all, &a, &b).unwrap();
        assert!(missing(&a, &b).iter().all(|x| x.1 == 0), "{:?}", missing(&a, &b));
        let _ = std::fs::remove_dir_all(&base);
    }
}
