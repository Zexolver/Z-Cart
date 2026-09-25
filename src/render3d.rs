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
    let fog_col = tr.theme.fog;
    for y in 0..HORIZON as usize {
        let t = y as f32 / HORIZON;
        let c = mix(tr.theme.sky, fog_col, t);
        fb.px[y * W..(y + 1) * W].iter_mut().for_each(|p| *p = c);
    }
    if tr.theme.scenery == Scenery::Night {
        for i in 0..90usize {
            let a = (i as f32 * 2.399) % std::f32::consts::TAU;
            let da = wrap_angle(a - pr.ang);
            if da.abs() < 1.1 {
                let x = W as f32 / 2.0 + da.tan() * FOCAL;
                let y = (i * 37 % 150) as f32 + 6.0;
                fb.put(x as i32, y as i32, 0xFFFFFF);
            }
        }
    }
    // distant hills, parallax with camera yaw
    for x in 0..W {
        let a = pr.ang + ((x as f32 - W as f32 / 2.0) / FOCAL).atan();
        let h = 26.0 + 16.0 * (a * 2.3 + 0.5).sin() + 9.0 * (a * 6.1).sin();
        let top = (HORIZON - h).max(0.0) as usize;
        for y in top..HORIZON as usize {
            fb.px[y * W + x] = tr.theme.hills;
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
            fb.px[y * W + x] = if fog > 0.0 { mix(c, fog_col, fog) } else { c };
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

fn draw_kart3d(fb: &mut Fb, pr: &Pr, k: &Kart, idx: usize, lean: f32, time: f32) {
    if k.invuln > 0.0 && k.spin <= 0.0 && k.star <= 0.0 && k.giant <= 0.0 && k.rocket <= 0.0 && (time * 14.0) as i32 % 2 == 0 {
        return;
    }
    let sc = k.scale();
    let body = KART_COLORS[idx % 8];
    let yaw = k.visual_heading() + lean;
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
        parts.push(([-16.0, 26.0, -11.0, 11.0, 0.0, 22.0], 0xE53935));
        parts.push(([26.0, 36.0, -6.0, 6.0, 4.0, 16.0], 0xFFFFFF));
        parts.push(([-16.0, -4.0, 11.0, 22.0, 2.0, 12.0], 0xB71C1C));
        parts.push(([-16.0, -4.0, -22.0, -11.0, 2.0, 12.0], 0xB71C1C));
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
    if k.rocket > 0.0 {
        if let Some((x, y, s)) = pr.p(k.pos, 12.0) {
            fb.ring(x, y, 34.0 * s, 4.0, if (time * 8.0) as i32 % 2 == 0 { 0xFF1744 } else { 0xFFFFFF });
        }
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
    Prop(usize),
}

fn glow(fb: &mut Fb, x: f32, y: f32, r: f32, c: u32, a: f32) {
    for yy in (y - r) as i32..=(y + r) as i32 {
        for xx in (x - r) as i32..=(x + r) as i32 {
            let d = ((xx as f32 - x).powi(2) + (yy as f32 - y).powi(2)).sqrt() / r;
            if d < 1.0 {
                fb.blend(xx, yy, c, a * (1.0 - d));
            }
        }
    }
}

fn draw_prop(fb: &mut Fb, pr: &Pr, tr: &Track, p: &Prop, time: f32) {
    let sc = p.scale;
    let th = tr.theme;
    match p.kind {
        PropKind::Arch => {
            let f = V2::from_angle(p.yaw);
            let r = f.right();
            for side in [-1.0f32, 1.0] {
                cuboid(fb, pr, p.pos + r * (side * (HALF_W + 8.0)), p.yaw, 1.0, [-5.0, 5.0, -5.0, 5.0, 0.0, 84.0], 0x424242);
            }
            cuboid(fb, pr, p.pos, p.yaw, 1.0, [-4.0, 4.0, -(HALF_W + 13.0), HALF_W + 13.0, 70.0, 90.0], 0xD32F2F);
            cuboid(fb, pr, p.pos + f * 4.5, p.yaw, 1.0, [-1.0, 1.0, -(HALF_W - 4.0), HALF_W - 4.0, 74.0, 86.0], 0xFAFAFA);
            return;
        }
        PropKind::Table => {
            let parts: [([f32; 6], u32); 6] = [
                ([-18.0, 18.0, -10.0, 10.0, 18.0, 22.0], 0x9C6B45),
                ([-18.0, 18.0, -3.0, 3.0, 22.0, 22.5], 0xD32F2F),
                ([-16.0, 16.0, -20.0, -12.0, 9.0, 12.0], 0x8D6E63),
                ([-16.0, 16.0, 12.0, 20.0, 9.0, 12.0], 0x8D6E63),
                ([-15.0, -12.0, -8.0, 8.0, 0.0, 18.0], 0x6D4C41),
                ([12.0, 15.0, -8.0, 8.0, 0.0, 18.0], 0x6D4C41),
            ];
            for (b, c) in parts.iter() {
                cuboid(fb, pr, p.pos, p.yaw, sc, *b, *c);
            }
            return;
        }
        _ => {}
    }
    let Some((x, y, s0)) = pr.p(p.pos, 0.0) else { return };
    let s = s0 * sc;
    match p.kind {
        PropKind::Pine | PropKind::SnowPine => {
            let (c1, c2) = match (p.kind, th.scenery) {
                (PropKind::SnowPine, _) => (0x2F5E52, 0x3E7A69),
                (_, Scenery::Night) => (0x0F3B1E, 0x175428),
                _ => (0x1B5E20, 0x2E7D32),
            };
            fb.rect((x - 5.0 * s) as i32, (y - 26.0 * s) as i32, (10.0 * s).ceil() as i32, (26.0 * s).ceil() as i32, 0x5D4037);
            fb.poly(&[(x - 34.0 * s, y - 20.0 * s), (x + 34.0 * s, y - 20.0 * s), (x, y - 78.0 * s)], c1);
            fb.poly(&[(x - 26.0 * s, y - 50.0 * s), (x + 26.0 * s, y - 50.0 * s), (x, y - 108.0 * s)], c2);
            if p.kind == PropKind::SnowPine {
                fb.poly(&[(x - 11.0 * s, y - 92.0 * s), (x + 11.0 * s, y - 92.0 * s), (x, y - 108.0 * s)], 0xFFFFFF);
                fb.poly(&[(x - 19.0 * s, y - 66.0 * s), (x + 19.0 * s, y - 66.0 * s), (x, y - 78.0 * s)], 0xF4F8FB);
            }
        }
        PropKind::Oak => {
            fb.rect((x - 5.0 * s) as i32, (y - 40.0 * s) as i32, (10.0 * s).ceil() as i32, (40.0 * s).ceil() as i32, 0x6D4C41);
            fb.circle(x, y - 62.0 * s, 32.0 * s, 0x2E7D32);
            fb.circle(x - 16.0 * s, y - 50.0 * s, 22.0 * s, 0x388E3C);
            fb.circle(x + 15.0 * s, y - 72.0 * s, 20.0 * s, 0x43A047);
        }
        PropKind::Bush => {
            let c = if th.scenery == Scenery::Desert { 0x8D8B4B } else if th.scenery == Scenery::Snow { 0x5D8A7A } else { 0x2E7D32 };
            fb.circle(x, y - 10.0 * s, 15.0 * s, c);
            fb.circle(x + 13.0 * s, y - 6.0 * s, 10.0 * s, shade(c, 1.2));
        }
        PropKind::Cactus => {
            let c = 0x2E7D32;
            fb.rect((x - 6.0 * s) as i32, (y - 62.0 * s) as i32, (12.0 * s).ceil() as i32, (62.0 * s).ceil() as i32, c);
            fb.rect((x - 20.0 * s) as i32, (y - 44.0 * s) as i32, (16.0 * s).ceil() as i32, (6.0 * s).ceil() as i32, c);
            fb.rect((x - 20.0 * s) as i32, (y - 58.0 * s) as i32, (6.0 * s).ceil() as i32, (20.0 * s).ceil() as i32, c);
            fb.rect((x + 4.0 * s) as i32, (y - 34.0 * s) as i32, (16.0 * s).ceil() as i32, (6.0 * s).ceil() as i32, c);
            fb.rect((x + 14.0 * s) as i32, (y - 48.0 * s) as i32, (6.0 * s).ceil() as i32, (20.0 * s).ceil() as i32, c);
        }
        PropKind::Palm => {
            fb.poly(&[(x - 4.0 * s, y), (x + 4.0 * s, y), (x + 9.0 * s, y - 80.0 * s), (x + 3.0 * s, y - 80.0 * s)], 0x8D6E63);
            for i in 0..5 {
                let a = -0.4 + i as f32 * 0.9 - 1.0;
                let (ex, ey) = (x + 6.0 * s + a.cos() * 40.0 * s, y - 80.0 * s - a.sin().abs() * 14.0 * s + (i as f32 - 2.0).abs() * 6.0 * s);
                fb.poly(&[(x + 6.0 * s, y - 84.0 * s), (ex, ey), (x + 6.0 * s + a.cos() * 22.0 * s, y - 92.0 * s)], 0x2E9E4A);
            }
        }
        PropKind::Rock => {
            let base = if th.scenery == Scenery::Snow { 0x9AA7B4 } else if th.scenery == Scenery::Desert { 0xA1887F } else { 0x78909C };
            fb.poly(&[(x - 20.0 * s, y), (x - 14.0 * s, y - 16.0 * s), (x - 2.0 * s, y - 22.0 * s), (x + 12.0 * s, y - 15.0 * s), (x + 20.0 * s, y)], base);
            fb.poly(&[(x - 2.0 * s, y - 22.0 * s), (x + 12.0 * s, y - 15.0 * s), (x + 20.0 * s, y), (x + 4.0 * s, y)], shade(base, 0.8));
        }
        PropKind::Lamp => {
            fb.rect((x - 2.0 * s) as i32, (y - 92.0 * s) as i32, (4.0 * s).ceil() as i32, (92.0 * s).ceil() as i32, 0x37474F);
            if th.scenery == Scenery::Night {
                glow(fb, x, y - 94.0 * s, 46.0 * s, 0xFFF59D, 0.55);
            }
            fb.circle(x, y - 94.0 * s, 5.0 * s, 0xFFF9C4);
        }
        PropKind::Tent => {
            let c = match th.scenery { Scenery::Snow => 0xE3F2FD, Scenery::Desert => 0xD7A86E, _ => 0xFF8F00 };
            fb.poly(&[(x - 30.0 * s, y), (x, y - 44.0 * s), (x + 30.0 * s, y)], c);
            fb.poly(&[(x - 8.0 * s, y), (x, y - 26.0 * s), (x + 8.0 * s, y)], shade(c, 0.45));
            fb.poly(&[(x, y - 44.0 * s), (x + 30.0 * s, y), (x + 14.0 * s, y)], shade(c, 0.8));
        }
        PropKind::Flowers => {
            for (i, col) in [0xF48FB1, 0xFFF176, 0xFFFFFF, 0xCE93D8].iter().enumerate() {
                let (dx, dy) = ((i as f32 - 1.5) * 7.0, ((i * 5) % 3) as f32 * 3.0);
                fb.circle(x + dx * s, y - dy * s - 3.0 * s, 3.0 * s, *col);
                fb.rect((x + dx * s) as i32, (y - dy * s) as i32, 1, (4.0 * s) as i32, 0x2E7D32);
            }
        }
        PropKind::Campfire => {
            fb.poly(&[(x - 12.0 * s, y), (x + 12.0 * s, y - 4.0 * s), (x + 12.0 * s, y), (x - 12.0 * s, y + 3.0 * s)], 0x5D4037);
            let fl = 22.0 + 5.0 * (time * 13.0 + p.pos.x).sin();
            if th.scenery == Scenery::Night {
                glow(fb, x, y - 14.0 * s, 70.0 * s, 0xFFB74D, 0.45);
            }
            fb.poly(&[(x - 9.0 * s, y - 2.0 * s), (x, y - fl * s), (x + 9.0 * s, y - 2.0 * s)], 0xFF6F00);
            fb.poly(&[(x - 5.0 * s, y - 2.0 * s), (x, y - fl * 0.6 * s), (x + 5.0 * s, y - 2.0 * s)], 0xFFEB3B);
        }
        PropKind::Snowman => {
            fb.circle(x, y - 12.0 * s, 13.0 * s, 0xFFFFFF);
            fb.circle(x, y - 32.0 * s, 10.0 * s, 0xFFFFFF);
            fb.circle(x, y - 47.0 * s, 7.0 * s, 0xFFFFFF);
            fb.rect((x - 6.0 * s) as i32, (y - 58.0 * s) as i32, (12.0 * s).ceil() as i32, (6.0 * s).ceil() as i32, 0x263238);
            fb.poly(&[(x, y - 47.0 * s), (x + 10.0 * s, y - 45.0 * s), (x, y - 44.0 * s)], 0xFF7043);
            fb.circle(x - 2.5 * s, y - 49.0 * s, 1.2 * s, 0x111111);
            fb.circle(x + 2.5 * s, y - 49.0 * s, 1.2 * s, 0x111111);
        }
        PropKind::Table | PropKind::Arch => {}
    }
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
    for (i, p) in tr.props.iter().enumerate() {
        let margin = if p.kind == PropKind::Arch { 200.0 } else { 80.0 };
        if let Some(d) = visible(p.pos, margin) {
            objs.push((d, Obj::Prop(i)));
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
            Obj::Prop(i) => draw_prop(fb, &pr, tr, &tr.props[i], time),
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
            Obj::Kart(i) => draw_kart3d(fb, &pr, &gs.karts[i], i, cam.vis[i % 8], time),
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
