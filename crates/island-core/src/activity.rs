// The activity contract (docs/SPEC-draft.md): everything the island can show,
// whoever sends it. A sender's JSON becomes an `Activity` here, with the kind's
// defaults filled in and over-long fields cut rather than refused.

use serde::{Deserialize, Serialize};

pub const ID_MAX: usize = 128;
pub const SOURCE_MAX: usize = 64;
pub const TITLE_MAX: usize = 120;
pub const BODY_MAX: usize = 500;
pub const LABEL_MAX: usize = 24;
pub const ACTIONS_MAX: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Alert,
    Ask,
    Progress,
    Status,
    Ambient,
}

impl Kind {
    pub fn default_priority(self) -> Priority {
        match self {
            Kind::Ask => Priority::High,
            Kind::Alert | Kind::Status => Priority::Normal,
            Kind::Progress | Kind::Ambient => Priority::Low,
        }
    }

    /// None: it stays until the sender ends it.
    pub fn default_ttl_ms(self) -> Option<u64> {
        match self {
            Kind::Alert => Some(6_000),
            Kind::Status => Some(3_000),
            Kind::Ask | Kind::Progress | Kind::Ambient => None,
        }
    }

    /// Among equal priorities, who goes first (higher first).
    pub fn rank(self) -> u8 {
        match self {
            Kind::Ask => 4,
            Kind::Alert => 3,
            Kind::Status => 2,
            Kind::Progress => 1,
            Kind::Ambient => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Action {
    pub id: String,
    pub label: String,
}

/// An activity as the island keeps it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Activity {
    pub id: String,
    pub source: String,
    pub kind: Kind,
    pub priority: Priority,
    pub title: String,
    pub body: String,
    pub progress: Option<f32>,
    pub ttl_ms: Option<u64>,
    pub actions: Vec<Action>,
    /// When it was last sent (ms, the arbiter's clock).
    pub updated_at: u64,
}

impl Activity {
    /// When it runs out, if it does.
    pub fn expires_at(&self) -> Option<u64> {
        self.ttl_ms.map(|ttl| self.updated_at.saturating_add(ttl))
    }
}

/// What a sender posts. Everything but `id`, `kind` and `title` is optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Incoming {
    pub id: String,
    #[serde(default)]
    pub source: Option<String>,
    pub kind: Kind,
    #[serde(default)]
    pub priority: Option<Priority>,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub progress: Option<f32>,
    /// Present and null: stays until ended. Absent: the kind's default.
    #[serde(default, deserialize_with = "explicit_option")]
    pub ttl_ms: Option<Option<u64>>,
    #[serde(default)]
    pub actions: Vec<Action>,
}

/// Tells "ttl_ms": null (Some(None)) apart from no ttl_ms at all (None).
fn explicit_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<u64>>, D::Error> {
    Option::<u64>::deserialize(d).map(Some)
}

#[derive(Debug, PartialEq)]
pub enum Invalid {
    Json(String),
    EmptyId,
    EmptyTitle,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Invalid::Json(e) => write!(f, "not an activity: {e}"),
            Invalid::EmptyId => write!(f, "id is empty"),
            Invalid::EmptyTitle => write!(f, "title is empty"),
        }
    }
}

/// `text` on one line of at most `max` characters, cut with "…".
pub fn cut(text: &str, max: usize) -> String {
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Like `cut`, but line breaks are kept (a body has up to three lines).
fn cut_lines(text: &str, max: usize) -> String {
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect();
    let joined = lines.join("\n");
    if joined.chars().count() <= max {
        return joined;
    }
    let mut out: String = joined.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

impl Incoming {
    pub fn parse(json: &[u8]) -> Result<Incoming, Invalid> {
        serde_json::from_slice(json).map_err(|e| Invalid::Json(e.to_string()))
    }

    /// The activity as kept, sent at `now`: defaults filled in, limits applied.
    pub fn into_activity(self, now: u64) -> Result<Activity, Invalid> {
        let id = cut(&self.id, ID_MAX);
        if id.is_empty() {
            return Err(Invalid::EmptyId);
        }
        let title = cut(&self.title, TITLE_MAX);
        if title.is_empty() {
            return Err(Invalid::EmptyTitle);
        }
        let source = cut(self.source.as_deref().unwrap_or("webhook"), SOURCE_MAX);
        let actions = self
            .actions
            .into_iter()
            .map(|a| Action { id: cut(&a.id, ID_MAX), label: cut(&a.label, LABEL_MAX) })
            .filter(|a| !a.id.is_empty() && !a.label.is_empty())
            .take(ACTIONS_MAX)
            .collect();
        Ok(Activity {
            id,
            source: if source.is_empty() { "webhook".into() } else { source },
            kind: self.kind,
            priority: self.priority.unwrap_or(self.kind.default_priority()),
            title,
            body: self.body.as_deref().map(|b| cut_lines(b, BODY_MAX)).unwrap_or_default(),
            progress: self.progress.filter(|p| p.is_finite()).map(|p| p.clamp(0.0, 1.0)),
            ttl_ms: self.ttl_ms.unwrap_or(self.kind.default_ttl_ms()),
            actions,
            updated_at: now,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str, now: u64) -> Result<Activity, Invalid> {
        Incoming::parse(json.as_bytes())?.into_activity(now)
    }

    #[test]
    fn defaults_come_from_the_kind() {
        let a = parse(r#"{"id":"x","kind":"alert","title":"Hi"}"#, 100).unwrap();
        assert_eq!((a.priority, a.ttl_ms, a.source.as_str(), a.expires_at()), (Priority::Normal, Some(6_000), "webhook", Some(6_100)));
        let ask = parse(r#"{"id":"x","kind":"ask","title":"Allow?"}"#, 0).unwrap();
        assert_eq!((ask.priority, ask.ttl_ms), (Priority::High, None));
    }

    #[test]
    fn a_sender_may_override_priority_and_ttl() {
        let a = parse(r#"{"id":"x","kind":"alert","title":"Hi","priority":"critical","ttl_ms":null}"#, 0).unwrap();
        assert_eq!((a.priority, a.ttl_ms), (Priority::Critical, None), "null ttl = stays");
        let b = parse(r#"{"id":"x","kind":"ambient","title":"Hi","ttl_ms":1000}"#, 0).unwrap();
        assert_eq!(b.ttl_ms, Some(1000));
    }

    #[test]
    fn over_long_fields_are_cut_not_refused() {
        let long = "a".repeat(400);
        let a = parse(&format!(r#"{{"id":"x","kind":"alert","title":"{long}","body":"{long}{long}"}}"#), 0).unwrap();
        assert_eq!(a.title.chars().count(), TITLE_MAX);
        assert!(a.title.ends_with('…'));
        assert_eq!(a.body.chars().count(), BODY_MAX);
    }

    #[test]
    fn progress_is_clamped_and_actions_are_capped() {
        let a = parse(
            r#"{"id":"x","kind":"progress","title":"t","progress":3.5,"actions":[{"id":"a","label":"A"},{"id":"b","label":"B"},{"id":"c","label":"C"},{"id":"d","label":"D"},{"id":"","label":"E"}]}"#,
            0,
        )
        .unwrap();
        assert_eq!(a.progress, Some(1.0));
        assert_eq!(a.actions.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
    }

    #[test]
    fn bad_input_is_refused_with_a_reason() {
        assert_eq!(parse(r#"{"id":"  ","kind":"alert","title":"t"}"#, 0), Err(Invalid::EmptyId));
        assert_eq!(parse(r#"{"id":"x","kind":"alert","title":" "}"#, 0), Err(Invalid::EmptyTitle));
        assert!(matches!(parse(r#"{"id":"x","kind":"shout","title":"t"}"#, 0), Err(Invalid::Json(_))));
        assert!(matches!(parse(r#"{"id":"x","kind":"alert","title":"t","colour":"red"}"#, 0), Err(Invalid::Json(_))), "unknown fields are refused");
    }

    #[test]
    fn a_body_keeps_its_lines() {
        let a = parse(r#"{"id":"x","kind":"alert","title":"t","body":"one\n\n  two   words \nthree"}"#, 0).unwrap();
        assert_eq!(a.body, "one\ntwo words\nthree");
    }
}
