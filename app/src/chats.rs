// Settings > Accounts > Chats (NotchDeck's Chats tab, the owner 19 Nov 2026:
// "fitur notchdeck jangan ada yg kena nerf"): an account's chats, grouped by
// folder, newest first, copied into another account of the same app. Claude
// Code chats (<config>\projects\<project>\<id>.jsonl, plus the chat's side
// folder) keep their project folder so `claude --resume` finds them there;
// Claude Desktop Code-tab chats (claude-code-sessions\<uuid>\<org>\local_*.json)
// go into the other account's newest org folder. A chat the other account
// already has is never overwritten. Read from files on this PC only.

use serde_json::Value;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::accounts::ChatHome;

#[derive(Clone, Debug)]
pub struct Chat {
    /// Its file.
    pub id: String,
    /// The CLI session id (`claude --resume <sid>`), "" when unknown.
    pub sid: String,
    pub title: String,
    pub folder: String,
    /// Last change, ms since the epoch.
    pub date: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Folder {
    pub path: String,
    pub name: String,
    pub chats: Vec<Chat>,
}

fn clean(s: &str) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 90 {
        format!("{}…", one.chars().take(90).collect::<String>())
    } else {
        one
    }
}

fn mtime(p: &Path) -> u64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn ls(p: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(p).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

fn read_json(p: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

/// `n` bytes from the start (or, `tail`, the end) of a file, as text.
fn read_part(p: &Path, n: u64, tail: bool) -> String {
    let Ok(mut f) = std::fs::File::open(p) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if tail && len > n {
        let _ = f.seek(SeekFrom::Start(len - n));
    }
    let mut buf = Vec::new();
    let _ = f.take(n).read_to_end(&mut buf);
    String::from_utf8_lossy(&buf).into_owned()
}

/// Titles Claude Desktop keeps for the Code-tab chats it ran: session id → title.
fn desktop_titles(sessions: &Path) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for acc in ls(sessions) {
        for org in ls(&acc) {
            for f in ls(&org) {
                if f.file_name().is_some_and(|n| n.to_string_lossy().starts_with("local_")) {
                    if let Some(j) = read_json(&f) {
                        if let (Some(sid), Some(t)) = (j.get("cliSessionId").and_then(|x| x.as_str()), j.get("title").and_then(|x| x.as_str())) {
                            out.insert(sid.to_string(), t.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

#[derive(Default)]
struct Found {
    custom: Option<String>,
    ai: Option<String>,
    first: Option<String>,
    cwd: Option<String>,
}

fn scan(text: &str, f: &mut Found) {
    for line in text.lines() {
        let is_title = line.contains("\"custom-title\"") || line.contains("\"ai-title\"");
        let is_user = (f.first.is_none() || f.cwd.is_none()) && line.contains("\"type\":\"user\"");
        if !(is_title || is_user || (f.cwd.is_none() && line.contains("\"cwd\""))) {
            continue;
        }
        let Ok(o) = serde_json::from_str::<Value>(line) else { continue };
        if f.cwd.is_none() {
            if let Some(c) = o.get("cwd").and_then(|x| x.as_str()) {
                f.cwd = Some(c.to_string());
            }
        }
        if let Some(t) = o.get("customTitle").and_then(|x| x.as_str()) {
            f.custom = Some(t.to_string());
        }
        if let Some(t) = o.get("aiTitle").and_then(|x| x.as_str()) {
            f.ai = Some(t.to_string());
        }
        if f.first.is_none() && is_user {
            let c = o.get("message").and_then(|m| m.get("content"));
            let t = match c {
                Some(Value::String(s)) => Some(s.clone()),
                Some(Value::Array(a)) => a.iter().find(|e| e.get("type").and_then(|x| x.as_str()) == Some("text")).and_then(|e| e.get("text")).and_then(|x| x.as_str()).map(str::to_string),
                _ => None,
            };
            if let Some(t) = t.map(|t| t.trim().to_string()) {
                if !t.is_empty() && !t.starts_with('<') && !t.starts_with("Caveat:") {
                    f.first = Some(t);
                }
            }
        }
    }
}

/// Claude Code chats in `projects`, newest first (at most `limit`).
fn code_chats(projects: &Path, sessions: &Path, limit: usize) -> Vec<Chat> {
    let mut files: Vec<(PathBuf, u64, u64)> = Vec::new();
    for d in ls(projects) {
        for f in ls(&d) {
            if f.extension().is_some_and(|e| e == "jsonl") {
                let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                files.push((f.clone(), mtime(&f), size));
            }
        }
    }
    files.sort_by_key(|x| std::cmp::Reverse(x.1));
    let known = desktop_titles(sessions);
    let mut out = Vec::new();
    for (i, (p, t, size)) in files.into_iter().take(limit).enumerate() {
        if size == 0 {
            continue;
        }
        let mut f = Found::default();
        scan(&read_part(&p, 96_000, false), &mut f);
        scan(&read_part(&p, 24_000, true), &mut f);
        let sid = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if f.custom.is_none() && f.ai.is_none() {
            if let Some(k) = known.get(&sid) {
                f.custom = Some(k.clone());
            } else if i < 60 {
                scan(&read_part(&p, 400_000, true), &mut f);
            }
        }
        let title = f.custom.or(f.ai).or(f.first).unwrap_or_else(|| "Untitled chat".into());
        out.push(Chat { id: p.to_string_lossy().into_owned(), sid, title: clean(&title), folder: f.cwd.unwrap_or_default(), date: t, size });
    }
    out
}

/// Claude Desktop Code-tab chats of one signed-in account.
fn desktop_chats(sessions: &Path, uuid: &str) -> Vec<Chat> {
    let mut out = Vec::new();
    for org in ls(&sessions.join(uuid)) {
        for p in ls(&org) {
            let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !(n.starts_with("local_") && n.ends_with(".json")) {
                continue;
            }
            let Some(j) = read_json(&p) else { continue };
            if j.get("isArchived").and_then(|x| x.as_bool()) == Some(true) {
                continue;
            }
            let num = |k: &str| j.get(k).and_then(|x| x.as_u64().or_else(|| x.as_f64().map(|f| f as u64)).or_else(|| x.as_str().and_then(|s| s.parse().ok())));
            let date = num("lastActivityAt").or_else(|| num("createdAt")).unwrap_or(0);
            out.push(Chat {
                id: p.to_string_lossy().into_owned(),
                sid: j.get("cliSessionId").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                title: clean(j.get("title").and_then(|x| x.as_str()).unwrap_or("Untitled chat")),
                folder: j.get("cwd").and_then(|x| x.as_str()).unwrap_or("").to_string(),
                date,
                size: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
            });
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.date));
    out
}

/// The account's chats by folder, the folder with the newest chat first.
pub fn list(home: &ChatHome) -> Vec<Folder> {
    let chats = match home {
        ChatHome::Code(projects) => {
            let sessions = match crate::accounts::chat_sessions_dir() {
                Some(s) => s,
                None => PathBuf::new(),
            };
            code_chats(projects, &sessions, 400)
        }
        ChatHome::Desktop { sessions, uuid } => desktop_chats(sessions, uuid),
    };
    let mut folders: Vec<Folder> = Vec::new();
    for c in chats {
        match folders.iter_mut().find(|f| f.path == c.folder) {
            Some(f) => f.chats.push(c),
            None => {
                let name = if c.folder.is_empty() { "No folder".to_string() } else { c.folder.split(['\\', '/']).filter(|s| !s.is_empty()).last().unwrap_or(&c.folder).to_string() };
                folders.push(Folder { path: c.folder.clone(), name, chats: vec![c] });
            }
        }
    }
    for f in &mut folders {
        f.chats.sort_by_key(|c| std::cmp::Reverse(c.date));
    }
    folders.sort_by_key(|f| std::cmp::Reverse(f.chats.first().map_or(0, |c| c.date)));
    folders
}

fn copy_tree_no_overwrite(from: &Path, to: &Path) {
    let _ = std::fs::create_dir_all(to);
    for p in ls(from) {
        let dest = to.join(p.file_name().unwrap_or_default());
        if p.is_dir() {
            copy_tree_no_overwrite(&p, &dest);
        } else if !dest.exists() {
            let _ = std::fs::copy(&p, &dest);
        }
    }
}

/// Copies the chats `ids` from one account into another of the same app:
/// (copied, skipped). Only files inside the sending account's chat folder are
/// taken, and nothing the other account has is overwritten.
pub fn copy(ids: &[String], from: &ChatHome, to: &ChatHome) -> Result<(usize, usize), String> {
    let (mut copied, mut skipped) = (0, 0);
    match (from, to) {
        (ChatHome::Code(a), ChatHome::Code(b)) => {
            if a == b {
                return Err("that is the same account".into());
            }
            let base = a.canonicalize().map_err(|e| e.to_string())?;
            for id in ids {
                let Ok(src) = Path::new(id).canonicalize() else {
                    skipped += 1;
                    continue;
                };
                let Ok(rel) = src.strip_prefix(&base) else {
                    skipped += 1;
                    continue;
                };
                let dest = b.join(rel);
                if src.extension().is_none_or(|e| e != "jsonl") || dest.exists() {
                    skipped += 1;
                    continue;
                }
                if let Some(d) = dest.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                if std::fs::copy(&src, &dest).is_err() {
                    skipped += 1;
                    continue;
                }
                // The chat's side folder (subagents, saved tool output), when it has one.
                let side = src.with_extension("");
                if side.is_dir() {
                    copy_tree_no_overwrite(&side, &dest.with_extension(""));
                }
                copied += 1;
            }
        }
        (ChatHome::Desktop { sessions, .. }, ChatHome::Desktop { uuid: to_uuid, .. }) => {
            let base = sessions.canonicalize().map_err(|e| e.to_string())?;
            let to_dir = sessions.join(to_uuid);
            let newest = |org: &Path| ls(org).iter().map(|f| mtime(f)).max().unwrap_or(0);
            let mut orgs: Vec<PathBuf> = ls(&to_dir).into_iter().filter(|p| p.is_dir()).collect();
            orgs.sort_by_key(|o| std::cmp::Reverse(newest(o)));
            let Some(org) = orgs.first() else {
                return Err("the other account has no chat list yet: open a Code chat in it once".into());
            };
            for id in ids {
                let Ok(src) = Path::new(id).canonicalize() else {
                    skipped += 1;
                    continue;
                };
                if !src.starts_with(&base) {
                    skipped += 1;
                    continue;
                }
                let dest = org.join(src.file_name().unwrap_or_default());
                if dest.exists() || std::fs::copy(&src, &dest).is_err() {
                    skipped += 1;
                } else {
                    copied += 1;
                }
            }
        }
        _ => return Err("chats can only be copied between accounts of the same app".into()),
    }
    Ok((copied, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_code_chats_without_overwriting_and_keeps_project_and_side_folder() {
        let root = std::env::temp_dir().join(format!("fubunotch-chats-{}", std::process::id()));
        let (a, b) = (root.join("a").join("projects"), root.join("b").join("projects"));
        let proj = a.join("C--x");
        std::fs::create_dir_all(proj.join("s1").join("subagents")).unwrap();
        std::fs::write(proj.join("s1.jsonl"), r#"{"type":"user","cwd":"C:\\x","message":{"content":"hello there"}}"#.to_string() + "\n").unwrap();
        std::fs::write(proj.join("s1").join("subagents").join("t.jsonl"), "x").unwrap();
        std::fs::write(proj.join("s2.jsonl"), "{\"type\":\"user\",\"message\":{\"content\":\"two\"}}\n").unwrap();
        std::fs::create_dir_all(b.join("C--x")).unwrap();
        std::fs::write(b.join("C--x").join("s2.jsonl"), "theirs").unwrap();
        let folders = list(&ChatHome::Code(a.clone()));
        let ids: Vec<String> = folders.iter().flat_map(|f| f.chats.iter().map(|c| c.id.clone())).collect();
        assert_eq!(ids.len(), 2);
        assert!(folders.iter().any(|f| f.chats.iter().any(|c| c.title == "hello there" && c.folder == r"C:\x")));
        let (copied, skipped) = copy(&ids, &ChatHome::Code(a.clone()), &ChatHome::Code(b.clone())).unwrap();
        assert_eq!((copied, skipped), (1, 1));
        assert!(b.join("C--x").join("s1.jsonl").exists());
        assert!(b.join("C--x").join("s1").join("subagents").join("t.jsonl").exists());
        assert_eq!(std::fs::read_to_string(b.join("C--x").join("s2.jsonl")).unwrap(), "theirs");
        // A file outside the sending account is never taken.
        let outside = root.join("evil.jsonl");
        std::fs::write(&outside, "x").unwrap();
        let (c2, s2) = copy(&[outside.to_string_lossy().into_owned()], &ChatHome::Code(a), &ChatHome::Code(b)).unwrap();
        assert_eq!((c2, s2), (0, 1));
        let _ = std::fs::remove_dir_all(root);
    }
}

/// The chats of `ids` the other account already has (the same file at the
/// same place, as a copy would put it): hidden from the list, so what is
/// left is what has not been synced yet.
pub fn already_there(ids: &[&str], from: &ChatHome, to: &ChatHome) -> std::collections::HashSet<String> {
    let mut out = std::collections::HashSet::new();
    match (from, to) {
        (ChatHome::Code(a), ChatHome::Code(b)) if a != b => {
            for id in ids {
                if let Ok(rel) = Path::new(id).strip_prefix(a) {
                    if b.join(rel).exists() {
                        out.insert(id.to_string());
                    }
                }
            }
        }
        (ChatHome::Desktop { sessions, uuid: fu }, ChatHome::Desktop { uuid: tu, .. }) if fu != tu => {
            let orgs: Vec<PathBuf> = ls(&sessions.join(tu)).into_iter().filter(|p| p.is_dir()).collect();
            for id in ids {
                if let Some(name) = Path::new(id).file_name() {
                    if orgs.iter().any(|o| o.join(name).exists()) {
                        out.insert(id.to_string());
                    }
                }
            }
        }
        _ => {}
    }
    out
}
