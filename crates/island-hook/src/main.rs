//! fubunotch-v2-hook: what Claude Code runs on each hook event.
//!
//!   fubunotch-v2-hook <EventName>   (the event's JSON on stdin)
//!
//! It posts the JSON to FubuNotch v2's webhook (127.0.0.1:47821/claude/<Event>)
//! and, for a PermissionRequest, waits for the island's answer and prints the
//! documented `hookSpecificOutput`. The rules:
//! - never block Claude Code: if the app isn't running, it exits at once;
//! - never allow anything a human didn't click: no answer prints nothing, and
//!   Claude Code asks in its terminal as if FubuNotch weren't there.


use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

const PORT: u16 = 47821;
const CONNECT: Duration = Duration::from_millis(300);
/// Longer than the app's own wait (110 s), shorter than the hook timeout (120 s).
const DECISION: Duration = Duration::from_secs(115);
const QUICK: Duration = Duration::from_secs(3);
const STDIN_LIMIT: u64 = 1024 * 1024;

fn token() -> Option<String> {
    let dir = std::env::var_os("APPDATA")?;
    let path = std::path::Path::new(&dir).join("FubuNotch v2").join("token");
    let t = std::fs::read_to_string(path).ok()?.trim().to_string();
    (t.len() >= 32).then_some(t)
}

/// POST `body` to the webhook; the response body, or None on any failure.
fn post(event: &str, body: &[u8], token: &str, wait: Duration) -> Option<String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], PORT));
    let mut stream = TcpStream::connect_timeout(&addr, CONNECT).ok()?;
    stream.set_read_timeout(Some(wait)).ok()?;
    stream.set_write_timeout(Some(QUICK)).ok()?;
    let head = format!(
        "POST /claude/{event} HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).ok()?;
    stream.write_all(body).ok()?;
    let mut answer = Vec::new();
    stream.take(64 * 1024).read_to_end(&mut answer).ok()?;
    let text = String::from_utf8_lossy(&answer);
    let (status_line, rest) = text.split_once("\r\n")?;
    if !status_line.contains(" 200") {
        return None;
    }
    Some(rest.split_once("\r\n\r\n").map(|(_, b)| b.trim().to_string()).unwrap_or_default())
}

/// The documented PermissionRequest output for a clicked "allow" / "deny".
fn decision_json(decision: &str) -> Option<String> {
    let behavior = match decision.trim() {
        "allow" => r#"{"behavior":"allow"}"#,
        "deny" => r#"{"behavior":"deny","message":"Denied from FubuNotch v2"}"#,
        _ => return None,
    };
    Some(format!(r#"{{"hookSpecificOutput":{{"hookEventName":"PermissionRequest","decision":{behavior}}}}}"#))
}

/// An event name as Claude Code passes it: letters only.
fn clean_event(arg: Option<String>) -> Option<String> {
    arg.filter(|e| !e.is_empty() && e.len() <= 64 && e.chars().all(|c| c.is_ascii_alphanumeric()))
}

fn main() {
    let Some(event) = clean_event(std::env::args().nth(1)) else { return };
    let mut body = Vec::new();
    if std::io::stdin().take(STDIN_LIMIT).read_to_end(&mut body).is_err() || body.is_empty() {
        return;
    }
    let Some(token) = token() else { return };
    let asks = event == "PermissionRequest";
    let answer = post(&event, &body, &token, if asks { DECISION } else { QUICK });
    if asks {
        if let Some(out) = answer.as_deref().and_then(decision_json) {
            println!("{out}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_clicked_answer_prints_anything() {
        assert_eq!(
            decision_json("allow").as_deref(),
            Some(r#"{"hookSpecificOutput":{"hookEventName":"PermissionRequest","decision":{"behavior":"allow"}}}"#)
        );
        assert!(decision_json("deny").unwrap().contains(r#""behavior":"deny""#));
        assert_eq!(decision_json(""), None, "no click: silence, Claude Code asks itself");
        assert_eq!(decision_json("always allow everything"), None);
    }

    #[test]
    fn event_names_are_plain_words() {
        assert_eq!(clean_event(Some("PreToolUse".into())).as_deref(), Some("PreToolUse"));
        assert_eq!(clean_event(Some("../x".into())), None);
        assert_eq!(clean_event(None), None);
    }

    #[test]
    fn no_app_means_an_instant_no() {
        // Nothing listens on this port in a test: the relay gives up at once.
        let started = std::time::Instant::now();
        let addr = SocketAddr::from(([127, 0, 0, 1], 9));
        assert!(TcpStream::connect_timeout(&addr, CONNECT).is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
