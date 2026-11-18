// Accounts: the Claude Desktop, Claude Code and Codex accounts that NotchDeck
// (an AccountDeck fork) manages, read from its state file, and a switch that
// asks NotchDeck to do it. NotchDeck owns the switching (closing Claude
// Desktop, swapping its login, the CLAUDE_CONFIG_DIR / CODEX_HOME user
// variables); the island only shows the list and passes the click on.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: String,
    pub service: String,
    pub name: String,
    pub active: bool,
}

/// The services shown, in order, with their titles.
pub const SERVICES: [(&str, &str); 3] = [("desktop", "Claude Desktop"), ("claude", "Claude Code"), ("codex", "Codex")];

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_default()
}

/// NotchDeck's state: %USERPROFILE%\.accountdeck\state.json.
fn state_path() -> PathBuf {
    home().join(".accountdeck").join("state.json")
}

/// NotchDeck as the per-user installer puts it.
fn notchdeck_exe() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("Programs").join("NotchDeck").join("NotchDeck.exe");
    p.exists().then_some(p)
}

pub fn installed() -> bool {
    notchdeck_exe().is_some()
}

/// The accounts in NotchDeck's order; empty when NotchDeck isn't set up.
pub fn load() -> Vec<Account> {
    std::fs::read(state_path()).map(|b| parse(&b)).unwrap_or_default()
}

pub fn parse(bytes: &[u8]) -> Vec<Account> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else { return Vec::new() };
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
            Some(Account { id, service, name, active: is_active })
        })
        .collect()
}

/// An id NotchDeck made: letters, digits and dashes only (it goes on a command line).
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Asks NotchDeck to switch to the account. It confirms Claude Desktop restarts itself.
pub fn switch(id: &str) -> Result<(), String> {
    if !safe_id(id) {
        return Err(format!("not an account id: {id}"));
    }
    let exe = notchdeck_exe().ok_or("NotchDeck is not installed")?;
    std::process::Command::new(exe).arg(format!("--switch={id}")).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_accounts_and_the_active_one() {
        let s = br#"{"accounts":[
            {"id":"desktop-default","service":"desktop","name":"Daffa Max","isDefault":true},
            {"id":"claude-default","service":"claude","name":"Partner"},
            {"id":"claude-daffa","service":"claude","name":"Daffa"},
            {"id":"chatgpt-x","service":"chatgpt","name":"Web"}],
            "active":{"desktop":"desktop-default","claude":"claude-daffa"}}"#;
        let a = parse(s);
        assert_eq!(a.len(), 3, "chatgpt is left out");
        assert!(a[0].active);
        assert!(!a[1].active);
        assert!(a[2].active);
        assert_eq!(a[2].name, "Daffa");
    }

    #[test]
    fn garbage_gives_nothing() {
        assert!(parse(b"").is_empty());
        assert!(parse(b"{}").is_empty());
        assert!(parse(b"[1,2]").is_empty());
    }

    #[test]
    fn ids_that_could_break_the_command_line_are_refused() {
        assert!(safe_id("claude-daffa-kyou-team"));
        assert!(!safe_id(""));
        assert!(!safe_id("a b"));
        assert!(!safe_id("x\" --evil"));
        assert!(!safe_id("..\\x"));
    }
}
