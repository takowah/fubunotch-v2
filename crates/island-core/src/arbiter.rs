// The priority arbiter (docs/SPEC-draft.md): which live activity has the
// island. Pure: the caller passes the time (ms, any monotonic clock), and asks
// `next_wake` when to come back, so the app needs one timer and no polling.

use crate::activity::{Activity, Kind};

/// A front activity keeps the island this long against a newcomer of the
/// same or a lower priority.
pub const MIN_SHOW_MS: u64 = 2_500;

#[derive(Default)]
pub struct Arbiter {
    live: Vec<Activity>,
    /// The activity in front and since when.
    front: Option<(String, u64)>,
}

/// What the island shows.
#[derive(Debug, PartialEq)]
pub struct View<'a> {
    pub front: Option<&'a Activity>,
    /// Live activities other than the front one.
    pub waiting: usize,
}

/// Best first: priority, then kind, then the most recent update.
fn better(a: &Activity, b: &Activity) -> bool {
    (a.priority, a.kind.rank(), a.updated_at) > (b.priority, b.kind.rank(), b.updated_at)
}

impl Arbiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// A new or updated activity (same id = the same activity).
    pub fn upsert(&mut self, activity: Activity, now: u64) {
        match self.live.iter_mut().find(|a| a.id == activity.id) {
            Some(slot) => *slot = activity,
            None => self.live.push(activity),
        }
        self.settle(now);
    }

    /// The sender ended it. True when it was live.
    pub fn end(&mut self, id: &str, now: u64) -> bool {
        let before = self.live.len();
        self.live.retain(|a| a.id != id);
        let gone = self.live.len() != before;
        self.settle(now);
        gone
    }

    /// Time passed: expiries, and a held front that may now give way.
    pub fn tick(&mut self, now: u64) {
        self.settle(now);
    }

    pub fn view(&self) -> View<'_> {
        let front = self.front.as_ref().and_then(|(id, _)| self.live.iter().find(|a| &a.id == id));
        View { front, waiting: self.live.len().saturating_sub(front.is_some() as usize) }
    }

    /// Every live activity, best first (the expanded island's list).
    pub fn ranked(&self) -> Vec<&Activity> {
        let mut all: Vec<&Activity> = self.live.iter().collect();
        all.sort_by(|a, b| {
            if better(a, b) {
                std::cmp::Ordering::Less
            } else if better(b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        all
    }

    /// When to call `tick` next: the next expiry, or the end of a hold that
    /// keeps a better activity waiting. None: nothing will change by itself.
    pub fn next_wake(&self, now: u64) -> Option<u64> {
        let expiry = self.live.iter().filter_map(Activity::expires_at).min();
        let hold_end = self.front.as_ref().and_then(|(id, since)| {
            let current = self.live.iter().find(|a| &a.id == id)?;
            let best = self.best()?;
            (best.id != current.id).then_some(since + MIN_SHOW_MS)
        });
        [expiry, hold_end].into_iter().flatten().filter(|t| *t > now).min().or_else(|| {
            // Something is already due: come back at once.
            [expiry, hold_end].into_iter().flatten().min().map(|_| now)
        })
    }

    fn best(&self) -> Option<&Activity> {
        self.live.iter().fold(None, |best: Option<&Activity>, a| match best {
            Some(b) if !better(a, b) => Some(b),
            _ => Some(a),
        })
    }

    fn settle(&mut self, now: u64) {
        self.live.retain(|a| a.expires_at().is_none_or(|t| t > now));
        let Some(best) = self.best() else {
            self.front = None;
            return;
        };
        let best_id = best.id.clone();
        let best_priority = best.priority;
        let current = self.front.as_ref().and_then(|(id, since)| {
            self.live.iter().find(|a| &a.id == id).map(|a| (a, *since))
        });
        let next = match current {
            None => Some((best_id, now)),
            Some((cur, _)) if cur.id == best_id => None,
            // Ambient yields at once; a higher priority takes over at once.
            Some((cur, _)) if cur.kind == Kind::Ambient || best_priority > cur.priority => Some((best_id, now)),
            // Same or lower priority: the front keeps the island for MIN_SHOW_MS.
            Some((_, since)) if now.saturating_sub(since) >= MIN_SHOW_MS => Some((best_id, now)),
            Some(_) => None,
        };
        if let Some(front) = next {
            self.front = Some(front);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::Incoming;

    fn act(id: &str, kind: &str, extra: &str, now: u64) -> Activity {
        let json = format!(r#"{{"id":"{id}","kind":"{kind}","title":"{id}"{extra}}}"#);
        Incoming::parse(json.as_bytes()).unwrap().into_activity(now).unwrap()
    }

    fn front(arb: &Arbiter) -> Option<&str> {
        arb.view().front.map(|a| a.id.as_str())
    }

    #[test]
    fn nothing_live_shows_nothing() {
        let arb = Arbiter::new();
        assert_eq!(arb.view(), View { front: None, waiting: 0 });
        assert_eq!(arb.next_wake(0), None);
    }

    #[test]
    fn higher_priority_takes_over_at_once() {
        let mut arb = Arbiter::new();
        arb.upsert(act("song", "ambient", "", 0), 0);
        arb.upsert(act("msg", "alert", "", 100), 100);
        assert_eq!(front(&arb), Some("msg"));
        arb.upsert(act("approve", "ask", "", 200), 200);
        assert_eq!(front(&arb), Some("approve"), "high beats normal without waiting");
        assert_eq!(arb.view().waiting, 2);
    }

    #[test]
    fn same_priority_waits_for_min_show() {
        let mut arb = Arbiter::new();
        arb.upsert(act("a", "alert", "", 0), 0);
        arb.upsert(act("b", "alert", "", 1_000), 1_000);
        assert_eq!(front(&arb), Some("a"), "no flicker");
        assert_eq!(arb.next_wake(1_000), Some(MIN_SHOW_MS), "come back when the hold ends");
        arb.tick(MIN_SHOW_MS);
        assert_eq!(front(&arb), Some("b"), "the newer one after the hold");
    }

    #[test]
    fn ambient_never_holds_the_island() {
        let mut arb = Arbiter::new();
        arb.upsert(act("song", "ambient", "", 0), 0);
        arb.upsert(act("build", "progress", "", 10), 10);
        assert_eq!(front(&arb), Some("build"), "low progress beats ambient at once");
    }

    #[test]
    fn expiry_hands_the_island_back() {
        let mut arb = Arbiter::new();
        arb.upsert(act("song", "ambient", "", 0), 0);
        arb.upsert(act("msg", "alert", r#","ttl_ms":3000"#, 0), 0);
        assert_eq!(arb.next_wake(0), Some(3_000));
        arb.tick(3_000);
        assert_eq!(front(&arb), Some("song"));
        assert_eq!(arb.next_wake(3_000), None);
    }

    #[test]
    fn ending_and_updating_by_id() {
        let mut arb = Arbiter::new();
        arb.upsert(act("approve", "ask", "", 0), 0);
        arb.upsert(act("approve", "ask", r#","body":"now with text""#, 50), 50);
        assert_eq!(arb.view().waiting, 0, "same id = same activity");
        assert_eq!(arb.view().front.unwrap().body, "now with text");
        assert!(arb.end("approve", 60));
        assert!(!arb.end("approve", 60));
        assert_eq!(front(&arb), None);
    }

    #[test]
    fn ranked_lists_best_first() {
        let mut arb = Arbiter::new();
        arb.upsert(act("song", "ambient", "", 0), 0);
        arb.upsert(act("approve", "ask", "", 1), 1);
        arb.upsert(act("msg", "alert", "", 2), 2);
        let ids: Vec<&str> = arb.ranked().iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["approve", "msg", "song"]);
    }
}
