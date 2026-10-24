//! The local webhook (docs/SPEC-draft.md): anything on this PC can show an
//! activity by posting JSON to 127.0.0.1. `handle` is the whole protocol as a
//! pure function; `spawn` serves it on a thread.

use std::io::Read;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::time::Duration;

use island_core::{Activity, Incoming};

pub const PORT: u16 = 47821;
pub const BODY_LIMIT: usize = 64 * 1024;
/// A Claude Code hook carries the tool's input, which may be a whole file.
pub const HOOK_BODY_LIMIT: usize = 1024 * 1024;
/// How long a permission request waits for a click. Claude Code's hook timeout
/// is set a little longer, so the relay always answers first.
pub const DECISION_WAIT: Duration = Duration::from_secs(110);

/// Where the island's answer to a permission request goes: "allow", "deny",
/// or nothing (dropped) to let Claude Code ask in its terminal.
pub struct Reply(pub Sender<String>);

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

/// YouTube Music's own shuffle and repeat, reported by the browser extension
/// (extension/ytm): Windows' media controls don't carry them for a browser.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct YtmState {
    pub shuffle: bool,
    /// "none", "all" or "one".
    pub repeat: String,
    #[serde(default)]
    pub title: String,
}

/// Commands waiting for the extension ("shuffle", "repeat"); it collects them
/// with a long poll on /ytm/command.
static YTM_COMMANDS: (std::sync::Mutex<std::collections::VecDeque<String>>, std::sync::Condvar) =
    (std::sync::Mutex::new(std::collections::VecDeque::new()), std::sync::Condvar::new());
/// How long /ytm/command waits for a command before answering "none" (204).
pub const YTM_POLL: Duration = Duration::from_secs(20);

/// Queues a command for the YouTube Music extension.
pub fn ytm_command(command: &str) {
    let (lock, ready) = &YTM_COMMANDS;
    let mut q = lock.lock().unwrap_or_else(|e| e.into_inner());
    q.push_back(command.to_string());
    while q.len() > 4 {
        q.pop_front();
    }
    ready.notify_all();
}

/// The next command, waiting up to `wait` for one.
fn ytm_next(wait: Duration) -> Option<String> {
    let (lock, ready) = &YTM_COMMANDS;
    let q = lock.lock().unwrap_or_else(|e| e.into_inner());
    let (mut q, _) = ready.wait_timeout_while(q, wait, |q| q.is_empty()).unwrap_or_else(|e| e.into_inner());
    q.pop_front()
}

/// What a request asks of the island.
#[derive(Debug)]
pub enum Event {
    /// Create or update; `updated_at` is restamped by the app's clock.
    Upsert(Activity),
    End(String),
    /// A Claude Code hook (relayed by fubunotch-v2-hook): its event name and
    /// payload; a permission request also carries where its answer goes.
    Claude { event: String, payload: serde_json::Value, reply: Option<Reply> },
    /// The YouTube Music extension's report.
    Ytm(YtmState),
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Event::Upsert(a), Event::Upsert(b)) => a == b,
            (Event::End(a), Event::End(b)) => a == b,
            (Event::Claude { event: a, payload: pa, .. }, Event::Claude { event: b, payload: pb, .. }) => a == b && pa == pb,
            (Event::Ytm(a), Event::Ytm(b)) => a == b,
            _ => false,
        }
    }
}

/// A request, reduced to what the protocol looks at.
pub struct Request<'a> {
    /// The Origin header: /ytm/ answers browser extensions only.
    pub origin: Option<&'a str>,
    pub method: &'a str,
    pub path: &'a str,
    pub authorization: Option<&'a str>,
    pub content_type: Option<&'a str>,
    pub body: &'a [u8],
}

#[derive(Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

fn reply(status: u16, body: impl Into<String>) -> (Response, Option<Event>) {
    (Response { status, body: body.into() }, None)
}

/// Equal without leaking where they differ through timing.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The protocol. `token` is the secret every request but /health must carry.
pub fn handle(req: &Request, token: &str) -> (Response, Option<Event>) {
    if req.method == "GET" && req.path == "/health" {
        return reply(200, "ok");
    }
    // The YouTube Music extension has no token: it is told apart by its
    // Origin, which no web page can claim.
    if let Some(rest) = req.path.strip_prefix("/ytm/") {
        if !req.origin.is_some_and(|o| o.starts_with("chrome-extension://")) {
            return reply(403, "extensions only");
        }
        return match (req.method, rest) {
            ("POST", "state") => {
                if req.body.len() > BODY_LIMIT {
                    return reply(413, "body over 64 KB");
                }
                match serde_json::from_slice::<YtmState>(req.body) {
                    Ok(state) => (Response { status: 200, body: "ok".into() }, Some(Event::Ytm(state))),
                    Err(err) => reply(400, err.to_string()),
                }
            }
            // Answered by `spawn` after a wait (a long poll).
            ("GET", "command") => reply(204, ""),
            _ => reply(404, "not found"),
        };
    }
    let presented = req.authorization.and_then(|h| h.strip_prefix("Bearer ")).unwrap_or("");
    if token.is_empty() || !same(presented.trim().as_bytes(), token.as_bytes()) {
        return reply(401, "missing or wrong token");
    }
    match (req.method, req.path) {
        ("POST", "/activity") => {
            // Browsers can't send this to localhost without a preflight we never answer.
            let json = req.content_type.is_some_and(|c| c.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"));
            if !json {
                return reply(415, "Content-Type must be application/json");
            }
            if req.body.len() > BODY_LIMIT {
                return reply(413, "body over 64 KB");
            }
            // (Claude Code hooks have their own, larger limit below.)
            match Incoming::parse(req.body).and_then(|i| i.into_activity(0)) {
                Ok(activity) => (Response { status: 200, body: "ok".into() }, Some(Event::Upsert(activity))),
                Err(err) => reply(400, err.to_string()),
            }
        }
        ("POST", path) if path.starts_with("/claude/") => {
            let event = &path["/claude/".len()..];
            if event.is_empty() || event.len() > 64 || !event.chars().all(|c| c.is_ascii_alphanumeric()) {
                return reply(400, "bad event name");
            }
            if req.body.len() > HOOK_BODY_LIMIT {
                return reply(413, "body over 1 MB");
            }
            match serde_json::from_slice::<serde_json::Value>(req.body) {
                Ok(payload) => (
                    Response { status: 200, body: String::new() },
                    Some(Event::Claude { event: event.to_string(), payload, reply: None }),
                ),
                Err(err) => reply(400, format!("not JSON: {err}")),
            }
        }
        ("DELETE", path) if path.starts_with("/activity/") => {
            let id = &path["/activity/".len()..];
            if id.is_empty() {
                return reply(400, "no id");
            }
            (Response { status: 200, body: "ok".into() }, Some(Event::End(percent_decode(id))))
        }
        _ => reply(404, "not found"),
    }
}

/// "%2F" → "/" (ids may hold anything).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The token in `dir/token`, made (32 random bytes, hex) the first time.
pub fn load_or_create_token(dir: &Path) -> std::io::Result<String> {
    let file = dir.join("token");
    if let Ok(existing) = std::fs::read_to_string(&file) {
        let t = existing.trim().to_string();
        if t.len() >= 32 {
            return Ok(t);
        }
    }
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&file, &token)?;
    Ok(token)
}

/// Serves the webhook on 127.0.0.1:`port` on a thread of its own; every
/// accepted request's event goes to `on_event`.
pub fn spawn(port: u16, token: String, on_event: impl Fn(Event) + Send + 'static) -> std::io::Result<std::thread::JoinHandle<()>> {
    let server = tiny_http::Server::http(("127.0.0.1", port)).map_err(|e| std::io::Error::other(e.to_string()))?;
    std::thread::Builder::new().name("island-webhook".into()).spawn(move || {
        for mut request in server.incoming_requests() {
            let mut authorization = None;
            let mut content_type = None;
            let mut origin = None;
            for h in request.headers() {
                if h.field.equiv("Authorization") {
                    authorization = Some(h.value.as_str().to_string());
                } else if h.field.equiv("Content-Type") {
                    content_type = Some(h.value.as_str().to_string());
                } else if h.field.equiv("Origin") {
                    origin = Some(h.value.as_str().to_string());
                }
            }
            let mut body = Vec::new();
            let too_big = request.as_reader().take(HOOK_BODY_LIMIT as u64 + 1).read_to_end(&mut body).is_err();
            let method = request.method().as_str().to_string();
            let path = request.url().split('?').next().unwrap_or("").to_string();
            let (response, event) = if too_big {
                reply(400, "unreadable body")
            } else {
                handle(
                    &Request { origin: origin.as_deref(), method: &method, path: &path, authorization: authorization.as_deref(), content_type: content_type.as_deref(), body: &body },
                    &token,
                )
            };
            // The extension's long poll: answered when a command comes, or
            // empty after YTM_POLL, on a thread of its own.
            if method == "GET" && path == "/ytm/command" && response.status == 204 {
                std::thread::spawn(move || {
                    let answer = match ytm_next(YTM_POLL) {
                        Some(c) => tiny_http::Response::from_string(serde_json::json!({ "command": c }).to_string()).with_status_code(200),
                        None => tiny_http::Response::from_string(String::new()).with_status_code(204),
                    };
                    let _ = request.respond(answer);
                });
                continue;
            }
            // A permission request is answered when the island decides, on a
            // thread of its own, so other requests are never held up.
            if let Some(Event::Claude { event: name, payload, .. }) = event.as_ref().filter(|e| matches!(e, Event::Claude { event, .. } if event == "PermissionRequest")) {
                let (tx, rx) = channel::<String>();
                on_event(Event::Claude { event: name.clone(), payload: payload.clone(), reply: Some(Reply(tx)) });
                std::thread::spawn(move || {
                    let answer = rx.recv_timeout(DECISION_WAIT).unwrap_or_default();
                    let _ = request.respond(tiny_http::Response::from_string(answer).with_status_code(200));
                });
                continue;
            }
            if let Some(event) = event {
                on_event(event);
            }
            let _ = request.respond(tiny_http::Response::from_string(response.body).with_status_code(response.status));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn post(body: &str, auth: Option<&str>, ctype: Option<&str>) -> (Response, Option<Event>) {
        handle(&Request { origin: None, method: "POST", path: "/activity", authorization: auth, content_type: ctype, body: body.as_bytes() }, TOKEN)
    }

    fn bearer() -> String {
        format!("Bearer {TOKEN}")
    }

    #[test]
    fn ytm_is_for_extensions_only() {
        let body = br#"{"shuffle":true,"repeat":"all","title":"x"}"#;
        let page = Request { origin: Some("https://evil.example"), method: "POST", path: "/ytm/state", authorization: None, content_type: Some("application/json"), body };
        assert_eq!(handle(&page, TOKEN).0.status, 403, "a web page can't");
        let ext = Request { origin: Some("chrome-extension://abc"), method: "POST", path: "/ytm/state", authorization: None, content_type: Some("application/json"), body };
        let (r, e) = handle(&ext, TOKEN);
        assert_eq!(r.status, 200);
        assert_eq!(e, Some(Event::Ytm(YtmState { shuffle: true, repeat: "all".into(), title: "x".into() })));
    }

    #[test]
    fn ytm_commands_wait_in_line() {
        ytm_command("shuffle");
        assert_eq!(ytm_next(Duration::from_millis(10)).as_deref(), Some("shuffle"));
        assert_eq!(ytm_next(Duration::from_millis(10)), None, "nothing more: an empty answer after the wait");
    }

    #[test]
    fn health_needs_no_token() {
        let (r, e) = handle(&Request { origin: None, method: "GET", path: "/health", authorization: None, content_type: None, body: b"" }, TOKEN);
        assert_eq!((r.status, e), (200, None));
    }

    #[test]
    fn a_good_post_becomes_an_upsert() {
        let (r, e) = post(r#"{"id":"b","kind":"alert","title":"Build done"}"#, Some(&bearer()), Some("application/json; charset=utf-8"));
        assert_eq!(r.status, 200);
        assert!(matches!(e, Some(Event::Upsert(a)) if a.id == "b" && a.title == "Build done"));
    }

    #[test]
    fn no_token_wrong_token_wrong_type_are_refused() {
        let body = r#"{"id":"b","kind":"alert","title":"t"}"#;
        assert_eq!(post(body, None, Some("application/json")).0.status, 401);
        assert_eq!(post(body, Some("Bearer nope"), Some("application/json")).0.status, 401);
        assert_eq!(post(body, Some(&bearer()), Some("text/plain")).0.status, 415, "what a browser could send without preflight");
        assert_eq!(handle(&Request { origin: None, method: "POST", path: "/activity", authorization: Some(&bearer()), content_type: Some("application/json"), body: b"" }, "").0.status, 401, "no token configured: nothing gets in");
    }

    #[test]
    fn bad_json_says_why() {
        let (r, e) = post(r#"{"id":"b","kind":"alert"}"#, Some(&bearer()), Some("application/json"));
        assert_eq!((r.status, e.is_none()), (400, true));
        assert!(r.body.contains("title"), "{}", r.body);
    }

    #[test]
    fn too_big_is_refused() {
        let big = format!(r#"{{"id":"b","kind":"alert","title":"{}"}}"#, "x".repeat(BODY_LIMIT));
        assert_eq!(post(&big, Some(&bearer()), Some("application/json")).0.status, 413);
    }

    #[test]
    fn a_claude_hook_becomes_a_claude_event() {
        let body = br#"{"hook_event_name":"Stop","session_id":"s1"}"#;
        let (r, e) = handle(&Request { origin: None, method: "POST", path: "/claude/Stop", authorization: Some(&bearer()), content_type: Some("application/json"), body }, TOKEN);
        assert_eq!(r.status, 200);
        assert!(matches!(e, Some(Event::Claude { event, payload, reply: None }) if event == "Stop" && payload["session_id"] == "s1"));
        let (r, _) = handle(&Request { origin: None, method: "POST", path: "/claude/../x", authorization: Some(&bearer()), content_type: Some("application/json"), body }, TOKEN);
        assert_eq!(r.status, 400);
    }

    #[test]
    fn delete_ends_by_id_decoded() {
        let (r, e) = handle(&Request { origin: None, method: "DELETE", path: "/activity/claude%2Fs1", authorization: Some(&bearer()), content_type: None, body: b"" }, TOKEN);
        assert_eq!((r.status, e), (200, Some(Event::End("claude/s1".into()))));
    }

    #[test]
    fn the_token_is_made_once_and_kept() {
        let dir = std::env::temp_dir().join(format!("island-webhook-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = load_or_create_token(&dir).unwrap();
        assert_eq!(first.len(), 64);
        assert_eq!(load_or_create_token(&dir).unwrap(), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_permission_request_waits_for_the_islands_answer() {
        let port = 48000 + (std::process::id() % 90) as u16;
        spawn(port, TOKEN.into(), move |e| {
            if let Event::Claude { reply: Some(reply), .. } = e {
                // The user clicks Allow a moment later.
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    let _ = reply.0.send("allow".into());
                });
            }
        })
        .unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"hook_event_name":"PermissionRequest","tool_name":"Bash"}"#;
        use std::io::Write;
        write!(stream, "POST /claude/PermissionRequest HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(answer.ends_with("allow"), "{answer}");
    }

    #[test]
    fn served_for_real_on_localhost() {
        let (tx, rx) = std::sync::mpsc::channel();
        let port = 47900 + (std::process::id() % 90) as u16;
        spawn(port, TOKEN.into(), move |e| tx.send(e).unwrap()).unwrap();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let body = r#"{"id":"live","kind":"status","title":"Hello"}"#;
        use std::io::Write;
        write!(stream, "POST /activity HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {TOKEN}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        assert!(matches!(rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap(), Event::Upsert(a) if a.id == "live"));
    }
}
