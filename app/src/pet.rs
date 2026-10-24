// The mascot that lives in the Settings window: it walks along the floor,
// climbs the sidebar's edge and sits up there, naps when left alone, and
// jumps for joy when clicked or when a setting changes. Pure: the caller
// ticks it (10 a second, only while Settings is open) and draws the result.

/// Ticks a second.
pub const HZ: u64 = 10;
/// The sprite on screen, logical px (64 px drawn at 2x).
pub const SIZE: f32 = 128.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Idle(u32),
    Walk(f32),
    ToWall,
    ClimbUp,
    Perch(u32),
    ClimbDown,
    Sit(u32),
    Sleep,
    Happy(u32),
}

pub struct Pet {
    pub x: f32,
    pub y: f32,
    pub left: bool,
    pub frame: &'static str,
    pub say: String,
    mode: Mode,
    ticks: u64,
    quiet: u32,
    say_left: u32,
    seed: u64,
}

/// Where it lives: the window's height and the sidebar's edge it climbs.
#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub h: f32,
    pub wall: f32,
}

impl Room {
    fn floor(&self) -> f32 {
        self.h - SIZE - 6.0
    }
    fn top(&self) -> f32 {
        64.0
    }
    /// Its x when hugging the wall from the sidebar's side (it never covers the page).
    fn wall_x(&self) -> f32 {
        self.wall - SIZE + 6.0
    }
}

/// What it says when clicked, per theme.
pub fn greeting(theme: &str, n: u64) -> &'static str {
    let lines: &[&str] = match theme {
        "mio" => &["Mio-sha!", "Uchi da yo~", "Ganbatte ne"],
        "towa" => &["Towa-sama desu!", "Konyappi~", "Bibi ga..."],
        "azki" => &["AZKi desu~", "Utau yo!", "Ganbaru!"],
        "suisei" => &["Kyou mo kawaii!", "Suichan desu", "Hoshimachi!"],
        _ => &["Konkon~", "Kitsune ja!", "Sukon-bu!", "Neko ja nai!"],
    };
    lines[(n % lines.len() as u64) as usize]
}

impl Pet {
    pub fn new(room: Room) -> Pet {
        Pet {
            x: 20.0,
            y: room.floor(),
            left: false,
            frame: "idle",
            say: String::new(),
            mode: Mode::Idle(20),
            ticks: 0,
            quiet: 0,
            say_left: 0,
            seed: 0x9e37_79b9_7f4a_7c15,
        }
    }

    fn rand(&mut self, n: u32) -> u32 {
        // xorshift: enough for a mascot's whims.
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        (self.seed % n.max(1) as u64) as u32
    }

    /// Clicked: a happy jump and a line.
    pub fn poke(&mut self, theme: &str) {
        self.mode = Mode::Happy(14);
        self.quiet = 0;
        self.say = greeting(theme, self.ticks / 3).to_string();
        self.say_left = 22;
    }

    /// A setting changed: a short cheer, no words.
    pub fn cheer(&mut self) {
        if !matches!(self.mode, Mode::ClimbUp | Mode::ClimbDown | Mode::Perch(_)) {
            self.mode = Mode::Happy(8);
        }
        self.quiet = 0;
    }

    pub fn tick(&mut self, room: Room) {
        self.ticks += 1;
        self.quiet += 1;
        if self.say_left > 0 {
            self.say_left -= 1;
            if self.say_left == 0 {
                self.say.clear();
            }
        }
        // It lives in the sidebar: the page stays clear for the mouse.
        self.x = self.x.clamp(4.0, room.wall_x().max(4.0));
        let floor = room.floor();
        let walk_frame = if (self.ticks / 2) % 2 == 0 { "walk1" } else { "walk2" };
        let climb_frame = if (self.ticks / 2) % 2 == 0 { "climb1" } else { "climb2" };
        // Left alone a minute on the floor: a nap.
        if self.quiet > 60 * HZ as u32 && matches!(self.mode, Mode::Idle(_) | Mode::Sit(_)) {
            self.mode = Mode::Sleep;
        }
        match self.mode {
            Mode::Idle(n) => {
                self.y = floor;
                self.frame = if self.ticks % 37 < 2 { "blink" } else { "idle" };
                self.mode = if n > 0 { Mode::Idle(n - 1) } else { self.next(room) };
            }
            Mode::Walk(target) => {
                self.y = floor;
                self.frame = walk_frame;
                let step = 3.0;
                if (target - self.x).abs() <= step {
                    self.x = target;
                    self.mode = Mode::Idle(15 + self.rand(30));
                } else {
                    self.left = target < self.x;
                    self.x += if self.left { -step } else { step };
                }
            }
            Mode::ToWall => {
                self.y = floor;
                self.frame = walk_frame;
                let target = room.wall_x();
                if (target - self.x).abs() <= 3.0 {
                    self.x = target;
                    self.mode = Mode::ClimbUp;
                } else {
                    self.left = target < self.x;
                    self.x += if self.left { -3.0 } else { 3.0 };
                }
            }
            Mode::ClimbUp => {
                self.x = room.wall_x();
                self.frame = climb_frame;
                self.y -= 2.5;
                if self.y <= room.top() {
                    self.y = room.top();
                    self.mode = Mode::Perch(40 + self.rand(50));
                }
            }
            Mode::Perch(n) => {
                self.x = room.wall_x();
                self.y = room.top();
                self.frame = if self.ticks % 41 < 2 { "blink" } else { "sit" };
                self.mode = if n > 0 { Mode::Perch(n - 1) } else { Mode::ClimbDown };
            }
            Mode::ClimbDown => {
                self.x = room.wall_x();
                self.frame = climb_frame;
                self.y += 3.0;
                if self.y >= floor {
                    self.y = floor;
                    self.mode = Mode::Idle(10);
                }
            }
            Mode::Sit(n) => {
                self.y = floor;
                self.frame = "sit";
                self.mode = if n > 0 { Mode::Sit(n - 1) } else { Mode::Idle(5) };
            }
            Mode::Sleep => {
                self.frame = "sleep";
            }
            Mode::Happy(n) => {
                self.frame = "happy";
                // A hop: up and back down over the first ticks.
                let base = if (self.x - room.wall_x()).abs() < 1.0 && self.y < floor - 1.0 { self.y } else { floor };
                let hop = [0.0, -8.0, -14.0, -16.0, -14.0, -8.0, 0.0];
                let i = 14u32.saturating_sub(n) as usize;
                self.y = if base == floor { floor + hop.get(i).copied().unwrap_or(0.0) } else { base };
                self.mode = if n > 0 { Mode::Happy(n - 1) } else { Mode::Idle(10) };
            }
        }
    }

    /// What to do after standing a while.
    fn next(&mut self, room: Room) -> Mode {
        match self.rand(10) {
            0..=4 => {
                let (lo, hi) = (6.0, (room.wall_x() - 6.0).max(7.0));
                Mode::Walk(lo + self.rand((hi - lo) as u32) as f32)
            }
            5..=7 => Mode::ToWall,
            _ => Mode::Sit(30 + self.rand(40)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_stays_in_the_room_and_climbs_the_wall() {
        let room = Room { h: 600.0, wall: 232.0 };
        let mut p = Pet::new(room);
        let (mut climbed, mut walked) = (false, false);
        for _ in 0..20_000 {
            p.tick(room);
            assert!(p.x >= 0.0 && p.x + SIZE <= room.wall + 6.0, "in the sidebar: x {}", p.x);
            assert!(p.y >= room.top() - 20.0 && p.y <= room.floor() + 0.1, "y {}", p.y);
            climbed |= p.frame.starts_with("climb");
            walked |= p.frame.starts_with("walk");
            if p.frame == "sleep" {
                p.poke("fubuki");
            }
        }
        assert!(climbed && walked);
    }

    #[test]
    fn a_poke_wakes_it_with_a_line() {
        let room = Room { h: 600.0, wall: 232.0 };
        let mut p = Pet::new(room);
        p.poke("mio");
        p.tick(room);
        assert_eq!(p.frame, "happy");
        assert!(p.say.starts_with("Mio") || p.say.contains("da yo") || p.say.contains("Ganbatte"));
        for _ in 0..30 {
            p.tick(room);
        }
        assert!(p.say.is_empty(), "the bubble goes away");
    }
}
