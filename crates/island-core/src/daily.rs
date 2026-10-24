// The daily modules' logic, pure (the caller passes the time): Focus
// (Pomodoro), Water and To-do. The app draws them and keeps their data in
// data.json; nothing here knows about the UI or the disk.

use serde::{Deserialize, Serialize};

// ── Focus ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Focus,
    ShortBreak,
    LongBreak,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Phase::Focus => "Focus",
            Phase::ShortBreak => "Short break",
            Phase::LongBreak => "Long break",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FocusPlan {
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// Focus rounds before a long break.
    pub rounds: u32,
}

impl Default for FocusPlan {
    fn default() -> Self {
        FocusPlan { focus_min: 25, short_min: 5, long_min: 15, rounds: 4 }
    }
}

impl FocusPlan {
    pub fn length_ms(&self, p: Phase) -> u64 {
        let min = match p {
            Phase::Focus => self.focus_min,
            Phase::ShortBreak => self.short_min,
            Phase::LongBreak => self.long_min,
        };
        min.max(1) as u64 * 60_000
    }
}

/// The Pomodoro timer. Times are ms on the caller's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Focus {
    pub phase: Phase,
    /// Focus rounds finished since the last long break.
    pub done_rounds: u32,
    /// Running: when the phase ends. Paused: None, and `left_ms` holds the rest.
    pub ends_at: Option<u64>,
    pub left_ms: u64,
}

impl Focus {
    pub fn new(plan: &FocusPlan) -> Self {
        Focus { phase: Phase::Focus, done_rounds: 0, ends_at: None, left_ms: plan.length_ms(Phase::Focus) }
    }

    pub fn running(&self) -> bool {
        self.ends_at.is_some()
    }

    pub fn left(&self, now: u64) -> u64 {
        match self.ends_at {
            Some(end) => end.saturating_sub(now),
            None => self.left_ms,
        }
    }

    pub fn start(&mut self, now: u64) {
        if self.ends_at.is_none() {
            self.ends_at = Some(now + self.left_ms);
        }
    }

    pub fn pause(&mut self, now: u64) {
        if let Some(end) = self.ends_at.take() {
            self.left_ms = end.saturating_sub(now);
        }
    }

    pub fn reset(&mut self, plan: &FocusPlan) {
        *self = Focus::new(plan);
    }

    /// The next phase, paused at its full length.
    pub fn skip(&mut self, plan: &FocusPlan) {
        let next = match self.phase {
            Phase::Focus => {
                self.done_rounds += 1;
                if self.done_rounds >= plan.rounds.max(1) {
                    self.done_rounds = 0;
                    Phase::LongBreak
                } else {
                    Phase::ShortBreak
                }
            }
            Phase::ShortBreak | Phase::LongBreak => Phase::Focus,
        };
        self.phase = next;
        self.ends_at = None;
        self.left_ms = plan.length_ms(next);
    }

    /// Time passed: when the running phase is over, the next one is ready
    /// (paused). Returns the phase that just ended.
    pub fn tick(&mut self, now: u64, plan: &FocusPlan) -> Option<Phase> {
        let end = self.ends_at?;
        if now < end {
            return None;
        }
        let ended = self.phase;
        self.skip(plan);
        Some(ended)
    }
}

/// One focus round in the log: when, and how long the user actually focused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FocusRound {
    /// "2026-10-24".
    pub date: String,
    /// Minutes after midnight.
    pub from_min: u32,
    pub to_min: u32,
    pub focused_s: u32,
    /// Ran its full length (not skipped or reset).
    pub full: bool,
}

/// Rounds kept: about a month of heavy use.
pub const FOCUS_LOG_MAX: usize = 400;

/// Adds a round (shorter than a minute isn't one) and drops the oldest past the cap.
pub fn log_round(log: &mut Vec<FocusRound>, round: FocusRound) {
    if round.focused_s < 60 {
        return;
    }
    log.push(round);
    if log.len() > FOCUS_LOG_MAX {
        let extra = log.len() - FOCUS_LOG_MAX;
        log.drain(..extra);
    }
}

/// A day's rounds and seconds focused.
pub fn day_total(log: &[FocusRound], date: &str) -> (usize, u32) {
    log.iter().filter(|r| r.date == date).fold((0, 0), |(n, s), r| (n + 1, s + r.focused_s))
}

/// "1 h 05 m" / "25 m".
pub fn h_m(secs: u32) -> String {
    let m = secs / 60;
    if m >= 60 { format!("{} h {:02} m", m / 60, m % 60) } else { format!("{m} m") }
}

/// "24:59" for ms.
pub fn mm_ss(ms: u64) -> String {
    let s = ms.div_ceil(1000);
    format!("{:02}:{:02}", s / 60, s % 60)
}

// ── Water ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WaterDay {
    /// "2026-10-23".
    pub date: String,
    /// Each drink: ml.
    pub drinks: Vec<u32>,
    /// Minutes after midnight of the last drink.
    pub last_drink_min: Option<u32>,
    /// When each drink was, minutes after midnight (beside `drinks`; empty
    /// in files from before it was kept).
    #[serde(default)]
    pub at: Vec<u32>,
}

impl WaterDay {
    /// Today's record: a new day starts empty.
    pub fn for_date(&mut self, date: &str) {
        if self.date != date {
            *self = WaterDay { date: date.into(), ..Default::default() };
        }
    }

    pub fn total(&self) -> u32 {
        self.drinks.iter().sum()
    }

    pub fn drink(&mut self, ml: u32, minute: u32) {
        if self.at.len() != self.drinks.len() {
            self.at.clear();
        }
        self.drinks.push(ml);
        self.at.push(minute);
        self.last_drink_min = Some(minute);
    }

    pub fn undo(&mut self) {
        self.drinks.pop();
        self.at.pop();
        self.last_drink_min = self.at.last().copied();
    }

    /// Ml drunk in each hour from `from_h` to `until_h` (inclusive), widened
    /// to any hour with a drink, as v1's chart: (hour, ml).
    pub fn per_hour(&self, from_h: u32, until_h: u32) -> Vec<(u32, u32)> {
        let hours: Vec<u32> = if self.at.len() == self.drinks.len() { self.at.iter().map(|m| m / 60).collect() } else { Vec::new() };
        let lo = hours.iter().copied().min().map_or(from_h, |h| h.min(from_h));
        let hi = hours.iter().copied().max().map_or(until_h, |h| h.max(until_h)).min(23);
        (lo..=hi).map(|h| (h, hours.iter().zip(&self.drinks).filter(|(x, _)| **x == h).map(|(_, ml)| *ml).sum())).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterPlan {
    pub goal_ml: u32,
    pub glass_ml: u32,
    /// Minutes without a drink before a reminder.
    pub every_min: u32,
    /// Reminder hours, minutes after midnight.
    pub from_min: u32,
    pub until_min: u32,
}

impl Default for WaterPlan {
    fn default() -> Self {
        WaterPlan { goal_ml: 2000, glass_ml: 250, every_min: 60, from_min: 9 * 60, until_min: 21 * 60 }
    }
}

/// The minute (after midnight) the next reminder is due today, if any:
/// `every_min` after the last drink (or after `from`), inside the hours, and
/// only while the goal isn't reached.
pub fn next_reminder(plan: &WaterPlan, day: &WaterDay, now_min: u32, last_reminded_min: Option<u32>) -> Option<u32> {
    // Past the reminder hours, or the goal reached: nothing more today.
    if day.total() >= plan.goal_ml || plan.every_min == 0 || now_min > plan.until_min {
        return None;
    }
    let base = [Some(plan.from_min), day.last_drink_min, last_reminded_min].into_iter().flatten().max()?;
    let due = (base + plan.every_min).max(plan.from_min);
    let due = due.max(now_min.min(plan.until_min));
    (due <= plan.until_min).then_some(due)
}

// ── To-do ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Todo {
    pub tasks: Vec<Task>,
    pub next_id: u64,
}

pub const TASK_MAX: usize = 200;

impl Todo {
    pub fn add(&mut self, text: &str) -> Option<u64> {
        let text = crate::activity::cut(text, TASK_MAX);
        if text.is_empty() {
            return None;
        }
        self.next_id += 1;
        self.tasks.push(Task { id: self.next_id, text, done: false });
        Some(self.next_id)
    }

    /// Done goes to the very bottom (the latest done last); open again, it
    /// comes back under the open ones.
    pub fn toggle(&mut self, id: u64) {
        let Some(i) = self.tasks.iter().position(|t| t.id == id) else { return };
        let mut t = self.tasks.remove(i);
        t.done = !t.done;
        if t.done {
            self.tasks.push(t);
        } else {
            let at = self.open_count();
            self.tasks.insert(at, t);
        }
    }

    /// Drags an open task to `to`, a place among the open tasks (0 = top).
    pub fn move_open(&mut self, id: u64, to: usize) {
        let Some(i) = self.tasks.iter().position(|t| t.id == id && !t.done) else { return };
        let t = self.tasks.remove(i);
        let to = to.min(self.open_count());
        self.tasks.insert(to, t);
    }

    pub fn clear_done(&mut self) {
        self.tasks.retain(|t| !t.done);
    }

    pub fn remove(&mut self, id: u64) {
        self.tasks.retain(|t| t.id != id);
    }

    /// Open first (in their order), then done. The list is kept this way;
    /// this also sorts lists saved before it was.
    pub fn ordered(&self) -> Vec<&Task> {
        let mut v: Vec<&Task> = self.tasks.iter().filter(|t| !t.done).collect();
        v.extend(self.tasks.iter().filter(|t| t.done));
        v
    }

    pub fn open_count(&self) -> usize {
        self.tasks.iter().filter(|t| !t.done).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_runs_pauses_and_moves_on() {
        let plan = FocusPlan { focus_min: 1, short_min: 1, long_min: 2, rounds: 2 };
        let mut f = Focus::new(&plan);
        f.start(0);
        assert_eq!(f.left(10_000), 50_000);
        f.pause(10_000);
        assert_eq!(f.left(99_000), 50_000, "paused: the time stands still");
        f.start(20_000);
        assert_eq!(f.tick(69_999, &plan), None);
        assert_eq!(f.tick(70_000, &plan), Some(Phase::Focus));
        assert_eq!((f.phase, f.running()), (Phase::ShortBreak, false), "the next phase waits to be started");
        f.skip(&plan);
        f.skip(&plan);
        assert_eq!(f.phase, Phase::LongBreak, "after `rounds` focus rounds");
        assert_eq!(mm_ss(f.left(0)), "02:00");
    }

    #[test]
    fn focus_rounds_are_logged_and_summed() {
        let mut log = Vec::new();
        let r = |date: &str, s: u32| FocusRound { date: date.into(), from_min: 600, to_min: 625, focused_s: s, full: s >= 1500 };
        log_round(&mut log, r("2026-10-24", 1500));
        log_round(&mut log, r("2026-10-24", 30));
        log_round(&mut log, r("2026-10-24", 600));
        log_round(&mut log, r("2026-10-23", 1500));
        assert_eq!(day_total(&log, "2026-10-24"), (2, 2100), "under a minute isn't a round");
        assert_eq!(h_m(2100), "35 m");
        assert_eq!(h_m(3900), "1 h 05 m");
        for _ in 0..FOCUS_LOG_MAX {
            log_round(&mut log, r("x", 60));
        }
        assert_eq!(log.len(), FOCUS_LOG_MAX);
    }

    #[test]
    fn water_counts_the_day_and_starts_over() {
        let mut d = WaterDay::default();
        d.for_date("2026-10-23");
        d.drink(250, 600);
        d.drink(250, 660);
        assert_eq!(d.total(), 500);
        d.undo();
        assert_eq!(d.total(), 250);
        d.for_date("2026-10-24");
        assert_eq!(d.total(), 0);
    }

    #[test]
    fn water_by_the_hour() {
        let mut d = WaterDay::default();
        d.drink(250, 9 * 60 + 5);
        d.drink(250, 9 * 60 + 50);
        d.drink(300, 7 * 60);
        let chart = d.per_hour(9, 12);
        assert_eq!(chart.first(), Some(&(7, 300)), "widened to an early glass");
        assert_eq!(chart.iter().find(|(h, _)| *h == 9), Some(&(9, 500)));
        assert_eq!(chart.last(), Some(&(12, 0)));
        d.undo();
        assert_eq!(d.last_drink_min, Some(9 * 60 + 50), "undo moves the last drink back");
    }

    #[test]
    fn water_reminders_follow_the_last_drink_and_the_hours() {
        let plan = WaterPlan { goal_ml: 1000, glass_ml: 250, every_min: 60, from_min: 540, until_min: 1260 };
        let mut d = WaterDay { date: "x".into(), ..Default::default() };
        assert_eq!(next_reminder(&plan, &d, 500, None), Some(600), "an hour after the hours start");
        d.drink(250, 700);
        assert_eq!(next_reminder(&plan, &d, 710, None), Some(760));
        assert_eq!(next_reminder(&plan, &d, 800, Some(760)), Some(820), "after a reminder, the next one an hour later");
        assert_eq!(next_reminder(&plan, &d, 1250, Some(1240)), None, "past the hours");
        assert_eq!(next_reminder(&plan, &d, 1400, None), None, "late at night: no reminder (it was firing at 23:20)");
        d.drink(750, 710);
        assert_eq!(next_reminder(&plan, &d, 720, None), None, "goal reached");
    }

    #[test]
    fn todo_adds_toggles_and_orders() {
        let mut t = Todo::default();
        let a = t.add("Reply to Mika").unwrap();
        let b = t.add("  ").map(|_| 0);
        assert_eq!(b, None, "blank is not a task");
        let c = t.add("Rekap dachi").unwrap();
        t.toggle(a);
        assert_eq!(t.ordered().iter().map(|x| x.id).collect::<Vec<_>>(), [c, a], "open first");
        assert_eq!(t.open_count(), 1);
        t.remove(a);
        assert_eq!(t.tasks.len(), 1);
    }

    #[test]
    fn todo_done_sinks_and_open_tasks_reorder() {
        let mut t = Todo::default();
        let ids: Vec<u64> = ["a", "b", "c", "d"].iter().map(|x| t.add(x).unwrap()).collect();
        let order = |t: &Todo| t.tasks.iter().map(|x| x.text.clone()).collect::<String>();
        t.toggle(ids[0]);
        assert_eq!(order(&t), "bcda", "done goes to the bottom");
        t.toggle(ids[2]);
        assert_eq!(order(&t), "bdac", "the latest done last");
        t.move_open(ids[3], 0);
        assert_eq!(order(&t), "dbac", "dragged to the top");
        t.move_open(ids[3], 99);
        assert_eq!(order(&t), "bdac", "never below the done ones");
        t.toggle(ids[0]);
        assert_eq!(order(&t), "bdac", "open again: under the open ones");
        t.clear_done();
        assert_eq!(order(&t), "bda");
    }
}
