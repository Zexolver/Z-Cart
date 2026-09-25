//! Authoritative game simulation. Runs on the hosting peer; other peers only
//! send inputs and render the snapshots they get back.

use crate::math::*;
use crate::track::*;

pub const DT: f32 = 1.0 / 60.0;
pub const MAX_KARTS: usize = 8;
pub const MAX_ENTS: usize = 40;
pub const NAME_LEN: usize = 12;
const COUNTDOWN: f32 = 4.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Lobby = 0,
    Countdown = 1,
    Racing = 2,
    Results = 3,
}

impl Phase {
    pub fn from_u8(v: u8) -> Phase {
        match v {
            1 => Phase::Countdown,
            2 => Phase::Racing,
            3 => Phase::Results,
            _ => Phase::Lobby,
        }
    }
}

/// Non-Mario-flavoured items with familiar behaviour.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Peel = 1,        // banana: drop behind, spins whoever hits it
    TriplePeel,      // 3x banana
    Bouncer,         // green shell: straight, bounces off walls
    TripleBouncer,
    Seeker,          // red shell: follows track, homes in on kart ahead
    Nova,            // blue shell: hunts the race leader, area blast
    Turbo,           // speed mushroom
    TripleTurbo,
    Giant,           // mega mushroom: huge, crushes everything
    Star,            // invincible + fast, knocks karts over
    Bomb,            // thrown, explodes after fuse or on contact
    Decoy,           // fake item box
    Zap,             // lightning: spins + shrinks everyone else
    Rocket,          // bullet: auto-drives at top speed, invincible
    Ink,             // squid: blinds everyone ahead
    Nitro,           // golden mushroom: many boosts
}

impl Item {
    pub const ALL: [Item; 16] = [
        Item::Peel, Item::TriplePeel, Item::Bouncer, Item::TripleBouncer, Item::Seeker, Item::Nova,
        Item::Turbo, Item::TripleTurbo, Item::Giant, Item::Star, Item::Bomb, Item::Decoy, Item::Zap,
        Item::Rocket, Item::Ink, Item::Nitro,
    ];
    pub fn from_u8(v: u8) -> Option<Item> {
        if v >= 1 && v <= 16 { Some(Item::ALL[v as usize - 1]) } else { None }
    }
    pub fn uses(self) -> u8 {
        match self {
            Item::TriplePeel | Item::TripleBouncer | Item::TripleTurbo => 3,
            Item::Nitro => 5,
            _ => 1,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Item::Peel => "Peel",
            Item::TriplePeel => "Peel x3",
            Item::Bouncer => "Bouncer",
            Item::TripleBouncer => "Bouncer x3",
            Item::Seeker => "Seeker",
            Item::Nova => "Nova",
            Item::Turbo => "Turbo",
            Item::TripleTurbo => "Turbo x3",
            Item::Giant => "Giant",
            Item::Star => "Star",
            Item::Bomb => "Bomb",
            Item::Decoy => "Decoy",
            Item::Zap => "Zap",
            Item::Rocket => "Rocket",
            Item::Ink => "Ink",
            Item::Nitro => "Nitro",
        }
    }
    /// (front-runner weight, last-place weight)
    fn weights(self) -> (f32, f32) {
        match self {
            Item::Peel => (20.0, 3.0),
            Item::TriplePeel => (8.0, 3.0),
            Item::Bouncer => (20.0, 4.0),
            Item::TripleBouncer => (8.0, 5.0),
            Item::Seeker => (2.0, 14.0),
            Item::Nova => (0.0, 7.0),
            Item::Turbo => (15.0, 6.0),
            Item::TripleTurbo => (6.0, 8.0),
            Item::Giant => (1.0, 8.0),
            Item::Star => (1.0, 12.0),
            Item::Bomb => (12.0, 3.0),
            Item::Decoy => (12.0, 2.0),
            Item::Zap => (0.0, 6.0),
            Item::Rocket => (0.0, 7.0),
            Item::Ink => (0.0, 5.0),
            Item::Nitro => (0.0, 7.0),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntKind {
    Peel = 1,
    Bouncer,
    Seeker,
    Nova,
    Bomb,
    Decoy,
    Blast,
}

impl EntKind {
    pub fn from_u8(v: u8) -> Option<EntKind> {
        Some(match v {
            1 => EntKind::Peel,
            2 => EntKind::Bouncer,
            3 => EntKind::Seeker,
            4 => EntKind::Nova,
            5 => EntKind::Bomb,
            6 => EntKind::Decoy,
            7 => EntKind::Blast,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Ent {
    pub kind: EntKind,
    pub pos: V2,
    pub vel: V2,
    pub owner: u8,
    pub age: f32,
    /// Bomb fuse / blast lifetime remaining.
    pub timer: f32,
    pub target: u8,
    pub run: i32,
    pub bounces: u8,
}

impl Ent {
    fn new(kind: EntKind, pos: V2, vel: V2, owner: usize) -> Ent {
        Ent { kind, pos, vel, owner: owner as u8, age: 0.0, timer: 0.0, target: 255, run: 0, bounces: 0 }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Input {
    pub steer: f32,
    pub throttle: bool,
    pub brake: bool,
    pub drift: bool,
    /// Click counters: wrapping, so a lost packet never loses a click.
    pub use_seq: u8,
    pub swap_seq: u8,
}

#[derive(Clone, Debug)]
pub struct Kart {
    pub name: String,
    pub is_bot: bool,
    pub pos: V2,
    pub vel: V2,
    pub heading: f32,
    pub drift_dir: i8,
    pub drift_charge: f32,
    pub boost: f32,
    pub star: f32,
    pub giant: f32,
    pub shrunk: f32,
    pub inked: f32,
    pub rocket: f32,
    pub spin: f32,
    pub invuln: f32,
    /// (item id, uses left); id 0 = empty. Slot 0 is the primary.
    pub slots: [(u8, u8); 2],
    pub coins: u8,
    pub run: i32,
    pub prog: f32,
    pub finished: bool,
    pub finish_time: f32,
    pub place: u8,
    pub input: Input,
    pub last_use: u8,
    pub last_swap: u8,
    pub lane: f32,
    pub ai_timer: f32,
}

impl Kart {
    pub fn new(name: &str, is_bot: bool) -> Kart {
        Kart {
            name: name.chars().take(NAME_LEN).collect(),
            is_bot,
            pos: V2::ZERO,
            vel: V2::ZERO,
            heading: 0.0,
            drift_dir: 0,
            drift_charge: 0.0,
            boost: 0.0,
            star: 0.0,
            giant: 0.0,
            shrunk: 0.0,
            inked: 0.0,
            rocket: 0.0,
            spin: 0.0,
            invuln: 0.0,
            slots: [(0, 0); 2],
            coins: 0,
            run: 0,
            prog: 0.0,
            finished: false,
            finish_time: 0.0,
            place: 0,
            input: Input::default(),
            last_use: 0,
            last_swap: 0,
            lane: 0.0,
            ai_timer: 2.0,
        }
    }

    pub fn scale(&self) -> f32 {
        if self.giant > 0.0 {
            2.0
        } else if self.shrunk > 0.0 {
            0.6
        } else {
            1.0
        }
    }

    pub fn radius(&self) -> f32 {
        14.0 * self.scale()
    }

    pub fn smasher(&self) -> bool {
        self.star > 0.0 || self.giant > 0.0 || self.rocket > 0.0
    }

    pub fn speed(&self) -> f32 {
        self.vel.len()
    }

    pub fn drift_tier(&self) -> u8 {
        if self.drift_dir == 0 {
            0
        } else if self.drift_charge >= 3.0 {
            3
        } else if self.drift_charge >= 1.8 {
            2
        } else if self.drift_charge >= 0.9 {
            1
        } else {
            0
        }
    }

    /// Knock the kart over. Returns false if protected.
    fn hit(&mut self, big: bool) -> bool {
        if self.smasher() || self.invuln > 0.0 || self.spin > 0.0 {
            return false;
        }
        self.spin = if big { 1.8 } else { 1.3 };
        self.invuln = self.spin + 0.8;
        self.vel = self.vel * 0.25;
        self.drift_dir = 0;
        self.drift_charge = 0.0;
        self.boost = 0.0;
        self.coins = self.coins.saturating_sub(2);
        true
    }
}

#[derive(Clone)]
pub struct GameState {
    pub phase: Phase,
    /// Countdown remaining, or race clock, or time since results began.
    pub timer: f32,
    pub laps: u8,
    pub bots: u8,
    pub karts: Vec<Kart>,
    pub ents: Vec<Ent>,
    pub boxes: u32,
    pub coins: u64,
    pub box_timer: Vec<f32>,
    pub coin_timer: Vec<f32>,
    pub first_finish: Option<f32>,
    pub rng: u64,
}

impl GameState {
    pub fn new(tr: &Track) -> GameState {
        GameState {
            phase: Phase::Lobby,
            timer: 0.0,
            laps: 3,
            bots: 3,
            karts: Vec::new(),
            ents: Vec::new(),
            boxes: (1u64 << tr.box_pos.len()) as u32 - 1,
            coins: if tr.coin_pos.len() >= 64 { u64::MAX } else { (1u64 << tr.coin_pos.len()) - 1 },
            box_timer: vec![0.0; tr.box_pos.len()],
            coin_timer: vec![0.0; tr.coin_pos.len()],
            first_finish: None,
            rng: 0x1234_5678_9abc_def1,
        }
    }

    pub fn humans(&self) -> usize {
        self.karts.iter().filter(|k| !k.is_bot).count()
    }

    pub fn add_human(&mut self, tr: &Track, name: &str) -> Option<usize> {
        if self.phase != Phase::Lobby || self.karts.len() >= MAX_KARTS {
            return None;
        }
        self.karts.push(Kart::new(name, false));
        self.grid(tr);
        Some(self.karts.len() - 1)
    }

    /// Remove a player. In a race the kart is handed to the autopilot.
    pub fn remove_human(&mut self, tr: &Track, i: usize) {
        if i >= self.karts.len() {
            return;
        }
        if self.phase == Phase::Lobby {
            self.karts.remove(i);
            self.grid(tr);
        } else {
            self.karts[i].is_bot = true;
        }
    }

    /// Put karts on the starting grid, behind the line.
    pub fn grid(&mut self, tr: &Track) {
        let mut seed = self.rng;
        for (i, k) in self.karts.iter_mut().enumerate() {
            let back = 8 + (i as i32 / 2) * 7;
            let run = -back;
            let side = if i % 2 == 0 { -30.0 } else { 30.0 };
            let t = tr.tangent(run);
            let name = k.name.clone();
            let bot = k.is_bot;
            *k = Kart::new(&name, bot);
            k.pos = tr.pt(run) + t.right() * side;
            k.heading = t.y.atan2(t.x);
            k.run = run;
            k.prog = run as f32;
            k.place = i as u8;
            k.lane = (rng_f32(&mut seed) - 0.5) * 90.0;
        }
        self.rng = seed;
    }

    pub fn start_race(&mut self, tr: &Track) {
        let free = MAX_KARTS - self.karts.len();
        let nb = (self.bots as usize).min(free);
        for b in 0..nb {
            self.karts.push(Kart::new(&format!("Bot {}", b + 1), true));
        }
        self.grid(tr);
        self.ents.clear();
        self.boxes = (1u64 << tr.box_pos.len()) as u32 - 1;
        self.coins = (1u64 << tr.coin_pos.len()) - 1;
        self.box_timer.iter_mut().for_each(|t| *t = 0.0);
        self.coin_timer.iter_mut().for_each(|t| *t = 0.0);
        self.first_finish = None;
        self.phase = Phase::Countdown;
        self.timer = COUNTDOWN;
    }

    pub fn to_lobby(&mut self, tr: &Track) {
        self.karts.retain(|k| !k.is_bot);
        self.ents.clear();
        self.phase = Phase::Lobby;
        self.timer = 0.0;
        self.grid(tr);
    }

    /// Debug: drop a new bot just behind the host's kart mid-race.
    #[cfg(feature = "debug-tools")]
    pub fn debug_add_bot(&mut self, tr: &Track) {
        if self.karts.len() >= MAX_KARTS || !matches!(self.phase, Phase::Racing | Phase::Countdown) || self.karts.is_empty() {
            return;
        }
        let bots = self.karts.iter().filter(|k| k.is_bot).count();
        let mut k = Kart::new(&format!("Bot {}", bots + 1), true);
        let run = self.karts[0].run - 10;
        let mut seed = self.rng ^ self.karts.len() as u64;
        k.lane = (rng_f32(&mut seed) - 0.5) * 90.0;
        self.rng = seed;
        let t = tr.tangent(run);
        k.pos = tr.pt(run) + t.right() * k.lane;
        k.heading = t.y.atan2(t.x);
        k.run = run;
        k.prog = run as f32;
        k.place = self.karts.len() as u8;
        self.karts.push(k);
    }

    #[cfg(feature = "debug-tools")]
    pub fn debug_remove_bot(&mut self) {
        if self.karts.len() > 1 && self.karts.last().map_or(false, |k| k.is_bot) {
            self.karts.pop();
            self.ents.retain(|e| (e.owner as usize) < self.karts.len() || e.owner == 255);
        }
    }

    fn roll_item(&mut self, place: usize) -> Item {
        let n = self.karts.len();
        let t = if n <= 1 { 0.0 } else { place as f32 / (n - 1) as f32 };
        let w = |it: Item| -> f32 {
            let (f, b) = it.weights();
            if place == 0 && matches!(it, Item::Nova | Item::Zap | Item::Rocket | Item::Ink) {
                0.0
            } else {
                lerp(f, b, t)
            }
        };
        let total: f32 = Item::ALL.iter().map(|&i| w(i)).sum();
        let mut r = rng_f32(&mut self.rng) * total;
        for &it in Item::ALL.iter() {
            r -= w(it);
            if r <= 0.0 && w(it) > 0.0 {
                return it;
            }
        }
        Item::Peel
    }

    fn rank_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.karts.len()).collect();
        order.sort_by(|&a, &b| {
            let (ka, kb) = (&self.karts[a], &self.karts[b]);
            match (ka.finished, kb.finished) {
                (true, true) => ka.finish_time.partial_cmp(&kb.finish_time).unwrap(),
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => kb.prog.partial_cmp(&ka.prog).unwrap(),
            }
        });
        order
    }

    pub fn step(&mut self, tr: &Track) {
        match self.phase {
            Phase::Lobby => return,
            Phase::Countdown => {
                self.timer -= DT;
                if self.timer <= 0.0 {
                    self.phase = Phase::Racing;
                    self.timer = 0.0;
                }
            }
            Phase::Racing => self.timer += DT,
            Phase::Results => {
                self.timer += DT;
                return;
            }
        }
        let racing = self.phase == Phase::Racing;
        let n = self.karts.len();
        let laps_total = self.laps as f32 * tr.n as f32;

        // --- controls + driving
        let mut uses = Vec::new();
        for i in 0..n {
            let mut k = std::mem::replace(&mut self.karts[i], Kart::new("", true));
            let auto = k.is_bot || k.finished || k.rocket > 0.0;
            let mut inp = if !racing {
                Input::default()
            } else if k.rocket > 0.0 {
                autopilot(&k, tr, 0.0, 18)
            } else if auto {
                let mut a = autopilot(&k, tr, k.lane, 14);
                a.use_seq = k.input.use_seq;
                a
            } else {
                k.input
            };
            if racing && k.is_bot && !k.finished {
                if bot_wants_item(&mut k, &self.karts, i, &mut self.rng) {
                    inp.use_seq = inp.use_seq.wrapping_add(1);
                }
                // bots are slightly slower than a perfect human
            }
            if k.is_bot {
                k.input.use_seq = inp.use_seq;
            }
            if racing {
                if inp.swap_seq != k.last_swap {
                    k.last_swap = inp.swap_seq;
                    k.slots.swap(0, 1);
                }
                if inp.use_seq != k.last_use {
                    k.last_use = inp.use_seq;
                    if k.spin <= 0.0 {
                        uses.push(i);
                    }
                }
            } else {
                k.last_use = inp.use_seq;
                k.last_swap = inp.swap_seq;
            }
            if k.slots[0].0 == 0 {
                k.slots.swap(0, 1);
            }
            drive(&mut k, inp, tr, self.laps, self.timer, &mut self.first_finish, racing, laps_total);
            self.karts[i] = k;
        }
        for i in uses {
            self.use_item(i, tr);
        }

        // --- pickups
        for i in 0..n {
            let (pos, r) = (self.karts[i].pos, self.karts[i].radius());
            for b in 0..tr.box_pos.len() {
                if self.boxes & (1 << b) != 0 && tr.box_pos[b].dist(pos) < 24.0 + r {
                    let has_room = self.karts[i].slots.iter().any(|s| s.0 == 0);
                    if has_room && racing {
                        self.boxes &= !(1 << b);
                        self.box_timer[b] = 4.0;
                        let it = self.roll_item(self.karts[i].place as usize);
                        let k = &mut self.karts[i];
                        let s = if k.slots[0].0 == 0 { 0 } else { 1 };
                        k.slots[s] = (it as u8, it.uses());
                    }
                }
            }
            for c in 0..tr.coin_pos.len() {
                if self.coins & (1 << c) != 0 && tr.coin_pos[c].dist(pos) < 16.0 + r {
                    if self.karts[i].coins < 10 {
                        self.karts[i].coins += 1;
                    }
                    self.coins &= !(1 << c);
                    self.coin_timer[c] = 12.0;
                }
            }
        }
        for b in 0..self.box_timer.len() {
            if self.boxes & (1 << b) == 0 {
                self.box_timer[b] -= DT;
                if self.box_timer[b] <= 0.0 {
                    self.boxes |= 1 << b;
                }
            }
        }
        for c in 0..self.coin_timer.len() {
            if self.coins & (1 << c) == 0 {
                self.coin_timer[c] -= DT;
                if self.coin_timer[c] <= 0.0 {
                    self.coins |= 1 << c;
                }
            }
        }

        self.kart_collisions();

        // --- entities
        let mut old = std::mem::take(&mut self.ents);
        let mut keep = Vec::with_capacity(old.len());
        for mut e in old.drain(..) {
            if self.update_ent(&mut e, tr) {
                keep.push(e);
            }
        }
        keep.extend(std::mem::take(&mut self.ents));
        keep.truncate(MAX_ENTS);
        self.ents = keep;

        // --- ranking + race end
        let order = self.rank_order();
        for (p, &i) in order.iter().enumerate() {
            self.karts[i].place = p as u8;
        }
        if racing {
            let humans_done = self.karts.iter().filter(|k| !k.is_bot).all(|k| k.finished);
            let all_done = self.karts.iter().all(|k| k.finished);
            let timeout = self.first_finish.map_or(false, |t| self.timer - t > 30.0);
            if all_done || timeout || (humans_done && self.humans() > 0) {
                self.phase = Phase::Results;
                self.timer = 0.0;
            }
        }
    }

    fn kart_collisions(&mut self) {
        let n = self.karts.len();
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (&self.karts[i], &self.karts[j]);
                let rad = a.radius() + b.radius();
                let d = b.pos - a.pos;
                let dist = d.len();
                if dist >= rad || dist < 0.01 {
                    continue;
                }
                let nrm = d * (1.0 / dist);
                let (ma, mb) = (a.scale().powi(2), b.scale().powi(2));
                let over = rad - dist;
                let (wa, wb) = (mb / (ma + mb), ma / (ma + mb));
                let rel = (b.vel - a.vel).dot(nrm);
                let (a_smash, b_smash) = (a.smasher(), b.smasher());
                let (ga, gb) = (a.giant > 0.0, b.giant > 0.0);
                {
                    let (left, right) = self.karts.split_at_mut(j);
                    let (a, b) = (&mut left[i], &mut right[0]);
                    a.pos = a.pos - nrm * (over * wa);
                    b.pos = b.pos + nrm * (over * wb);
                    if rel < 0.0 {
                        a.vel = a.vel + nrm * (rel * wa * 1.2);
                        b.vel = b.vel - nrm * (rel * wb * 1.2);
                    }
                    if a_smash && !b_smash {
                        b.hit(ga || a.rocket > 0.0);
                    } else if b_smash && !a_smash {
                        a.hit(gb || b.rocket > 0.0);
                    }
                }
            }
        }
    }

    fn nearest_ahead(&self, of: usize) -> u8 {
        let me = self.karts[of].prog;
        let mut best = (f32::MAX, 255u8);
        for (j, k) in self.karts.iter().enumerate() {
            let d = k.prog - me;
            if j != of && d > 0.0 && d < best.0 {
                best = (d, j as u8);
            }
        }
        best.1
    }

    fn leader_excluding(&self, owner: usize) -> u8 {
        let mut best = (f32::MIN, 255u8);
        for (j, k) in self.karts.iter().enumerate() {
            if j != owner && k.prog > best.0 {
                best = (k.prog, j as u8);
            }
        }
        best.1
    }

    fn spawn(&mut self, e: Ent) {
        if self.ents.len() < MAX_ENTS {
            self.ents.push(e);
        }
    }

    fn use_item(&mut self, i: usize, tr: &Track) {
        let (id, count) = self.karts[i].slots[0];
        let Some(item) = Item::from_u8(id) else { return };
        let k = &self.karts[i];
        let fwd = V2::from_angle(k.heading);
        let (pos, sc, run, vel) = (k.pos, k.scale(), k.run, k.vel);
        let behind = pos - fwd * (26.0 * sc);
        let ahead = pos + fwd * (30.0 * sc);
        match item {
            Item::Peel | Item::TriplePeel => {
                let mut e = Ent::new(EntKind::Peel, behind, V2::ZERO, i);
                e.timer = 0.6;
                self.spawn(e);
            }
            Item::Decoy => {
                let mut e = Ent::new(EntKind::Decoy, behind, V2::ZERO, i);
                e.timer = 0.6;
                self.spawn(e);
            }
            Item::Bouncer | Item::TripleBouncer => {
                self.spawn(Ent::new(EntKind::Bouncer, ahead, fwd * 640.0, i));
            }
            Item::Seeker => {
                let mut e = Ent::new(EntKind::Seeker, ahead, fwd * 540.0, i);
                e.target = self.nearest_ahead(i);
                e.run = run;
                self.spawn(e);
            }
            Item::Nova => {
                let mut e = Ent::new(EntKind::Nova, pos, fwd * 850.0, i);
                e.target = self.leader_excluding(i);
                e.run = run;
                self.spawn(e);
            }
            Item::Bomb => {
                let mut e = Ent::new(EntKind::Bomb, ahead, fwd * 380.0 + vel * 0.5, i);
                e.timer = 2.8;
                self.spawn(e);
            }
            Item::Turbo | Item::TripleTurbo => self.karts[i].boost = self.karts[i].boost.max(1.3),
            Item::Nitro => self.karts[i].boost = self.karts[i].boost.max(1.6),
            Item::Giant => {
                let k = &mut self.karts[i];
                k.giant = 8.0;
                k.shrunk = 0.0;
            }
            Item::Star => self.karts[i].star = 8.0,
            Item::Rocket => {
                let k = &mut self.karts[i];
                k.rocket = 6.0;
                k.invuln = 6.5;
                k.drift_dir = 0;
            }
            Item::Zap => {
                for j in 0..self.karts.len() {
                    if j == i {
                        continue;
                    }
                    let v = &mut self.karts[j];
                    if v.giant > 0.0 {
                        v.giant = 0.0;
                    }
                    if v.hit(true) {
                        v.shrunk = 6.0;
                    }
                }
            }
            Item::Ink => {
                let my = self.karts[i].place;
                for j in 0..self.karts.len() {
                    if j != i && self.karts[j].place < my && !self.karts[j].smasher() {
                        self.karts[j].inked = 6.0;
                    }
                }
            }
        }
        let k = &mut self.karts[i];
        k.slots[0].1 = count.saturating_sub(1);
        if k.slots[0].1 == 0 {
            k.slots[0] = (0, 0);
        }
        let _ = tr;
    }

    fn explode(&mut self, pos: V2, radius: f32, skip: Option<usize>) {
        for (j, k) in self.karts.iter_mut().enumerate() {
            if Some(j) != skip && k.pos.dist(pos) < radius + k.radius() {
                k.hit(true);
            }
        }
        let mut b = Ent::new(EntKind::Blast, pos, V2::ZERO, 255);
        b.timer = 0.45;
        b.age = radius;
        self.spawn(b);
    }

    fn contact(&self, e: &Ent, r: f32) -> Option<usize> {
        for (j, k) in self.karts.iter().enumerate() {
            if j == e.owner as usize && e.age < 0.6 {
                continue;
            }
            if k.pos.dist(e.pos) < r + k.radius() {
                return Some(j);
            }
        }
        None
    }

    /// Returns false when the entity should be removed.
    fn update_ent(&mut self, e: &mut Ent, tr: &Track) -> bool {
        e.age += DT;
        match e.kind {
            EntKind::Blast => {
                e.timer -= DT;
                e.timer > 0.0
            }
            EntKind::Peel | EntKind::Decoy => {
                if let Some(j) = self.contact(e, 12.0) {
                    let k = &mut self.karts[j];
                    if !k.smasher() {
                        k.hit(e.kind == EntKind::Decoy);
                    }
                    return false;
                }
                true
            }
            EntKind::Bouncer => {
                e.pos += e.vel * DT;
                if tr.dist_at(e.pos) > SHELL_LIMIT {
                    let n = tr.outward(e.pos);
                    e.vel = e.vel - n * (2.0 * e.vel.dot(n));
                    e.pos = e.pos - n * 4.0;
                    e.bounces += 1;
                }
                if let Some(j) = self.contact(e, 9.0) {
                    self.karts[j].hit(false);
                    return false;
                }
                e.bounces < 6 && e.age < 12.0
            }
            EntKind::Seeker | EntKind::Nova => {
                let nova = e.kind == EntKind::Nova;
                let speed = if nova { 850.0 } else { 540.0 };
                if nova {
                    e.target = self.leader_excluding(e.owner as usize);
                }
                let (run, _, _) = tr.locate(e.pos, e.run);
                e.run = run;
                let tgt = self.karts.get(e.target as usize).map(|k| k.pos);
                let homing = tgt.map_or(false, |t| t.dist(e.pos) < if nova { 520.0 } else { 380.0 });
                let want = if homing {
                    tgt.unwrap() - e.pos
                } else {
                    tr.pt(run + 9) - e.pos
                };
                let cur = e.vel.y.atan2(e.vel.x);
                let diff = wrap_angle(want.y.atan2(want.x) - cur);
                let lim = if homing { 5.0 } else { 6.0 } * DT;
                e.vel = V2::from_angle(cur + diff.clamp(-lim, lim)) * speed;
                e.pos += e.vel * DT;
                if nova {
                    let hit_tgt = tgt.map_or(false, |t| t.dist(e.pos) < 40.0);
                    if hit_tgt || e.age > 15.0 {
                        self.explode(e.pos, 140.0, Some(e.owner as usize));
                        return false;
                    }
                    true
                } else {
                    if let Some(j) = self.contact(e, 9.0) {
                        self.karts[j].hit(false);
                        return false;
                    }
                    e.age < 14.0
                }
            }
            EntKind::Bomb => {
                e.timer -= DT;
                e.pos += e.vel * DT;
                e.vel = e.vel * (1.0 - 1.6 * DT);
                if tr.dist_at(e.pos) > SHELL_LIMIT {
                    let n = tr.outward(e.pos);
                    e.vel = (e.vel - n * (2.0 * e.vel.dot(n))) * 0.5;
                    e.pos = e.pos - n * 4.0;
                }
                let touch = e.age > 0.5 && self.contact(e, 12.0).is_some();
                if e.timer <= 0.0 || touch {
                    self.explode(e.pos, 110.0, None);
                    return false;
                }
                true
            }
        }
    }
}

/// Steering/throttle for bots, finished karts and rocket mode.
pub fn autopilot(k: &Kart, tr: &Track, lane: f32, look: i32) -> Input {
    let ti = k.run + look;
    let tgt = tr.pt(ti) + tr.tangent(ti).right() * lane;
    let want = tgt - k.pos;
    let ang = wrap_angle(want.y.atan2(want.x) - k.heading);
    Input {
        steer: (ang * 2.2).clamp(-1.0, 1.0),
        throttle: !(ang.abs() > 1.1 && k.speed() > 220.0),
        ..Input::default()
    }
}

fn bot_wants_item(k: &mut Kart, all: &[Kart], me: usize, rng: &mut u64) -> bool {
    k.ai_timer -= DT;
    let Some(item) = Item::from_u8(k.slots[0].0) else {
        k.ai_timer = k.ai_timer.max(0.5);
        return false;
    };
    if k.ai_timer > 0.0 || k.spin > 0.0 {
        return false;
    }
    let fwd = V2::from_angle(k.heading);
    let near = |ahead: bool, range: f32| {
        all.iter().enumerate().any(|(j, o)| {
            let d = o.pos - k.pos;
            j != me && d.len() < range && (d.dot(fwd) > 0.0) == ahead
        })
    };
    let go = match item {
        Item::Peel | Item::TriplePeel | Item::Decoy | Item::Bomb => near(false, 260.0),
        Item::Bouncer | Item::TripleBouncer | Item::Seeker => near(true, 520.0),
        _ => true,
    };
    if go {
        k.ai_timer = 1.0 + rng_f32(rng) * 3.0;
    } else {
        k.ai_timer = 0.3;
    }
    go
}

#[allow(clippy::too_many_arguments)]
fn drive(
    k: &mut Kart,
    inp: Input,
    tr: &Track,
    laps: u8,
    clock: f32,
    first_finish: &mut Option<f32>,
    racing: bool,
    laps_total: f32,
) {
    // timers
    for t in [&mut k.boost, &mut k.star, &mut k.giant, &mut k.shrunk, &mut k.inked, &mut k.rocket, &mut k.invuln] {
        *t = (*t - DT).max(0.0);
    }
    let surf = tr.surface(k.pos);
    if surf == Surf::Pad {
        k.boost = k.boost.max(1.0);
    }
    let bot_handicap = if k.is_bot { 0.94 } else { 1.0 };
    let mut maxs = (360.0 + k.coins as f32 * 3.0) * bot_handicap;
    let boosting = k.boost > 0.0 || k.rocket > 0.0;
    if matches!(surf, Surf::Grass | Surf::Wall) && !boosting && !k.smasher() {
        maxs *= 0.5;
    }
    if k.star > 0.0 {
        maxs *= 1.15;
    }
    if k.shrunk > 0.0 {
        maxs *= 0.75;
    }
    if boosting {
        maxs *= if k.rocket > 0.0 { 1.45 } else { 1.4 };
    }

    let fwd = V2::from_angle(k.heading);
    let mut vf = k.vel.dot(fwd);

    if k.spin > 0.0 {
        k.spin -= DT;
        k.heading += 12.0 * DT;
        k.drift_dir = 0;
        k.drift_charge = 0.0;
        k.vel = k.vel * (1.0 - 2.5 * DT).max(0.0);
    } else {
        // --- drift state machine
        if racing && inp.drift && k.drift_dir == 0 && inp.steer.abs() > 0.3 && vf > 140.0 {
            k.drift_dir = if inp.steer > 0.0 { 1 } else { -1 };
            k.drift_charge = 0.0;
        }
        if k.drift_dir != 0 && (!inp.drift || vf < 90.0) {
            let c = k.drift_charge;
            if c >= 3.0 {
                k.boost = k.boost.max(1.5);
            } else if c >= 1.8 {
                k.boost = k.boost.max(1.0);
            } else if c >= 0.9 {
                k.boost = k.boost.max(0.55);
            }
            k.drift_dir = 0;
            k.drift_charge = 0.0;
        }
        // --- steering
        let sp = (vf.abs() / 140.0).min(1.0);
        let dir = if vf < -5.0 { -1.0 } else { 1.0 };
        let steer = if racing { inp.steer } else { 0.0 };
        let eff = if k.drift_dir != 0 {
            let d = k.drift_dir as f32;
            k.drift_charge += DT * if steer * d > 0.3 { 1.5 } else { 1.0 };
            d * (0.8 + 0.4 * steer * d)
        } else {
            steer
        };
        k.heading += eff * 2.3 * sp * (1.0 - 0.3 * (vf / maxs).clamp(0.0, 1.0)) * DT * dir;

        // --- longitudinal / lateral dynamics in the new heading frame
        let fwd = V2::from_angle(k.heading);
        let rgt = fwd.right();
        vf = k.vel.dot(fwd);
        let mut vl = k.vel.dot(rgt);
        let throttle = racing && (inp.throttle || k.rocket > 0.0);
        let accel = if boosting { 700.0 } else { 250.0 } * if k.giant > 0.0 { 0.8 } else { 1.0 };
        if throttle {
            if vf < maxs {
                vf = (vf + accel * DT).min(maxs);
            } else {
                vf = (vf - 220.0 * DT).max(maxs);
            }
        } else if racing && inp.brake {
            vf = (vf - if vf > 0.0 { 650.0 } else { 200.0 } * DT).max(-110.0);
        } else {
            vf -= vf * 0.9 * DT;
        }
        let grip = if k.drift_dir != 0 { 2.4 } else { 10.0 };
        vl *= (-grip * DT).exp();
        k.vel = fwd * vf + rgt * vl;
    }

    k.pos += k.vel * DT;

    // --- walls
    if k.rocket <= 0.0 {
        for _ in 0..6 {
            if tr.surface(k.pos) != Surf::Wall {
                break;
            }
            let n = tr.outward(k.pos);
            k.pos = k.pos - n * 3.0;
            let vn = k.vel.dot(n);
            if vn > 0.0 {
                k.vel = k.vel - n * (vn * 1.4);
            }
            k.vel = k.vel * 0.9;
            k.drift_dir = 0;
            k.drift_charge = 0.0;
        }
    }

    // --- progress
    let (run, t, _) = tr.locate(k.pos, k.run);
    k.run = run;
    k.prog = run as f32 + t;
    if racing && !k.finished && k.prog >= laps_total {
        k.finished = true;
        k.finish_time = clock;
        first_finish.get_or_insert(clock);
    }
    let _ = laps;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bots_finish_a_race() {
        let tr = Track::new();
        let mut g = GameState::new(&tr);
        g.laps = 2;
        g.bots = 7;
        g.add_human(&tr, "Tester");
        g.karts[0].is_bot = true; // let the autopilot drive it too
        g.start_race(&tr);
        for _ in 0..(60 * 400) {
            g.step(&tr);
            if g.phase == Phase::Results {
                break;
            }
        }
        assert_eq!(g.phase, Phase::Results, "race never finished; progs: {:?}", g.karts.iter().map(|k| k.prog).collect::<Vec<_>>());
        assert!(g.karts.iter().all(|k| k.finished));
        println!("race time {:.1}s", g.timer);
    }

    #[test]
    fn every_item_can_be_used() {
        let tr = Track::new();
        let mut g = GameState::new(&tr);
        for i in 0..3 {
            g.add_human(&tr, &format!("p{i}"));
        }
        g.start_race(&tr);
        for _ in 0..300 {
            g.step(&tr);
        }
        for item in Item::ALL {
            g.karts[1].slots[0] = (item as u8, item.uses());
            g.karts[1].input.use_seq = g.karts[1].input.use_seq.wrapping_add(1);
            for _ in 0..200 {
                g.step(&tr);
            }
        }
    }
}
