#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod math;
mod net;
mod render;
mod render3d;
mod sim;
mod track;

use minifb::{Key, KeyRepeat, MouseButton, Window, WindowOptions};
use net::*;
use render::*;
use render3d::draw_game3d;
use sim::*;
use std::time::Instant;
use math::rng_next;
use track::{track_name, Track};

enum Session {
    None,
    Host { host: Host, gs: GameState, port: u16 },
    Client { cl: Client },
}

enum Screen {
    Title,
    Browse(Browser, usize),
    Play,
}

struct App {
    tracks: Vec<Track>,
    name: String,
    editing_name: bool,
    msg: String,
    screen: Screen,
    session: Session,
    cam: Cam,
    use_seq: u8,
    swap_seq: u8,
    prev_left: bool,
    prev_right: bool,
    tick_acc: f32,
    lobby_tick: u32,
    quit: bool,
    mode3d: bool,
    #[allow(dead_code)]
    dbg_item: usize,
}

/// Which track the current session is playing (index into `App::tracks`).
fn track_index(session: &Session, count: usize) -> usize {
    let t = match session {
        Session::Host { gs, .. } => gs.track,
        Session::Client { cl } => cl.state.track,
        Session::None => 0,
    };
    (t as usize).min(count - 1)
}

fn default_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "Racer".into())
        .chars()
        .take(NAME_LEN)
        .collect()
}

fn key_char(k: Key, shift: bool) -> Option<char> {
    use Key::*;
    let letters = [A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z];
    if let Some(i) = letters.iter().position(|&l| l == k) {
        let c = (b'a' + i as u8) as char;
        return Some(if shift { c.to_ascii_uppercase() } else { c });
    }
    let digits = [Key0, Key1, Key2, Key3, Key4, Key5, Key6, Key7, Key8, Key9];
    digits.iter().position(|&d| d == k).map(|i| (b'0' + i as u8) as char)
}

impl App {
    fn new() -> App {
        App {
            tracks: Track::all(),
            name: default_name(),
            editing_name: false,
            msg: String::new(),
            screen: Screen::Title,
            session: Session::None,
            cam: Cam::new(),
            use_seq: 0,
            swap_seq: 0,
            prev_left: false,
            prev_right: false,
            tick_acc: 0.0,
            lobby_tick: 0,
            quit: false,
            mode3d: true,
            dbg_item: 0,
        }
    }

    fn read_input(&mut self, w: &Window) -> Input {
        let down = |a: Key, b: Key| w.is_key_down(a) || w.is_key_down(b);
        let left = w.get_mouse_down(MouseButton::Left);
        let right = w.get_mouse_down(MouseButton::Right);
        if left && !self.prev_left {
            self.use_seq = self.use_seq.wrapping_add(1);
        }
        if right && !self.prev_right {
            self.swap_seq = self.swap_seq.wrapping_add(1);
        }
        self.prev_left = left;
        self.prev_right = right;
        Input {
            steer: down(Key::D, Key::Right) as i32 as f32 - down(Key::A, Key::Left) as i32 as f32,
            throttle: down(Key::W, Key::Up),
            brake: down(Key::S, Key::Down),
            drift: down(Key::LeftShift, Key::RightShift),
            back: w.is_key_down(Key::Space) || w.get_mouse_down(MouseButton::Middle),
            use_seq: self.use_seq,
            swap_seq: self.swap_seq,
        }
    }

    /// Test helpers, only compiled into `--features debug-tools` builds.
    #[cfg(feature = "debug-tools")]
    fn debug_keys(&mut self, w: &Window) {
        let ti = track_index(&self.session, self.tracks.len());
        let tr = &self.tracks[ti];
        let Session::Host { gs, .. } = &mut self.session else { return };
        let hit = |k: Key| w.is_key_pressed(k, KeyRepeat::No);
        let n = Item::ALL.len();
        if hit(Key::F1) {
            gs.debug_add_bot(tr);
        }
        if hit(Key::F2) {
            gs.debug_remove_bot();
        }
        if hit(Key::F3) || hit(Key::F4) {
            self.dbg_item = if hit(Key::F3) { (self.dbg_item + 1) % n } else { (self.dbg_item + n - 1) % n };
            let it = Item::ALL[self.dbg_item];
            if let Some(k) = gs.karts.get_mut(0) {
                k.slots[0] = (it as u8, it.uses());
            }
        }
        if hit(Key::F5) && gs.phase == Phase::Racing {
            let run = gs.laps as i32 * tr.n as i32 - 60;
            if let Some(k) = gs.karts.get_mut(0) {
                k.run = run;
                k.prog = run as f32;
                k.pos = tr.pt(run);
                let t = tr.tangent(run);
                k.heading = t.y.atan2(t.x);
            }
        }
        if hit(Key::F6) {
            if let Some(k) = gs.karts.get_mut(0) {
                k.coins = 10;
            }
        }
        if hit(Key::F8) && gs.phase != Phase::Lobby {
            gs.to_lobby(tr);
            gs.start_race(tr);
        }
    }

    fn leave(&mut self) {
        match std::mem::replace(&mut self.session, Session::None) {
            Session::Host { mut host, .. } => host.shutdown(),
            Session::Client { cl } => cl.leave(),
            Session::None => {}
        }
        self.screen = Screen::Title;
    }

    fn start_hosting(&mut self) {
        match Host::bind(PORT) {
            Ok(host) => {
                let mut gs = GameState::new(&self.tracks[0]);
                gs.rng ^= std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64) | 1;
                gs.add_human(&self.tracks[0], &self.name);
                self.session = Session::Host { host, gs, port: PORT };
                self.screen = Screen::Play;
                self.msg.clear();
            }
            Err(e) => self.msg = format!("Cannot host: {e} (already hosting?)"),
        }
    }

    fn open_browser(&mut self) {
        match Browser::new() {
            Ok(mut b) => {
                b.query();
                self.screen = Screen::Browse(b, 0);
                self.msg.clear();
            }
            Err(e) => self.msg = format!("IPv6 networking unavailable: {e}"),
        }
    }

    fn title_keys(&mut self, w: &Window) {
        let shift = w.is_key_down(Key::LeftShift) || w.is_key_down(Key::RightShift);
        if self.editing_name {
            for k in w.get_keys_pressed(KeyRepeat::Yes) {
                match k {
                    Key::Enter | Key::Escape => self.editing_name = false,
                    Key::Backspace => {
                        self.name.pop();
                    }
                    Key::Space if self.name.len() < NAME_LEN => self.name.push(' '),
                    _ => {
                        if let Some(c) = key_char(k, shift) {
                            if self.name.chars().count() < NAME_LEN {
                                self.name.push(c);
                            }
                        }
                    }
                }
            }
            if self.name.trim().is_empty() && !self.editing_name {
                self.name = default_name();
            }
            return;
        }
        if w.is_key_pressed(Key::H, KeyRepeat::No) {
            self.start_hosting();
        } else if w.is_key_pressed(Key::J, KeyRepeat::No) {
            self.open_browser();
        } else if w.is_key_pressed(Key::V, KeyRepeat::No) {
            self.mode3d = !self.mode3d;
        } else if w.is_key_pressed(Key::N, KeyRepeat::No) {
            self.editing_name = true;
        } else if w.is_key_pressed(Key::Q, KeyRepeat::No) {
            self.quit = true;
        }
    }

    fn browse_keys(&mut self, w: &Window) {
        let mut join: Option<std::net::SocketAddr> = None;
        let mut back = false;
        if let Screen::Browse(b, sel) = &mut self.screen {
            b.poll();
            if b.hosts.is_empty() {
                *sel = 0;
            } else {
                *sel = (*sel).min(b.hosts.len() - 1);
            }
            if w.is_key_pressed(Key::Down, KeyRepeat::Yes) && *sel + 1 < b.hosts.len() {
                *sel += 1;
            }
            if w.is_key_pressed(Key::Up, KeyRepeat::Yes) && *sel > 0 {
                *sel -= 1;
            }
            if w.is_key_pressed(Key::R, KeyRepeat::No) {
                b.query();
            }
            if w.is_key_pressed(Key::Enter, KeyRepeat::No) {
                if let Some(h) = b.hosts.get(*sel) {
                    if h.joinable() {
                        join = Some(h.addr);
                    } else {
                        self.msg = "That game is full or already racing.".into();
                    }
                }
            }
            back = w.is_key_pressed(Key::Escape, KeyRepeat::No);
        }
        if let Some(addr) = join {
            if let Screen::Browse(b, _) = std::mem::replace(&mut self.screen, Screen::Play) {
                let cl = Client::new(b.sock, addr, &self.name, &self.tracks[0]);
                self.session = Session::Client { cl };
                self.msg.clear();
            }
        } else if back {
            self.screen = Screen::Title;
            self.msg.clear();
        }
    }

    /// One frame of in-game logic. `dt` is the real frame time.
    fn play_frame(&mut self, w: &Window, dt: f32) {
        let inp = self.read_input(w);
        if w.is_key_pressed(Key::V, KeyRepeat::No) {
            self.mode3d = !self.mode3d;
        }
        let esc = w.is_key_pressed(Key::Escape, KeyRepeat::No);
        let enter = w.is_key_pressed(Key::Enter, KeyRepeat::No);
        let mut leave = esc;
        #[cfg(feature = "debug-tools")]
        self.debug_keys(w);
        let tracks = &self.tracks;
        match &mut self.session {
            Session::Host { host, gs, .. } => {
                host.poll(gs, &tracks[gs.track as usize]);
                if let Some(k) = gs.karts.get_mut(0) {
                    k.input = inp;
                }
                match gs.phase {
                    Phase::Lobby => {
                        if enter {
                            let pick = if gs.track_sel == 255 { (rng_next(&mut gs.rng) % tracks.len() as u64) as u8 } else { gs.track_sel };
                            gs.track = pick;
                            gs.start_race(&tracks[pick as usize]);
                        }
                        if w.is_key_pressed(Key::T, KeyRepeat::No) {
                            gs.track_sel = match gs.track_sel {
                                255 => 0,
                                t if t as usize + 1 >= tracks.len() => 255,
                                t => t + 1,
                            };
                            if gs.track_sel != 255 {
                                gs.track = gs.track_sel;
                            }
                        }
                        if w.is_key_pressed(Key::B, KeyRepeat::No) {
                            gs.bots = (gs.bots + 1) % 8;
                        }
                        if w.is_key_pressed(Key::L, KeyRepeat::No) {
                            gs.laps = gs.laps % 9 + 1;
                        }
                    }
                    Phase::Results if enter => gs.to_lobby(&tracks[gs.track as usize]),
                    _ => {}
                }
                self.tick_acc += dt.min(0.1);
                while self.tick_acc >= DT {
                    self.tick_acc -= DT;
                    gs.step(&tracks[gs.track as usize]);
                    self.lobby_tick += 1;
                    if gs.phase != Phase::Lobby || self.lobby_tick % 6 == 0 {
                        host.broadcast(gs);
                    }
                }
            }
            Session::Client { cl } => {
                cl.poll();
                cl.send_input(inp);
                if let Some(r) = cl.rejected {
                    self.msg = if r == 1 { "Race already in progress.".into() } else { "Game is full.".into() };
                    leave = true;
                } else if cl.host_gone {
                    self.msg = "Lost connection to the host.".into();
                    leave = true;
                }
            }
            Session::None => leave = true,
        }
        if leave {
            let m = std::mem::take(&mut self.msg);
            self.leave();
            self.msg = m;
            return;
        }
        // camera follows our own kart
        if let Some((gs, me)) = self.view() {
            self.cam.update_vis(&gs, dt);
            if let Some(k) = gs.karts.get(me) {
                if matches!(gs.phase, Phase::Lobby) {
                    self.cam.pos = k.pos;
                    self.cam.ang = k.heading;
                } else {
                    if self.mode3d {
                        self.cam.follow3(k, dt);
                    } else {
                        self.cam.follow(k, dt);
                    }
                }
            }
        }
    }

    /// Game state to draw (clients extrapolate a little between snapshots).
    fn view(&self) -> Option<(GameState, usize)> {
        match &self.session {
            Session::Host { gs, .. } => Some((gs.clone(), 0)),
            Session::Client { cl } => {
                let id = cl.id?;
                cl.snap_at?;
                let mut gs = cl.state.clone();
                let age = cl.snap_at.unwrap().elapsed().as_secs_f32().min(0.1);
                if gs.phase == Phase::Racing || gs.phase == Phase::Countdown {
                    for k in gs.karts.iter_mut() {
                        k.pos += k.vel * age;
                        k.spin = (k.spin - age).max(0.0);
                    }
                }
                Some((gs, id))
            }
            Session::None => None,
        }
    }

    fn draw(&mut self, fb: &mut Fb, time: f32) {
        match &self.screen {
            Screen::Title => draw_title(fb, &self.name, self.editing_name, self.mode3d, &self.msg, time),
            Screen::Browse(b, sel) => draw_browse(fb, &b.hosts, *sel, &self.msg, time),
            Screen::Play => {
                let is_host = matches!(self.session, Session::Host { .. });
                let port = match &self.session {
                    Session::Host { port, .. } => Some(*port),
                    _ => None,
                };
                let ti = track_index(&self.session, self.tracks.len());
                let tr = &self.tracks[ti];
                match self.view() {
                    None => draw_connecting(fb, time),
                    Some((gs, me)) => match gs.phase {
                        Phase::Lobby => {
                            let label = if gs.track_sel == 255 { "Random".to_string() } else { track_name(gs.track_sel as usize).to_string() };
                            draw_lobby(fb, &gs, me, is_host, port, &label, time)
                        }
                        _ => {
                            if self.mode3d {
                                draw_game3d(fb, tr, &gs, me, &self.cam, time);
                            } else {
                                draw_game(fb, tr, &gs, me, &self.cam, time);
                            }
                            if gs.phase == Phase::Results {
                                draw_results(fb, &gs, me, is_host);
                            }
                        }
                    },
                }
            }
        }
    }
}

/// `z-cart --shot out.ppm`: render one frame of a bot race, for debugging.
fn screenshot(path: &str, three_d: bool, track: usize, extra: &str) {
    let tr = Track::build(track);
    let mut gs = GameState::new(&tr);
    gs.bots = 7;
    gs.add_human(&tr, "You");
    gs.karts[0].is_bot = true;
    gs.start_race(&tr);
    for _ in 0..(60 * 14) {
        gs.step(&tr);
    }
    gs.karts[0].is_bot = false;
    gs.karts[0].slots = [(Item::Seeker as u8, 1), (Item::TripleTurbo as u8, 3)];
    gs.karts[0].coins = 6;
    gs.ents.push(Ent { kind: EntKind::Peel, pos: gs.karts[0].pos + math::V2::from_angle(gs.karts[0].heading) * 200.0, vel: math::V2::ZERO, owner: 9, age: 1.0, timer: 0.0, target: 0, run: 0, bounces: 0, rev: false });
    let mut cam = Cam::new();
    cam.pos = gs.karts[0].pos;
    cam.ang = gs.karts[0].heading;
    let mut fb = Fb::new();
    if three_d {
        cam.follow3(&gs.karts[0], 1.0);
        draw_game3d(&mut fb, &tr, &gs, 0, &cam, 3.3);
    } else {
        cam.follow(&gs.karts[0], 1.0);
        draw_game(&mut fb, &tr, &gs, 0, &cam, 3.3);
    }
    match extra {
        "results" => {
            gs.phase = Phase::Results;
            for k in gs.karts.iter_mut() {
                k.finished = true;
                k.finish_time = 95.0 + k.place as f32 * 3.3;
            }
            draw_results(&mut fb, &gs, 0, true);
        }
        "title" => draw_title(&mut fb, "Zexolver", false, true, "", 1.0),
        "lobby" => draw_lobby(&mut fb, &gs, 0, true, Some(PORT), "Random", 1.0),
        _ => {}
    }
    let mut out = format!("P6\n{W} {H}\n255\n").into_bytes();
    for p in &fb.px {
        out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    std::fs::write(path, out).expect("write screenshot");
}

/// Scales the fixed-size frame into a window-sized buffer, letterboxed. Doing this
/// ourselves (instead of letting the window library stretch it) keeps maximized windows correct.
fn present(src: &[u32], ww: usize, wh: usize, out: &mut Vec<u32>) {
    out.clear();
    out.resize(ww * wh, 0);
    let scale = (ww as f32 / W as f32).min(wh as f32 / H as f32);
    let (dw, dh) = (((W as f32 * scale) as usize).max(1), ((H as f32 * scale) as usize).max(1));
    let (ox, oy) = ((ww - dw.min(ww)) / 2, (wh - dh.min(wh)) / 2);
    let xmap: Vec<usize> = (0..dw).map(|x| x * W / dw).collect();
    let mut prev_sy = usize::MAX;
    for y in 0..dh.min(wh) {
        let sy = y * H / dh;
        let row = (oy + y) * ww + ox;
        if sy == prev_sy {
            out.copy_within(row - ww..row - ww + dw.min(ww), row);
        } else {
            let srow = &src[sy * W..(sy + 1) * W];
            for (x, &sx) in xmap.iter().enumerate().take(ww) {
                out[row + x] = srow[sx];
            }
            prev_sy = sy;
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "--shot" {
        return screenshot(&args[2], args.get(3).map_or(false, |a| a == "3d"), args.get(4).and_then(|a| a.parse().ok()).unwrap_or(0), args.get(5).map_or("", |a| a.as_str()));
    }
    let mut window = Window::new(
        "Z-Cart",
        W,
        H,
        WindowOptions { resize: true, scale_mode: minifb::ScaleMode::UpperLeft, ..WindowOptions::default() },
    )
    .expect("could not open a window");
    window.set_target_fps(60);
    let mut fb = Fb::new();
    let mut out: Vec<u32> = Vec::new();
    let mut app = App::new();
    let start = Instant::now();
    let mut last = start;
    while window.is_open() && !app.quit {
        let now = Instant::now();
        let dt = (now - last).as_secs_f32();
        last = now;
        let time = (now - start).as_secs_f32();
        match app.screen {
            Screen::Title => app.title_keys(&window),
            Screen::Browse(..) => app.browse_keys(&window),
            Screen::Play => app.play_frame(&window, dt),
        }
        app.draw(&mut fb, time);
        #[cfg(feature = "debug-tools")]
        {
            fb.text_shadow(6, H as i32 - 10, "DEBUG BUILD", 1, 0xFF5252);
            if matches!(app.screen, Screen::Play) {
                let help = ["F1 add bot  F2 remove bot", "F3/F4 cycle item", "F5 jump to last lap", "F6 max coins  F8 restart"];
                for (i, l) in help.iter().enumerate() {
                    fb.text_shadow(W as i32 - 210, H as i32 - 60 + i as i32 * 12, l, 1, 0xFF8A80);
                }
            }
        }
        let (ww, wh) = window.get_size();
        if ww == 0 || wh == 0 {
            window.update();
            continue;
        }
        present(&fb.px, ww, wh, &mut out);
        window.update_with_buffer(&out, ww, wh).expect("present failed");
    }
    app.leave();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_letterboxes_any_window_size() {
        let src = vec![0xFF00FF; W * H];
        let mut out = Vec::new();
        for (ww, wh) in [(960, 540), (1920, 1080), (2560, 900), (500, 1400), (3, 3), (1, 1)] {
            present(&src, ww, wh, &mut out);
            assert_eq!(out.len(), ww * wh);
            assert_eq!(out[(wh / 2) * ww + ww / 2], 0xFF00FF, "{ww}x{wh}: centre must be picture");
        }
        present(&src, 2560, 900, &mut out);
        assert_eq!(out[450 * 2560 + 10], 0, "side bars are black, picture is centred");
        assert_eq!(out[450 * 2560 + 2550], 0);
    }
}
