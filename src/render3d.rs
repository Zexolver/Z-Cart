//! Chase-camera 3D view. The ground is a true perspective projection of the
//! track plane; karts are real 3D boxes; items, coins and trees are projected
//! sprites. Everything is depth-sorted (painter's algorithm) into a software framebuffer.

use crate::math::*;
use crate::render::*;
use crate::sim::*;
use crate::track::*;

const FOCAL: f32 = 420.0;
const HORIZON: f32 = H as f32 * 0.36;
const EYE_H: f32 = 80.0;
const BACK: f32 = 170.0;
const FOG: u32 = 0xBFE3F5;

struct Pr {
    eye: V2,
    f: V2,
    r: V2,
    ang: f32,
}

impl Pr {
    fn new(cam: &Cam) -> Pr {
        let f = V2::from_angle(cam.ang);
        Pr { eye: cam.pos - f * BACK, f, r: f.right(), ang: cam.ang }
    }

    fn depth(&self, p: V2) -> f32 {
        (p - self.eye).dot(self.f)
    }

    /// World point at height `z` -> (screen x, screen y, pixels per world unit).
    fn p(&self, p: V2, z: f32) -> Option<(f32, f32, f32)> {
        let rel = p - self.eye;
        let d = rel.dot(self.f);
        if d < 12.0 {
            return None;
        }
        let s = FOCAL / d;
        Some((W as f32 / 2.0 + rel.dot(self.r) * s, HORIZON + (EYE_H - z) * s, s))
    }
}

fn sky_and_ground(fb: &mut Fb, tr: &Track, pr: &Pr, time: f32) {
    for y in 0..HORIZON as usize {
        let t = y as f32 / HORIZON;
        let c = mix(0x3F8FD8, FOG, t);
        fb.px[y * W..(y + 1) * W].iter_mut().for_each(|p| *p = c);
    }
    // distant hills, parallax with camera yaw
    for x in 0..W {
        let a = pr.ang + ((x as f32 - W as f32 / 2.0) / FOCAL).atan();
        let h = 26.0 + 16.0 * (a * 2.3 + 0.5).sin() + 9.0 * (a * 6.1).sin();
        let top = (HORIZON - h).max(0.0) as usize;
        for y in top..HORIZON as usize {
            fb.px[y * W + x] = 0x4F8A73;
        }
    }
    let stripe = (time * 14.0) as i32;
    for y in HORIZON as usize..H {
        let dy = y as f32 + 0.5 - HORIZON;
        let d = EYE_H * FOCAL / dy;
        let row = pr.eye + pr.f * d;
        let step = d / FOCAL;
        let fog = ((d - 1500.0) / 2500.0).clamp(0.0, 1.0);
        for x in 0..W {
            let w = row + pr.r * ((x as f32 + 0.5 - W as f32 / 2.0) * step);
            let c = ground_color(tr, w, stripe);
            fb.px[y * W + x] = if fog > 0.0 { mix(c, FOG, fog) } else { c };
        }
    }
}

/// An oriented box: local ranges [a0,a1] forward, [b0,b1] right, [z0,z1] up, all times `sc`.
fn cuboid(fb: &mut Fb, pr: &Pr, pos: V2, yaw: f32, sc: f32, bx: [f32; 6], color: u32) {
    let f = V2::from_angle(yaw);
    let r = f.right();
    let mut w3 = [(V2::ZERO, 0.0f32); 8];
    let mut sp = [(0.0f32, 0.0f32); 8];
    for i in 0..8 {
        let a = if i & 1 != 0 { bx[1] } else { bx[0] } * sc;
        let b = if i & 2 != 0 { bx[3] } else { bx[2] } * sc;
        let z = if i & 4 != 0 { bx[5] } else { bx[4] } * sc;
        let wp = pos + f * a + r * b;
        w3[i] = (wp, z);
        match pr.p(wp, z) {
            Some((x, y, _)) => sp[i] = (x, y),
            None => return,
        }
    }
    // corner indices, outward local normal
    const FACES: [([usize; 4], (f32, f32, f32)); 5] = [
        ([1, 3, 7, 5], (1.0, 0.0, 0.0)),
        ([0, 4, 6, 2], (-1.0, 0.0, 0.0)),
        ([2, 6, 7, 3], (0.0, 1.0, 0.0)),
        ([0, 1, 5, 4], (0.0, -1.0, 0.0)),
        ([4, 5, 7, 6], (0.0, 0.0, 1.0)),
    ];
    for (idx, n) in FACES.iter() {
        let nxy = f * n.0 + r * n.1;
        let c = idx.iter().fold((V2::ZERO, 0.0), |acc, &i| (acc.0 + w3[i].0 * 0.25, acc.1 + w3[i].1 * 0.25));
        let to_eye = (pr.eye - c.0, EYE_H - c.1);
        if nxy.dot(to_eye.0) + n.2 * to_eye.1 <= 0.0 {
            continue;
        }
        let light = if n.2 > 0.5 { 1.05 } else { 0.68 + 0.22 * (nxy.x * 0.6 - nxy.y * 0.5).clamp(-1.0, 1.0) };
        let pts: Vec<(f32, f32)> = idx.iter().map(|&i| sp[i]).collect();
        fb.poly(&pts, shade(color, light));
    }
}

fn draw_kart3d(fb: &mut Fb, pr: &Pr, k: &Kart, idx: usize, time: f32) {
    if k.invuln > 0.0 && k.spin <= 0.0 && k.star <= 0.0 && k.giant <= 0.0 && k.rocket <= 0.0 && (time * 14.0) as i32 % 2 == 0 {
        return;
    }
    let sc = k.scale();
    let body = KART_COLORS[idx % 8];
    let drift_tilt = k.drift_dir as f32 * 0.32;
    let yaw = k.visual_heading() + drift_tilt;
    let f = V2::from_angle(yaw);
    let r = f.right();
    let at = |a: f32, b: f32| k.pos + f * (a * sc) + r * (b * sc);

    let mut parts: Vec<([f32; 6], u32)> = vec![
        ([4.5, 13.5, 8.0, 14.0, 0.0, 9.0], 0x1A1A1A),
        ([4.5, 13.5, -14.0, -8.0, 0.0, 9.0], 0x1A1A1A),
        ([-13.5, -4.5, 8.0, 14.0, 0.0, 9.0], 0x1A1A1A),
        ([-13.5, -4.5, -14.0, -8.0, 0.0, 9.0], 0x1A1A1A),
        ([-14.0, 14.0, -8.5, 8.5, 3.0, 10.0], body),
        ([14.0, 22.0, -5.0, 5.0, 3.0, 8.0], shade(body, 1.25)),
        ([-18.0, -13.0, -11.0, 11.0, 10.0, 17.0], shade(body, 0.55)),
        ([-6.0, 2.0, -4.0, 4.0, 10.0, 17.0], shade(body, 0.6)),
    ];
    if k.rocket > 0.0 {
        parts.push(([-14.0, 24.0, -9.0, 9.0, 3.0, 14.0], 0xE53935));
        parts.push(([24.0, 32.0, -4.0, 4.0, 5.0, 10.0], 0xFFFFFF));
    }
    parts.sort_by(|a, b| {
        let da = pr.depth(at((a.0[0] + a.0[1]) / 2.0, (a.0[2] + a.0[3]) / 2.0));
        let db = pr.depth(at((b.0[0] + b.0[1]) / 2.0, (b.0[2] + b.0[3]) / 2.0));
        db.partial_cmp(&da).unwrap()
    });

    // effects behind the kart
    if k.boost > 0.0 || k.rocket > 0.0 {
        let fl = 10.0 + 8.0 * ((time * 40.0).sin() * 0.5 + 0.5) + if k.combo > 0.0 { 10.0 } else { 0.0 };
        if let (Some(a), Some(b), Some(c)) = (pr.p(at(-18.0, -5.0), 7.0 * sc), pr.p(at(-18.0 - fl, 0.0), 7.0 * sc), pr.p(at(-18.0, 5.0), 7.0 * sc)) {
            fb.poly(&[(a.0, a.1), (b.0, b.1), (c.0, c.1)], 0xFF9800);
        }
    }
    if k.drift_dir != 0 {
        let c = [0x9E9E9E, 0x29B6F6, 0xFF9800, 0xE040FB][k.drift_tier() as usize];
        for s in [-1.0f32, 1.0] {
            if let Some((x, y, sc2)) = pr.p(at(-15.0, s * 12.0), 3.0) {
                let j = (time * 60.0 + s * 5.0).sin() * 3.0;
                fb.circle(x + j, y + j.abs(), 3.0 * sc2 + 1.0, c);
            }
        }
    }
    for (b, c) in &parts {
        cuboid(fb, pr, k.pos, yaw, sc, *b, *c);
    }
    // head
    if let Some((x, y, s)) = pr.p(k.pos, 22.0 * sc) {
        fb.circle(x, y, 6.5 * sc * s, shade(body, 0.6));
        fb.circle(x, y, 4.2 * sc * s, if k.rocket > 0.0 { 0x81D4FA } else { 0xFFE0B2 });
    }
    if k.star > 0.0 {
        if let Some((x, y, s)) = pr.p(k.pos, 12.0 * sc) {
            fb.ring(x, y, 26.0 * sc * s, 4.0, hue(time * 3.0 + idx as f32 * 0.1));
        }
    }
    if k.spin > 0.0 {
        for i in 0..3 {
            let a = time * 12.0 + i as f32 * 2.1;
            if let Some((x, y, s)) = pr.p(k.pos + V2::from_angle(a) * (16.0 * sc), 34.0 * sc) {
                star(fb, x, y, 5.0 * s, 0xFFEB3B);
            }
        }
    }
}

fn shadow(fb: &mut Fb, pr: &Pr, k: &Kart) {
    let f = V2::from_angle(k.visual_heading());
    let r = f.right();
    let sc = k.scale();
    let pts: Option<Vec<(f32, f32)>> = [(-17.0, -14.0), (23.0, -14.0), (23.0, 14.0), (-17.0, 14.0)]
        .iter()
        .map(|&(a, b)| pr.p(k.pos + f * (a * sc) + r * (b * sc), 0.0).map(|p| (p.0, p.1)))
        .collect();
    if let Some(p) = pts {
        fb.poly(&p, 0x0D260F);
    }
}

enum Obj {
    Kart(usize),
    Ent(usize),
    Box(usize),
    Coin(usize),
    Tree(usize),
}

fn draw_tree(fb: &mut Fb, pr: &Pr, p: V2) {
    let Some((x, y, s)) = pr.p(p, 0.0) else { return };
    fb.rect((x - 5.0 * s) as i32, (y - 26.0 * s) as i32, (10.0 * s).ceil() as i32, (26.0 * s).ceil() as i32, 0x5D4037);
    fb.poly(&[(x - 34.0 * s, y - 20.0 * s), (x + 34.0 * s, y - 20.0 * s), (x, y - 78.0 * s)], 0x1B5E20);
    fb.poly(&[(x - 26.0 * s, y - 50.0 * s), (x + 26.0 * s, y - 50.0 * s), (x, y - 108.0 * s)], 0x2E7D32);
}

pub fn draw_game3d(fb: &mut Fb, tr: &Track, gs: &GameState, me: usize, cam: &Cam, time: f32) {
    let pr = Pr::new(cam);
    sky_and_ground(fb, tr, &pr, time);
    for k in &gs.karts {
        shadow(fb, &pr, k);
    }

    let visible = |p: V2, margin: f32| -> Option<f32> {
        let d = pr.depth(p);
        if d < 20.0 || d > 2600.0 {
            return None;
        }
        let lat = (p - pr.eye).dot(pr.r);
        (lat.abs() <= d * 1.2 + margin).then_some(d)
    };
    let mut objs: Vec<(f32, Obj)> = Vec::new();
    for (i, p) in tr.trees.iter().enumerate() {
        if let Some(d) = visible(*p, 80.0) {
            objs.push((d, Obj::Tree(i)));
        }
    }
    for (i, p) in tr.coin_pos.iter().enumerate() {
        if gs.coins & (1 << i) != 0 {
            if let Some(d) = visible(*p, 20.0) {
                objs.push((d, Obj::Coin(i)));
            }
        }
    }
    for (i, p) in tr.box_pos.iter().enumerate() {
        if gs.boxes & (1 << i) != 0 {
            if let Some(d) = visible(*p, 30.0) {
                objs.push((d, Obj::Box(i)));
            }
        }
    }
    for (i, e) in gs.ents.iter().enumerate() {
        objs.push((pr.depth(e.pos), Obj::Ent(i)));
    }
    for (i, k) in gs.karts.iter().enumerate() {
        objs.push((pr.depth(k.pos), Obj::Kart(i)));
    }
    objs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    for (_, o) in &objs {
        match *o {
            Obj::Tree(i) => draw_tree(fb, &pr, tr.trees[i]),
            Obj::Coin(i) => {
                if let Some((x, y, s)) = pr.p(tr.coin_pos[i], 10.0) {
                    let w = ((time * 4.0 + i as f32).cos()).abs().max(0.25);
                    let pts: Vec<(f32, f32)> = (0..14)
                        .map(|j| {
                            let a = j as f32 / 14.0 * std::f32::consts::TAU;
                            (x + a.cos() * 6.0 * s * w, y + a.sin() * 6.0 * s)
                        })
                        .collect();
                    fb.poly(&pts, 0xFFD600);
                    fb.ring(x, y, 5.0 * s * (0.5 + w * 0.5), 1.5, 0xFF8F00);
                }
            }
            Obj::Box(i) => {
                let p = tr.box_pos[i];
                let bob = (time * 3.0 + i as f32).sin() * 3.0;
                let bx = [-11.0, 11.0, -11.0, 11.0, 6.0 + bob, 28.0 + bob];
                cuboid(fb, &pr, p, time * 2.0 + i as f32, 1.0, bx, hue(time * 0.5 + i as f32 * 0.07));
                if let Some((x, y, s)) = pr.p(p, 17.0 + bob) {
                    let sc = ((s * 1.6).round() as i32).clamp(1, 4);
                    fb.text_center(x as i32, y as i32 - 4 * sc, "?", sc, 0xFFFFFF);
                }
            }
            Obj::Ent(i) => {
                let e = &gs.ents[i];
                if let Some((x, y, s)) = pr.p(e.pos, if e.kind == EntKind::Blast { 18.0 } else { 9.0 }) {
                    draw_ent_at(fb, e, x, y, s, time);
                    if e.kind == EntKind::Decoy {
                        let sc = ((s * 1.6).round() as i32).clamp(1, 4);
                        let _ = sc;
                    }
                }
            }
            Obj::Kart(i) => draw_kart3d(fb, &pr, &gs.karts[i], i, time),
        }
    }
    for (i, k) in gs.karts.iter().enumerate() {
        if i != me && pr.depth(k.pos) < 1300.0 {
            if let Some((x, y, _)) = pr.p(k.pos, 34.0 * k.scale()) {
                fb.text_center(x as i32, y as i32 - 10, &format!("{} {}", k.place + 1, k.name), 1, 0xFFFFFF);
            }
        }
    }
    if let Some(k) = gs.karts.get(me) {
        draw_hud(fb, tr, gs, me, k, time);
    }
}
