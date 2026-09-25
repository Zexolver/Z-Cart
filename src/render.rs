//! Software renderer: everything is drawn into a plain u32 framebuffer.

use crate::math::*;
use crate::sim::*;
use crate::track::*;
use font8x8::legacy::BASIC_LEGACY;

pub const W: usize = 960;
pub const H: usize = 540;
const CX: f32 = W as f32 / 2.0;
const CY: f32 = H as f32 * 0.70;

pub const KART_COLORS: [u32; 8] = [
    0xE53935, 0x1E88E5, 0x43A047, 0xFDD835, 0x8E24AA, 0xFB8C00, 0x00ACC1, 0xEC407A,
];

pub struct Fb {
    pub px: Vec<u32>,
}

impl Fb {
    pub fn new() -> Fb {
        Fb { px: vec![0; W * H] }
    }

    pub fn clear(&mut self, c: u32) {
        self.px.iter_mut().for_each(|p| *p = c);
    }

    pub fn put(&mut self, x: i32, y: i32, c: u32) {
        if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
            self.px[y as usize * W + x as usize] = c;
        }
    }

    pub fn blend(&mut self, x: i32, y: i32, c: u32, a: f32) {
        if x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H {
            let d = &mut self.px[y as usize * W + x as usize];
            *d = mix(*d, c, a);
        }
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32) {
        for yy in y.max(0)..(y + h).min(H as i32) {
            for xx in x.max(0)..(x + w).min(W as i32) {
                self.px[yy as usize * W + xx as usize] = c;
            }
        }
    }

    pub fn rect_alpha(&mut self, x: i32, y: i32, w: i32, h: i32, c: u32, a: f32) {
        for yy in y.max(0)..(y + h).min(H as i32) {
            for xx in x.max(0)..(x + w).min(W as i32) {
                let d = &mut self.px[yy as usize * W + xx as usize];
                *d = mix(*d, c, a);
            }
        }
    }

    /// Rounded rectangle, optionally translucent.
    pub fn rrect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, c: u32, alpha: f32) {
        let r = r.min(w / 2).min(h / 2).max(0);
        for yy in y.max(0)..(y + h).min(H as i32) {
            for xx in x.max(0)..(x + w).min(W as i32) {
                let (lx, ly) = (xx - x, yy - y);
                let cx = if lx < r { r - lx } else if lx >= w - r { lx - (w - r - 1) } else { 0 };
                let cy = if ly < r { r - ly } else if ly >= h - r { ly - (h - r - 1) } else { 0 };
                if cx * cx + cy * cy > r * r + r {
                    continue;
                }
                let d = &mut self.px[yy as usize * W + xx as usize];
                *d = if alpha >= 1.0 { c } else { mix(*d, c, alpha) };
            }
        }
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: u32) {
        let r2 = r * r;
        for y in ((cy - r).floor() as i32).max(0)..=((cy + r).ceil() as i32).min(H as i32 - 1) {
            for x in ((cx - r).floor() as i32).max(0)..=((cx + r).ceil() as i32).min(W as i32 - 1) {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                if dx * dx + dy * dy <= r2 {
                    self.px[y as usize * W + x as usize] = c;
                }
            }
        }
    }

    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, w: f32, c: u32) {
        let (o, i) = ((r + w / 2.0).powi(2), (r - w / 2.0).max(0.0).powi(2));
        let e = r + w;
        for y in ((cy - e).floor() as i32).max(0)..=((cy + e).ceil() as i32).min(H as i32 - 1) {
            for x in ((cx - e).floor() as i32).max(0)..=((cx + e).ceil() as i32).min(W as i32 - 1) {
                let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                let d = dx * dx + dy * dy;
                if d <= o && d >= i {
                    self.px[y as usize * W + x as usize] = c;
                }
            }
        }
    }

    pub fn line(&mut self, a: (f32, f32), b: (f32, f32), c: u32) {
        let n = ((b.0 - a.0).abs().max((b.1 - a.1).abs()) as i32).max(1);
        for i in 0..=n {
            let t = i as f32 / n as f32;
            self.put((a.0 + (b.0 - a.0) * t) as i32, (a.1 + (b.1 - a.1) * t) as i32, c);
        }
    }

    /// Even-odd scanline polygon fill (handles concave shapes).
    pub fn poly(&mut self, pts: &[(f32, f32)], c: u32) {
        if pts.len() < 3 {
            return;
        }
        let miny = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
        let maxy = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil().min(H as f32 - 1.0) as i32;
        let mut xs: Vec<f32> = Vec::with_capacity(8);
        for y in miny..=maxy {
            let yc = y as f32 + 0.5;
            xs.clear();
            for i in 0..pts.len() {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                if (a.1 <= yc) != (b.1 <= yc) {
                    xs.push(a.0 + (yc - a.1) / (b.1 - a.1) * (b.0 - a.0));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.chunks(2) {
                if let [x0, x1] = pair {
                    for x in (x0.round() as i32).max(0)..(x1.round() as i32).min(W as i32) {
                        self.px[y as usize * W + x as usize] = c;
                    }
                }
            }
        }
    }

    pub fn text(&mut self, x: i32, y: i32, s: &str, scale: i32, c: u32) {
        let mut cx = x;
        for ch in s.chars() {
            let g = BASIC_LEGACY[(ch as usize).min(127)];
            for (row, bits) in g.iter().enumerate() {
                for col in 0..8 {
                    if bits >> col & 1 != 0 {
                        self.rect(cx + col * scale, y + row as i32 * scale, scale, scale, c);
                    }
                }
            }
            cx += 8 * scale;
        }
    }

    pub fn text_shadow(&mut self, x: i32, y: i32, s: &str, scale: i32, c: u32) {
        self.text(x + scale, y + scale, s, scale, 0x000000);
        self.text(x, y, s, scale, c);
    }

    pub fn text_center(&mut self, cx: i32, y: i32, s: &str, scale: i32, c: u32) {
        let w = s.chars().count() as i32 * 8 * scale;
        self.text_shadow(cx - w / 2, y, s, scale, c);
    }
}

pub fn mix(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 255) as f32, ((b >> s) & 255) as f32);
        (x + (y - x) * t) as u32 & 255
    };
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

pub fn shade(c: u32, f: f32) -> u32 {
    let ch = |s: u32| (((c >> s) & 255) as f32 * f).min(255.0) as u32;
    ch(16) << 16 | ch(8) << 8 | ch(0)
}

pub fn hue(h: f32) -> u32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let f = h.fract();
    let (r, g, b) = match h as i32 {
        0 => (1.0, f, 0.0),
        1 => (1.0 - f, 1.0, 0.0),
        2 => (0.0, 1.0, f),
        3 => (0.0, 1.0 - f, 1.0),
        4 => (f, 0.0, 1.0),
        _ => (1.0, 0.0, 1.0 - f),
    };
    ((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32
}

// ------------------------------------------------------------------ camera

#[derive(Clone, Copy)]
pub struct Cam {
    pub pos: V2,
    pub ang: f32,
    pub zoom: f32,
    /// Smoothed cosmetic drift lean per kart (3D view).
    pub vis: [f32; 8],
}

impl Cam {
    pub fn new() -> Cam {
        Cam { pos: V2::ZERO, ang: 0.0, zoom: 1.3, vis: [0.0; 8] }
    }

    pub fn update_vis(&mut self, gs: &GameState, dt: f32) {
        let a = 1.0 - (-10.0 * dt).exp();
        for (i, k) in gs.karts.iter().enumerate().take(8) {
            let target = if k.spin > 0.0 { 0.0 } else { k.drift_dir as f32 * 0.35 };
            self.vis[i] += (target - self.vis[i]) * a;
        }
    }

    pub fn project(&self, p: V2) -> (f32, f32) {
        let d = p - self.pos;
        let f = V2::from_angle(self.ang);
        (CX + d.dot(f.right()) * self.zoom, CY - d.dot(f) * self.zoom)
    }

    /// Chase-camera variant: focus stays on the kart, yaw lags a little behind its heading.
    pub fn follow3(&mut self, k: &Kart, dt: f32) {
        self.ang += wrap_angle(k.heading - self.ang) * (1.0 - (-4.5 * dt).exp());
        self.pos = k.pos;
    }

    /// Ease the camera toward a kart.
    pub fn follow(&mut self, k: &Kart, dt: f32) {
        let a = 1.0 - (-7.0 * dt).exp();
        self.ang += wrap_angle(k.heading - self.ang) * a;
        let want_zoom = 1.3 - 0.25 * (k.speed() / 520.0).min(1.0);
        self.zoom += (want_zoom - self.zoom) * (1.0 - (-3.0 * dt).exp());
        let target = k.pos + V2::from_angle(self.ang) * 40.0;
        self.pos = target;
    }
}

// ------------------------------------------------------------------ world

pub fn ground_color(tr: &Track, w: V2, stripe: i32) -> u32 {
    let d = tr.dist_at(w);
    let th = tr.theme;
    if d < HALF_W - KERB_W {
        if let Some((a, _)) = tr.pad_at(w) {
            return if ((a / 9.0).floor() as i32 + stripe / 2) & 1 == 0 { 0xFFC107 } else { 0xFF6F00 };
        }
        if let Some((a, b)) = tr.finish_at(w) {
            return if ((a / 6.0).floor() as i32 + (b / 6.0).floor() as i32) & 1 == 0 { 0xF5F5F5 } else { 0x212121 };
        }
        let i = tr.near_idx(w) as i32;
        if d < 2.5 && (i / 5) % 2 == 0 { 0xC8C8C8 } else { th.road }
    } else if d < HALF_W {
        let i = tr.near_idx(w);
        th.kerb[(i / 3) % 2]
    } else if d < HALF_W + GRASS_W {
        th.grass[(((w.x / 48.0).floor() as i32 + (w.y / 48.0).floor() as i32) & 1) as usize]
    } else if tr.in_clearing(w) {
        let dd = ((w.x / 32.0).floor() as i32 + (w.y / 32.0).floor() as i32) & 1;
        if dd == 0 { th.clearing } else { shade(th.clearing, 0.95) }
    } else {
        let h = ((w.x / 20.0).floor() as i32).wrapping_mul(73856093) ^ ((w.y / 20.0).floor() as i32).wrapping_mul(19349663);
        th.wall[(h & 3 != 0) as usize]
    }
}

fn terrain(fb: &mut Fb, tr: &Track, cam: &Cam, time: f32) {
    let f = V2::from_angle(cam.ang);
    let r = f.right();
    let inv = 1.0 / cam.zoom;
    let stripe = (time * 14.0) as i32;
    for y in 0..H {
        let fy = (CY - y as f32) * inv;
        let row = cam.pos + f * fy;
        for x in 0..W {
            let w = row + r * ((x as f32 - CX) * inv);
            fb.px[y * W + x] = ground_color(tr, w, stripe);
        }
    }
}

fn banana(fb: &mut Fb, x: f32, y: f32, r: f32) {
    let mut pts = Vec::new();
    for i in 0..=8 {
        let a = (200.0 + i as f32 * 17.5f32).to_radians();
        pts.push((x + a.cos() * r, y + r * 0.35 + a.sin() * r));
    }
    for i in (0..=8).rev() {
        let a = (200.0 + i as f32 * 17.5f32).to_radians();
        pts.push((x + a.cos() * r * 0.62, y + r * 0.35 + a.sin() * r * 0.62));
    }
    fb.poly(&pts, 0xFFEB3B);
    fb.circle(pts[0].0, pts[0].1, r * 0.13, 0x5D4037);
    fb.circle(pts[8].0, pts[8].1, r * 0.13, 0x5D4037);
}

fn shell(fb: &mut Fb, x: f32, y: f32, r: f32, c: u32) {
    fb.circle(x, y, r, 0xFFFFFF);
    fb.circle(x, y, r * 0.8, c);
    fb.circle(x - r * 0.25, y - r * 0.25, r * 0.25, shade(c, 1.6));
}

fn bolt(fb: &mut Fb, x: f32, y: f32, r: f32, c: u32) {
    let p = [(0.2, -1.0), (-0.6, 0.1), (-0.05, 0.1), (-0.3, 1.0), (0.6, -0.15), (0.05, -0.15), (0.5, -1.0)];
    let pts: Vec<_> = p.iter().map(|q| (x + q.0 * r, y + q.1 * r)).collect();
    fb.poly(&pts, c);
}

pub fn star(fb: &mut Fb, x: f32, y: f32, r: f32, c: u32) {
    let pts: Vec<_> = (0..10)
        .map(|i| {
            let a = (i as f32 * 36.0 - 90.0).to_radians();
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            (x + a.cos() * rr, y + a.sin() * rr)
        })
        .collect();
    fb.poly(&pts, c);
}

fn chevrons(fb: &mut Fb, x: f32, y: f32, r: f32, c: u32) {
    for k in 0..3 {
        let oy = y + r * 0.8 - k as f32 * r * 0.7;
        fb.poly(&[(x - r, oy), (x, oy - r * 0.6), (x + r, oy), (x + r, oy + r * 0.3), (x, oy - r * 0.3), (x - r, oy + r * 0.3)], c);
    }
}

pub fn item_box(fb: &mut Fb, x: f32, y: f32, r: f32, time: f32, fake: bool) {
    let a = time * 2.0;
    let pts: Vec<_> = (0..4)
        .map(|i| {
            let t = a + i as f32 * std::f32::consts::FRAC_PI_2;
            (x + t.cos() * r * 1.3, y + t.sin() * r * 1.3)
        })
        .collect();
    let base = if fake { 0xFF7043 } else { hue(time * 0.5) };
    fb.poly(&pts, base);
    let inner: Vec<_> = pts.iter().map(|p| (x + (p.0 - x) * 0.7, y + (p.1 - y) * 0.7)).collect();
    fb.poly(&inner, 0x263238);
    fb.text_center(x as i32, (y - r * 0.4) as i32, if fake { "!" } else { "?" }, ((r / 6.0) as i32).max(1), 0xFFFFFF);
}

/// Icon for a held item, centered at (x, y) with radius r.
pub fn item_icon(fb: &mut Fb, item: Item, x: f32, y: f32, r: f32, time: f32) {
    match item {
        Item::Peel | Item::TriplePeel => banana(fb, x, y - r * 0.2, r * 0.9),
        Item::Bouncer | Item::TripleBouncer => shell(fb, x, y, r * 0.8, 0x43A047),
        Item::Seeker => {
            shell(fb, x, y, r * 0.8, 0xE53935);
            for i in 0..6 {
                let a = i as f32 * 1.047;
                fb.circle(x + a.cos() * r * 0.85, y + a.sin() * r * 0.85, r * 0.15, 0xFFFFFF);
            }
        }
        Item::Nova => {
            fb.poly(&[(x - r, y - r * 0.6), (x - r * 0.4, y), (x - r, y + r * 0.2)], 0x81D4FA);
            fb.poly(&[(x + r, y - r * 0.6), (x + r * 0.4, y), (x + r, y + r * 0.2)], 0x81D4FA);
            shell(fb, x, y, r * 0.65, 0x1E88E5);
        }
        Item::Turbo | Item::TripleTurbo => chevrons(fb, x, y, r * 0.8, 0xFF7043),
        Item::Nitro => chevrons(fb, x, y, r * 0.8, 0xFFD700),
        Item::Giant => {
            fb.circle(x, y, r * 0.85, 0x8E24AA);
            fb.poly(&[(x, y - r * 0.6), (x + r * 0.5, y + r * 0.1), (x + r * 0.2, y + r * 0.1), (x + r * 0.2, y + r * 0.6), (x - r * 0.2, y + r * 0.6), (x - r * 0.2, y + r * 0.1), (x - r * 0.5, y + r * 0.1)], 0xFFFFFF);
        }
        Item::Star => star(fb, x, y, r, hue(time * 2.0)),
        Item::Bomb => {
            fb.circle(x, y + r * 0.1, r * 0.75, 0x212121);
            fb.circle(x - r * 0.25, y - r * 0.15, r * 0.18, 0x616161);
            fb.line((x + r * 0.3, y - r * 0.5), (x + r * 0.6, y - r * 0.9), 0xBCAAA4);
            fb.circle(x + r * 0.65, y - r * 0.95, r * 0.18, 0xFF9800);
        }
        Item::Decoy => item_box(fb, x, y, r * 0.65, time * 0.0, true),
        Item::Zap => bolt(fb, x, y, r, 0xFFEB3B),
        Item::Rocket => {
            fb.poly(&[(x, y - r), (x + r * 0.45, y - r * 0.2), (x + r * 0.45, y + r * 0.6), (x - r * 0.45, y + r * 0.6), (x - r * 0.45, y - r * 0.2)], 0xE53935);
            fb.poly(&[(x, y - r), (x + r * 0.3, y - r * 0.45), (x - r * 0.3, y - r * 0.45)], 0xFFFFFF);
            fb.circle(x, y, r * 0.2, 0x81D4FA);
            fb.poly(&[(x - r * 0.3, y + r * 0.6), (x, y + r * 1.1), (x + r * 0.3, y + r * 0.6)], 0xFF9800);
        }
        Item::Ink => {
            fb.circle(x, y, r * 0.75, 0x111111);
            for i in 0..5 {
                let a = i as f32 * 1.257;
                fb.circle(x + a.cos() * r * 0.8, y + a.sin() * r * 0.8, r * 0.22, 0x111111);
            }
            fb.circle(x - r * 0.25, y - r * 0.1, r * 0.17, 0xFFFFFF);
            fb.circle(x + r * 0.25, y - r * 0.1, r * 0.17, 0xFFFFFF);
        }
    }
}

fn draw_entity(fb: &mut Fb, cam: &Cam, e: &Ent, time: f32) {
    let (x, y) = cam.project(e.pos);
    if x < -80.0 || y < -80.0 || x > W as f32 + 80.0 || y > H as f32 + 80.0 {
        return;
    }
    draw_ent_at(fb, e, x, y, cam.zoom, time);
}

/// Draws a hazard at a screen position; `z` is pixels per world unit.
pub fn draw_ent_at(fb: &mut Fb, e: &Ent, x: f32, y: f32, z: f32, time: f32) {
    match e.kind {
        EntKind::Peel => banana(fb, x, y, 11.0 * z),
        EntKind::Decoy => item_box(fb, x, y, 12.0 * z, time, true),
        EntKind::Bouncer => shell(fb, x, y, 10.0 * z, 0x43A047),
        EntKind::Seeker => {
            shell(fb, x, y, 10.0 * z, 0xE53935);
            for i in 0..6 {
                let a = i as f32 * 1.047 + time * 4.0;
                fb.circle(x + a.cos() * 11.0 * z, y + a.sin() * 11.0 * z, 2.2 * z, 0xFFFFFF);
            }
        }
        EntKind::Nova => {
            fb.ring(x, y, 20.0 * z, 3.0, 0x81D4FA);
            item_icon(fb, Item::Nova, x, y, 18.0 * z, time);
        }
        EntKind::Bomb => {
            item_icon(fb, Item::Bomb, x, y, 14.0 * z, time);
            if (e.timer * 8.0) as i32 % 2 == 0 && e.timer < 1.5 {
                fb.circle(x, y, 14.0 * z, 0xFF1744);
            }
        }
        EntKind::Blast => {
            let prog = 1.0 - e.timer / 0.45;
            let rad = e.age * (0.3 + 0.7 * prog) * z;
            fb.circle(x, y, rad, 0xFF6F00);
            fb.circle(x, y, rad * 0.7, 0xFFC107);
            fb.circle(x, y, rad * 0.35, 0xFFFDE7);
        }
    }
}

fn draw_kart(fb: &mut Fb, cam: &Cam, k: &Kart, idx: usize, time: f32, label: bool) {
    if k.invuln > 0.0 && k.spin <= 0.0 && k.star <= 0.0 && k.giant <= 0.0 && k.rocket <= 0.0 && (time * 14.0) as i32 % 2 == 0 {
        return;
    }
    let sc = k.scale();
    let z = cam.zoom;
    let f = V2::from_angle(k.visual_heading());
    let r = f.right();
    let body = KART_COLORS[idx % 8];
    let to = |a: f32, b: f32| cam.project(k.pos + f * (a * sc) + r * (b * sc));
    let quad = |fb: &mut Fb, a0: f32, a1: f32, b0: f32, b1: f32, c: u32| {
        fb.poly(&[to(a0, b0), to(a1, b0), to(a1, b1), to(a0, b1)], c);
    };
    let (cx, cy) = to(0.0, 0.0);

    // shadow
    fb.circle(cx + 3.0, cy + 3.0, 16.0 * sc * z, 0x0F2A12);
    if k.star > 0.0 {
        fb.ring(cx, cy, 22.0 * sc * z, 4.0, hue(time * 3.0 + idx as f32 * 0.1));
    }
    if k.rocket > 0.0 {
        fb.ring(cx, cy, 26.0 * z, 4.0, if (time * 8.0) as i32 % 2 == 0 { 0xFF1744 } else { 0xFFFFFF });
    }
    if k.boost > 0.0 || k.rocket > 0.0 {
        let fl = 8.0 + 6.0 * ((time * 40.0).sin() * 0.5 + 0.5);
        fb.poly(&[to(-16.0, -5.0), to(-16.0 - fl, 0.0), to(-16.0, 5.0)], 0xFF9800);
        fb.poly(&[to(-16.0, -2.5), to(-16.0 - fl * 0.6, 0.0), to(-16.0, 2.5)], 0xFFF59D);
    }
    if k.drift_dir != 0 {
        let c = [0x9E9E9E, 0x29B6F6, 0xFF9800, 0xE040FB][k.drift_tier() as usize];
        for s in [-1.0f32, 1.0] {
            let (sx, sy) = to(-15.0, s * 11.0);
            let j = (time * 60.0 + s * 5.0).sin() * 3.0;
            fb.circle(sx + j, sy + j.abs(), 3.0 * z, c);
        }
    }
    // wheels
    for &(a, b) in &[(9.0, 11.0), (9.0, -11.0), (-9.0, 11.0), (-9.0, -11.0)] {
        quad(fb, a - 4.5, a + 4.5, b - 3.0, b + 3.0, 0x1A1A1A);
    }
    // chassis + nose + spoiler
    quad(fb, -14.0, 14.0, -8.5, 8.5, body);
    fb.poly(&[to(14.0, -8.5), to(21.0, 0.0), to(14.0, 8.5)], shade(body, 1.25));
    quad(fb, -17.0, -13.0, -10.0, 10.0, shade(body, 0.55));
    quad(fb, -2.0, 12.0, -1.5, 1.5, 0xFFFFFF);
    // driver
    let (hx, hy) = to(-3.0, 0.0);
    fb.circle(hx, hy, 6.5 * sc * z, shade(body, 0.6));
    fb.circle(hx, hy, 4.0 * sc * z, 0xFFE0B2);
    if k.rocket > 0.0 {
        fb.poly(&[to(22.0, 0.0), to(6.0, -9.0), to(-14.0, -9.0), to(-14.0, 9.0), to(6.0, 9.0)], 0xE53935);
        fb.poly(&[to(30.0, 0.0), to(20.0, -6.0), to(20.0, 6.0)], 0xFFFFFF);
        fb.circle(hx, hy, 4.0 * sc * z, 0x81D4FA);
    }
    if k.spin > 0.0 {
        for i in 0..3 {
            let a = time * 12.0 + i as f32 * 2.1;
            star(fb, cx + a.cos() * 20.0 * z, cy - 18.0 * z + a.sin() * 5.0 * z, 4.0 * z, 0xFFEB3B);
        }
    }
    if label {
        let name = format!("{} {}", k.place + 1, k.name);
        fb.text_center(cx as i32, (cy - 30.0 * sc * z) as i32, &name, 1, 0xFFFFFF);
    }
}

fn place_str(p: u8) -> String {
    let s = match p + 1 {
        1 => "st",
        2 => "nd",
        3 => "rd",
        _ => "th",
    };
    format!("{}{}", p + 1, s)
}

pub fn draw_game(fb: &mut Fb, tr: &Track, gs: &GameState, me: usize, cam: &Cam, time: f32) {
    terrain(fb, tr, cam, time);
    for (i, p) in tr.coin_pos.iter().enumerate() {
        if gs.coins & (1 << i) != 0 {
            let (x, y) = cam.project(*p);
            let w = ((time * 5.0 + i as f32).sin()).abs().max(0.35);
            fb.circle(x, y, 6.0 * cam.zoom, 0xFF8F00);
            fb.circle(x, y, 4.5 * cam.zoom * w, 0xFFD600);
        }
    }
    for (i, p) in tr.box_pos.iter().enumerate() {
        if gs.boxes & (1 << i) != 0 {
            let (x, y) = cam.project(*p);
            item_box(fb, x, y, 11.0 * cam.zoom, time + i as f32, false);
        }
    }
    for e in &gs.ents {
        draw_entity(fb, cam, e, time);
    }
    for (i, k) in gs.karts.iter().enumerate() {
        if i != me {
            draw_kart(fb, cam, k, i, time, true);
        }
    }
    if let Some(k) = gs.karts.get(me) {
        draw_kart(fb, cam, k, me, time, false);
        draw_hud(fb, tr, gs, me, k, time);
    }
}

pub fn draw_hud(fb: &mut Fb, tr: &Track, gs: &GameState, me: usize, k: &Kart, time: f32) {
    // ink overlay for blinded karts
    if k.inked > 0.0 {
        let a = (k.inked / 1.0).min(1.0) * 0.96;
        for i in 0..14 {
            let cx = (W as f32) * (((i * 37 + 11) % 100) as f32 / 100.0);
            let cy = (H as f32) * (((i * 53 + 7) % 100) as f32 / 100.0);
            let rr = 70.0 + ((i * 29) % 60) as f32;
            for y in ((cy - rr) as i32).max(0)..((cy + rr) as i32).min(H as i32) {
                for x in ((cx - rr) as i32).max(0)..((cx + rr) as i32).min(W as i32) {
                    let (dx, dy) = (x as f32 - cx, y as f32 - cy);
                    if dx * dx + dy * dy < rr * rr {
                        fb.blend(x, y, 0x0A0A0A, a);
                    }
                }
            }
        }
    }

    // place + lap + time
    let ps = place_str(k.place);
    fb.text_shadow(16, 12, &ps, 5, 0xFFEB3B);
    fb.text_shadow(16 + ps.chars().count() as i32 * 40 + 10, 30, &format!("/ {}", gs.karts.len()), 3, 0xFFFFFF);
    let lap = ((k.prog / tr.n as f32).floor() as i32 + 1).clamp(1, gs.laps as i32);
    fb.text_shadow(16, 68, &format!("LAP {}/{}", lap, gs.laps), 3, 0xFFFFFF);
    let t = if gs.phase == Phase::Racing || gs.phase == Phase::Results { gs.timer } else { 0.0 };
    let t = if gs.phase == Phase::Results { k.finish_time } else { t };
    fb.text_shadow(16, 100, &format!("{:02}:{:04.1}", (t / 60.0) as i32, t % 60.0), 2, 0xFFFFFF);

    // coins + speed
    fb.circle(28.0, H as f32 - 60.0, 9.0, 0xFF8F00);
    fb.circle(28.0, H as f32 - 60.0, 6.5, 0xFFD600);
    fb.text_shadow(46, H as i32 - 67, &format!("{}/10", k.coins), 2, 0xFFFFFF);
    let sp = (k.speed() / 520.0).min(1.0);
    fb.rect(16, H as i32 - 36, 204, 16, 0x202020);
    let col = if k.boost > 0.0 { 0xFF9800 } else { 0x66BB6A };
    fb.rect(18, H as i32 - 34, (200.0 * sp) as i32, 12, col);
    fb.text_shadow(16, H as i32 - 20, &format!("{} km/h", (k.speed() * 0.4) as i32), 1, 0xFFFFFF);

    // status effects
    let mut fx = Vec::new();
    if k.star > 0.0 { fx.push(("STAR", k.star, 0xFFEB3B)); }
    if k.giant > 0.0 { fx.push(("GIANT", k.giant, 0xCE93D8)); }
    if k.rocket > 0.0 { fx.push(("ROCKET", k.rocket, 0xEF5350)); }
    if k.shrunk > 0.0 { fx.push(("SHRUNK", k.shrunk, 0x90CAF9)); }
    if k.inked > 0.0 { fx.push(("INKED", k.inked, 0xBDBDBD)); }
    for (i, (n, t, c)) in fx.iter().enumerate() {
        fb.text_shadow(16, 134 + i as i32 * 16, &format!("{n} {:.0}s", t.ceil()), 2, *c);
    }

    // item slots
    let bx = W as i32 / 2 - 50;
    fb.rrect(bx - 8, 8, 116, 78, 16, 0x000000, 0.45);
    slot(fb, k.slots[0], bx as f32 + 30.0, 36.0, 24.0, time);
    fb.rrect(bx + 66, 30, 36, 36, 10, 0x000000, 0.4);
    slot(fb, k.slots[1], bx as f32 + 84.0, 48.0, 13.0, time);
    fb.text_center(W as i32 / 2, 92, "L-click use (hold Space: throw backward)   R-click swap", 1, 0xDDDDDD);

    minimap(fb, tr, gs, me);

    // wrong-way warning
    let along = V2::from_angle(k.heading).dot(tr.tangent(k.run));
    if gs.phase == Phase::Racing && !k.finished && along < -0.25 && (time * 3.0) as i32 % 2 == 0 {
        let cx = W as f32 / 2.0;
        fb.poly(&[(cx - 40.0, 150.0), (cx + 40.0, 150.0), (cx, 205.0)], 0xE53935);
        fb.text_center(cx as i32, 214, "WRONG WAY", 4, 0xFF5252);
    }

    // countdown
    if gs.phase == Phase::Countdown {
        let n = (gs.timer - 1.0).ceil() as i32;
        let (s, c) = if n >= 1 { (n.to_string(), 0xFFEB3B) } else { ("GO!".to_string(), 0x66FF66) };
        fb.text_center(W as i32 / 2, H as i32 / 2 - 100, &s, 10, c);
    } else if gs.phase == Phase::Racing && gs.timer < 1.2 {
        fb.text_center(W as i32 / 2, H as i32 / 2 - 100, "GO!", 10, 0x66FF66);
    }
    if k.finished && gs.phase == Phase::Racing {
        fb.text_center(W as i32 / 2, H as i32 / 2 - 60, &format!("FINISHED {}", place_str(k.place)), 5, 0xFFEB3B);
    }
    let _ = me;
}

fn slot(fb: &mut Fb, s: (u8, u8), x: f32, y: f32, r: f32, time: f32) {
    if let Some(item) = Item::from_u8(s.0) {
        item_icon(fb, item, x, y, r, time);
        if r > 20.0 {
            fb.text_center(x as i32, (y + r + 6.0) as i32, item.name(), 1, 0xFFFFFF);
        }
        if s.1 > 1 {
            fb.text_shadow((x + r * 0.4) as i32, (y + r * 0.3) as i32, &format!("x{}", s.1), if r > 20.0 { 2 } else { 1 }, 0xFFFFFF);
        }
    }
}

fn minimap(fb: &mut Fb, tr: &Track, gs: &GameState, me: usize) {
    let (mw, mh) = (150.0, 116.0);
    let (ox, oy) = (W as f32 - mw - 14.0, 14.0);
    fb.rrect(ox as i32 - 6, oy as i32 - 6, mw as i32 + 12, mh as i32 + 12, 14, 0x000000, 0.45);
    let sx = mw / (tr.max.x - tr.min.x);
    let sy = mh / (tr.max.y - tr.min.y);
    let s = sx.min(sy);
    let m = |p: V2| (ox + (p.x - tr.min.x) * s, oy + (p.y - tr.min.y) * s);
    for i in (0..tr.n).step_by(6) {
        let (a, b) = (m(tr.pts[i]), m(tr.pts[(i + 6) % tr.n]));
        fb.line(a, b, 0xCFD8DC);
        fb.line((a.0 + 1.0, a.1), (b.0 + 1.0, b.1), 0xCFD8DC);
    }
    let (s0, s1) = (m(tr.pts[0]), m(tr.pts[0] + tr.tangent(0).right() * 22.0));
    fb.line(s0, s1, 0xFFFFFF);
    for (i, k) in gs.karts.iter().enumerate() {
        let (x, y) = m(k.pos);
        fb.circle(x, y, if i == me { 4.0 } else { 3.0 }, if i == me { 0xFFFFFF } else { 0x000000 });
        fb.circle(x, y, if i == me { 2.8 } else { 2.0 }, KART_COLORS[i % 8]);
    }
}

// ------------------------------------------------------------------ screens

fn backdrop(fb: &mut Fb, time: f32) {
    fb.clear(0x14202B);
    for i in 0..12 {
        let y = ((i as f32 * 60.0 + time * 40.0) % 600.0) as i32 - 40;
        fb.rect(0, y, W as i32, 18, 0x182A38);
    }
}

pub fn draw_title(fb: &mut Fb, name: &str, editing: bool, mode3d: bool, msg: &str, time: f32) {
    backdrop(fb, time);
    fb.text_center(W as i32 / 2, 50, "Z-CART", 10, 0xFFEB3B);
    fb.text_center(W as i32 / 2, 150, "LAN kart racing - peer hosted, IPv6 link-local", 1, 0x9FB3C8);
    let cursor = if editing && (time * 2.0) as i32 % 2 == 0 { "_" } else { "" };
    let lines = [
        ("H", "Host a game"),
        ("J", "Join a game on the LAN"),
        ("V", if mode3d { "View: 3D chase" } else { "View: 2D top-down" }),
        ("N", "Change name"),
        ("Q", "Quit"),
    ];
    for (i, (k, v)) in lines.iter().enumerate() {
        let y = 205 + i as i32 * 32;
        fb.text_shadow(300, y, &format!("[{k}]"), 2, 0x66BB6A);
        fb.text_shadow(370, y, v, 2, 0xFFFFFF);
    }
    fb.text_shadow(300, 364, &format!("Name: {name}{cursor}"), 2, if editing { 0xFFEB3B } else { 0xCFD8DC });
    fb.text_center(W as i32 / 2, 404, "DRIVE: WASD / arrows   DRIFT: hold Shift   V: switch view", 1, 0xCFD8DC);
    fb.text_center(W as i32 / 2, 428, "USE ITEM: left click (hold Space: throw backward)   SWAP SLOT: right click", 1, 0xCFD8DC);
    fb.text_center(W as i32 / 2, 466, "Items: Peel Bouncer Seeker Nova Turbo Giant Star Bomb Decoy Zap Rocket Ink Nitro", 1, 0x9FB3C8);
    if !msg.is_empty() {
        fb.text_center(W as i32 / 2, 496, msg, 2, 0xFF8A80);
    }
}

pub fn draw_browse(fb: &mut Fb, hosts: &[crate::net::HostInfo], sel: usize, msg: &str, time: f32) {
    backdrop(fb, time);
    fb.text_center(W as i32 / 2, 30, "GAMES ON YOUR LAN", 4, 0xFFEB3B);
    if hosts.is_empty() {
        let dots = ".".repeat(1 + (time * 2.0) as usize % 3);
        fb.text_center(W as i32 / 2, 200, &format!("Searching{dots}"), 3, 0xCFD8DC);
        fb.text_center(W as i32 / 2, 240, "Hosts appear here automatically (IPv6 link-local multicast)", 1, 0x9FB3C8);
    }
    for (i, h) in hosts.iter().enumerate() {
        let y = 110 + i as i32 * 40;
        if i == sel {
            fb.rect_alpha(120, y - 8, 720, 36, 0x66BB6A, 0.35);
        }
        let status = match h.phase {
            Phase::Lobby if h.players >= h.max => "FULL",
            Phase::Lobby => "OPEN",
            _ => "IN RACE",
        };
        let col = if h.joinable() { 0xFFFFFF } else { 0x78909C };
        fb.text_shadow(136, y, &h.name, 3, col);
        fb.text_shadow(520, y, &format!("{}/{}", h.players, h.max), 3, col);
        fb.text_shadow(640, y, status, 3, if h.joinable() { 0x66BB6A } else { 0xEF9A9A });
        fb.text(136, y + 24, &h.addr.to_string(), 1, 0x78909C);
    }
    fb.text_center(W as i32 / 2, H as i32 - 60, "Up/Down select   Enter join   R rescan   Esc back", 2, 0xCFD8DC);
    if !msg.is_empty() {
        fb.text_center(W as i32 / 2, H as i32 - 100, msg, 2, 0xFF8A80);
    }
}

pub fn draw_lobby(fb: &mut Fb, gs: &GameState, me: usize, is_host: bool, port: Option<u16>, track_label: &str, time: f32) {
    backdrop(fb, time);
    fb.text_center(W as i32 / 2, 30, "LOBBY", 6, 0xFFEB3B);
    if let Some(p) = port {
        fb.text_center(W as i32 / 2, 90, &format!("Hosting on UDP port {p} - friends on your LAN will see this game under Join"), 1, 0x9FB3C8);
    }
    for (i, k) in gs.karts.iter().enumerate() {
        let y = 122 + i as i32 * 32;
        fb.rect(260, y, 24, 24, KART_COLORS[i % 8]);
        let tag = if i == 0 { " (host)" } else { "" };
        let you = if i == me { "  <- you" } else { "" };
        fb.text_shadow(300, y + 2, &format!("{}{tag}{you}", k.name), 2, 0xFFFFFF);
    }
    let bots = (gs.bots as usize).min(MAX_KARTS - gs.karts.len());
    fb.text_shadow(680, 126, &format!("+ {bots} bot(s)"), 2, 0x90A4AE);
    if is_host {
        fb.text_center(W as i32 / 2, H as i32 - 134, &format!("Track: {track_label}  [T]"), 2, 0xFFEB3B);
        fb.text_center(W as i32 / 2, H as i32 - 106, &format!("Bots: {}  [B]     Laps: {}  [L]", gs.bots, gs.laps), 2, 0xFFFFFF);
        fb.text_center(W as i32 / 2, H as i32 - 72, "ENTER: start race     ESC: close lobby", 2, 0x66BB6A);
    } else {
        fb.text_center(W as i32 / 2, H as i32 - 134, &format!("Track: {track_label}"), 2, 0xFFEB3B);
        fb.text_center(W as i32 / 2, H as i32 - 106, &format!("Bots: {}   Laps: {}", gs.bots, gs.laps), 2, 0xFFFFFF);
        fb.text_center(W as i32 / 2, H as i32 - 72, "Waiting for the host to start...   ESC: leave", 2, 0x66BB6A);
    }
}

pub fn draw_results(fb: &mut Fb, gs: &GameState, me: usize, is_host: bool) {
    fb.rrect(200, 50, 560, 440, 22, 0x000000, 0.78);
    fb.text_center(W as i32 / 2, 66, "RESULTS", 5, 0xFFEB3B);
    let mut order: Vec<usize> = (0..gs.karts.len()).collect();
    order.sort_by_key(|&i| gs.karts[i].place);
    for (row, &i) in order.iter().enumerate() {
        let k = &gs.karts[i];
        let y = 130 + row as i32 * 38;
        let c = if i == me { 0xFFEB3B } else { 0xFFFFFF };
        fb.text_shadow(220, y, &place_str(row as u8), 2, c);
        fb.rect(290, y - 2, 20, 20, KART_COLORS[i % 8]);
        fb.text_shadow(320, y, &k.name, 2, c);
        fb.circle(600.0, y as f32 + 8.0, 7.0, 0xFF8F00);
        fb.circle(600.0, y as f32 + 8.0, 5.0, 0xFFD600);
        fb.text_shadow(614, y, &format!("{}", k.coins), 2, c);
        let t = if k.finished { format!("{:02}:{:05.2}", (k.finish_time / 60.0) as i32, k.finish_time % 60.0) } else { "DNF".into() };
        fb.text_shadow(680, y, &t, 2, c);
    }
    let hint = if is_host { "ENTER: back to lobby    ESC: quit" } else { "Waiting for host...    ESC: leave" };
    fb.text_center(W as i32 / 2, 450, hint, 2, 0x66BB6A);
}

pub fn draw_connecting(fb: &mut Fb, time: f32) {
    backdrop(fb, time);
    fb.text_center(W as i32 / 2, H as i32 / 2 - 20, "Connecting...", 4, 0xFFFFFF);
    fb.text_center(W as i32 / 2, H as i32 / 2 + 40, "ESC: cancel", 2, 0xCFD8DC);
}
