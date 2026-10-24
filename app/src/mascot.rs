// The island's mascot, alive: every tick (5 a second, only while the island
// can be seen) it picks a frame — breathing, a blink, an ear flick, a tail
// wag, a few steps to and fro, a nap when left alone, a hop when poked.
// Pure: the caller ticks it and draws what it returns.

/// Milliseconds between ticks.
pub const TICK_MS: u64 = 200;
/// How far it wanders from its spot, in sprite pixels (a 32-px grid).
const ROAM: i32 = 4;
/// Ticks without a poke before a nap (about a minute and a half).
const NAP_AFTER: u32 = 450;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Doing {
    Idle,
    Wag(u8),
    Walk(i32),
    Happy(u8),
    Sleep,
}

/// What to draw this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub frame: &'static str,
    /// Offset from its spot, in sprite pixels.
    pub dx: i32,
    pub dy: i32,
    pub left: bool,
}

pub struct Life {
    doing: Doing,
    tick: u64,
    x: i32,
    left: bool,
    quiet: u32,
    seed: u64,
}

impl Default for Life {
    fn default() -> Self {
        Life { doing: Doing::Idle, tick: 0, x: 0, left: false, quiet: 0, seed: 0x2545_f491_4f6c_dd1d }
    }
}

impl Life {
    fn roll(&mut self, n: u32) -> u32 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed % n.max(1) as u64) as u32
    }

    /// Clicked (or the island came back): wakes up and hops.
    pub fn poke(&mut self) {
        self.quiet = 0;
        self.doing = Doing::Happy(8);
    }

    /// `roam`: room to walk (the expanded island); the capsule has none.
    pub fn tick(&mut self, roam: bool) -> Pose {
        self.tick += 1;
        self.quiet = self.quiet.saturating_add(1);
        if !roam {
            self.x = 0;
        }
        if self.quiet > NAP_AFTER && self.doing == Doing::Idle {
            self.doing = Doing::Sleep;
        }
        let breathe = if (self.tick / 2) % 2 == 0 { "idle" } else { "idle2" };
        let (frame, dy) = match self.doing {
            Doing::Sleep => ("sleep", 0),
            Doing::Happy(n) => {
                const HOP: [i32; 8] = [0, 0, -1, -2, -2, -1, 0, 0];
                self.doing = if n > 1 { Doing::Happy(n - 1) } else { Doing::Idle };
                ("happy", HOP[(8 - n as usize).min(7)])
            }
            Doing::Wag(n) => {
                self.doing = if n > 1 { Doing::Wag(n - 1) } else { Doing::Idle };
                (if n % 2 == 0 { "wag1" } else { "wag2" }, 0)
            }
            Doing::Walk(to) => {
                if self.x == to {
                    self.doing = Doing::Idle;
                    ("idle", 0)
                } else {
                    self.left = to < self.x;
                    self.x += if self.left { -1 } else { 1 };
                    const STEPS: [&str; 4] = ["walk1", "walk2", "walk3", "walk4"];
                    (STEPS[(self.tick % 4) as usize], 0)
                }
            }
            Doing::Idle => {
                // Now and then something small happens; mostly she just breathes.
                let r = self.roll(1000);
                if r < 60 {
                    ("blink", 0)
                } else if r < 85 {
                    ("twitch", 0)
                } else if r < 105 {
                    self.doing = Doing::Wag(6);
                    ("wag1", 0)
                } else if roam && r < 120 {
                    let to = self.roll((ROAM * 2 + 1) as u32) as i32 - ROAM;
                    self.doing = Doing::Walk(to);
                    (breathe, 0)
                } else {
                    (breathe, 0)
                }
            }
        };
        Pose { frame, dx: self.x, dy, left: self.left }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_breathes_wanders_naps_and_wakes() {
        let mut life = Life::default();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let p = life.tick(true);
            assert!(p.dx.abs() <= ROAM, "stays near its spot: {}", p.dx);
            assert!((-2..=0).contains(&p.dy));
            seen.insert(p.frame);
            if p.frame == "sleep" {
                life.poke();
            }
        }
        for f in ["idle", "idle2", "blink", "twitch", "wag1", "wag2", "walk1", "happy", "sleep"] {
            assert!(seen.contains(f), "never showed {f}");
        }
    }

    #[test]
    fn no_walking_in_the_capsule() {
        let mut life = Life::default();
        for _ in 0..2000 {
            assert_eq!(life.tick(false).dx, 0);
        }
    }
}
