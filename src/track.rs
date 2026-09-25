use crate::math::*;

pub const CELL: f32 = 4.0;
pub const HALF_W: f32 = 85.0;
pub const KERB_W: f32 = 10.0;
pub const GRASS_W: f32 = 70.0;
/// Shells and bombs bounce once they leave the asphalt by this much.
pub const SHELL_LIMIT: f32 = HALF_W + 8.0;
const FAR: f32 = 1.0e4;
const SPACING: f32 = 8.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surf {
    Road,
    Kerb,
    Pad,
    Grass,
    Wall,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scenery {
    Forest,
    Desert,
    Snow,
    Night,
}

pub struct Theme {
    pub scenery: Scenery,
    pub road: u32,
    pub grass: [u32; 2],
    pub wall: [u32; 2],
    pub clearing: u32,
    pub kerb: [u32; 2],
    pub sky: u32,
    pub fog: u32,
    pub hills: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropKind {
    Pine,
    Oak,
    SnowPine,
    Cactus,
    Palm,
    Rock,
    Bush,
    Lamp,
    Table,
    Tent,
    Flowers,
    Campfire,
    Snowman,
    Arch,
}

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub pos: V2,
    pub kind: PropKind,
    pub scale: f32,
    pub yaw: f32,
}

/// A rectangular boost pad: centre, unit direction of travel.
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    pub c: V2,
    pub t: V2,
}

pub const PAD_HALF_LEN: f32 = 36.0;
pub const PAD_HALF_W: f32 = 32.0;
const FINISH_HALF_LEN: f32 = 12.0;
pub const CLEARING_R: f32 = 95.0;

struct Def {
    name: &'static str,
    ctrl: &'static [(f32, f32)],
    theme: Theme,
}

const DEFS: [Def; 4] = [
    Def {
        name: "Meadow Circuit",
        ctrl: &[
            (500.0, 500.0), (1300.0, 420.0), (2100.0, 500.0), (2800.0, 800.0), (3200.0, 1400.0),
            (3000.0, 2000.0), (2400.0, 2300.0), (1900.0, 2000.0), (1500.0, 1500.0), (1100.0, 1800.0),
            (1200.0, 2400.0), (700.0, 2700.0), (200.0, 2300.0), (200.0, 1500.0), (300.0, 900.0),
        ],
        theme: Theme {
            scenery: Scenery::Forest, road: 0x5A5D66, grass: [0x3C8C3C, 0x37823A], wall: [0x1B5E20, 0x143D18],
            clearing: 0x5DAA4F, kerb: [0xD32F2F, 0xF5F5F5], sky: 0x3F8FD8, fog: 0xBFE3F5, hills: 0x4F8A73,
        },
    },
    Def {
        name: "Dune Drift",
        ctrl: &[
            (600.0, 600.0), (1500.0, 450.0), (2500.0, 600.0), (3200.0, 1000.0), (3200.0, 1700.0),
            (2600.0, 2100.0), (1900.0, 1900.0), (1400.0, 2300.0), (700.0, 2400.0), (250.0, 1900.0),
            (250.0, 1200.0),
        ],
        theme: Theme {
            scenery: Scenery::Desert, road: 0x66605A, grass: [0xE0C080, 0xD8B676], wall: [0xB58A4A, 0xA47C40],
            clearing: 0xEBD59A, kerb: [0xE65100, 0xF5F5F5], sky: 0x4FA8E8, fog: 0xF3DDB0, hills: 0xC79A5A,
        },
    },
    Def {
        name: "Frost Ridge",
        ctrl: &[
            (400.0, 400.0), (1200.0, 300.0), (1900.0, 700.0), (2600.0, 300.0), (3300.0, 500.0),
            (3400.0, 1200.0), (2900.0, 1700.0), (3300.0, 2300.0), (2600.0, 2700.0), (1800.0, 2400.0),
            (1000.0, 2700.0), (300.0, 2200.0), (500.0, 1500.0), (300.0, 900.0),
        ],
        theme: Theme {
            scenery: Scenery::Snow, road: 0x50555E, grass: [0xF0F4F8, 0xE3EAF1], wall: [0xC5D3E0, 0xB4C4D3],
            clearing: 0xFFFFFF, kerb: [0x1976D2, 0xF5F5F5], sky: 0x7FA8D8, fog: 0xE6EEF7, hills: 0x9DB4CC,
        },
    },
    Def {
        name: "Neon Nights",
        ctrl: &[
            (500.0, 500.0), (1500.0, 400.0), (2400.0, 600.0), (3000.0, 1100.0), (2700.0, 1700.0),
            (2000.0, 1500.0), (1600.0, 2000.0), (2100.0, 2600.0), (1300.0, 2900.0), (500.0, 2600.0),
            (300.0, 1800.0), (700.0, 1200.0),
        ],
        theme: Theme {
            scenery: Scenery::Night, road: 0x30323A, grass: [0x1F4D2E, 0x1B4529], wall: [0x0E2A18, 0x0A2012],
            clearing: 0x2A6A3E, kerb: [0xFF4081, 0xF5F5F5], sky: 0x0A0F2A, fog: 0x1C2450, hills: 0x16204A,
        },
    },
];

pub fn track_name(i: usize) -> &'static str {
    DEFS[i % DEFS.len()].name
}

pub struct Track {
    pub name: &'static str,
    pub theme: &'static Theme,
    pub pts: Vec<V2>,
    pub n: usize,
    pub tang: Vec<V2>,
    pub box_pos: Vec<V2>,
    pub coin_pos: Vec<V2>,
    pub pads: Vec<Pad>,
    /// Scenery for the 3D view: trees, props, picnic clearings...
    pub props: Vec<Prop>,
    pub clearings: Vec<V2>,
    pub min: V2,
    pub max: V2,
    origin: V2,
    gw: usize,
    gh: usize,
    dist: Vec<f32>,
    near: Vec<u16>,
}

fn catmull(p0: V2, p1: V2, p2: V2, p3: V2, t: f32) -> V2 {
    let t2 = t * t;
    let t3 = t2 * t;
    (p1 * 2.0
        + (p2 - p0) * t
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
        * 0.5
}

/// Largest direction change within +-w segments of i (0 = perfectly straight).
fn straightness(tang: &[V2], i: usize, w: i32, step: i32) -> f32 {
    let n = tang.len() as i32;
    let mut worst = 0.0f32;
    let mut d = -w;
    while d <= w {
        let j = (i as i32 + d).rem_euclid(n) as usize;
        worst = worst.max((tang[i].x * tang[j].y - tang[i].y * tang[j].x).abs());
        d += step;
    }
    worst
}

fn hash01(a: usize, b: usize) -> f32 {
    let mut h = (a as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (b as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 32;
    (h & 0xFFFFFF) as f32 / 0xFFFFFF as f32
}

impl Track {
    #[cfg(test)]
    pub fn new() -> Track {
        Track::build(0)
    }

    pub fn all() -> Vec<Track> {
        (0..DEFS.len()).map(Track::build).collect()
    }

    pub fn build(index: usize) -> Track {
        let def = &DEFS[index % DEFS.len()];
        let ctrl: Vec<V2> = def.ctrl.iter().map(|&(x, y)| v2(x, y)).collect();
        let c = ctrl.len();
        let mut dense = Vec::new();
        for i in 0..c {
            let (p0, p1, p2, p3) = (ctrl[(i + c - 1) % c], ctrl[i], ctrl[(i + 1) % c], ctrl[(i + 2) % c]);
            for s in 0..64 {
                dense.push(catmull(p0, p1, p2, p3, s as f32 / 64.0));
            }
        }
        // resample at constant arc-length spacing
        let m = dense.len();
        let mut total = 0.0;
        for i in 0..m {
            total += dense[i].dist(dense[(i + 1) % m]);
        }
        let n = (total / SPACING).round() as usize;
        let step = total / n as f32;
        let mut pts = Vec::with_capacity(n);
        let (mut seg, mut acc) = (0usize, 0.0f32);
        for k in 0..n {
            let target = k as f32 * step;
            loop {
                let l = dense[seg % m].dist(dense[(seg + 1) % m]);
                if acc + l >= target || seg > 2 * m {
                    let t = if l > 0.0 { (target - acc) / l } else { 0.0 };
                    pts.push(dense[seg % m] + (dense[(seg + 1) % m] - dense[seg % m]) * t);
                    break;
                }
                acc += l;
                seg += 1;
            }
        }
        let tangents = |pts: &[V2]| -> Vec<V2> { (0..pts.len()).map(|i| (pts[(i + 1) % pts.len()] - pts[i]).norm()).collect() };

        // start/finish goes in the straightest stretch (room for the starting grid)
        let t0 = tangents(&pts);
        let start = (0..n).step_by(3).min_by(|&a, &b| straightness(&t0, a, 30, 5).partial_cmp(&straightness(&t0, b, 30, 5)).unwrap()).unwrap();
        pts.rotate_left(start);
        let tang = tangents(&pts);

        let reach = HALF_W + GRASS_W + 12.0;
        let (mut mn, mut mx) = (v2(1e9, 1e9), v2(-1e9, -1e9));
        for p in &pts {
            mn = v2(mn.x.min(p.x), mn.y.min(p.y));
            mx = v2(mx.x.max(p.x), mx.y.max(p.y));
        }
        let origin = mn - v2(reach, reach);
        let gw = ((mx.x - mn.x + 2.0 * reach) / CELL) as usize + 2;
        let gh = ((mx.y - mn.y + 2.0 * reach) / CELL) as usize + 2;
        let mut dist = vec![FAR; gw * gh];
        let mut near = vec![0u16; gw * gh];
        let r = (reach / CELL) as i32 + 1;
        for (i, p) in pts.iter().enumerate() {
            let cx = ((p.x - origin.x) / CELL) as i32;
            let cy = ((p.y - origin.y) / CELL) as i32;
            for gy in (cy - r).max(0)..=(cy + r).min(gh as i32 - 1) {
                for gx in (cx - r).max(0)..=(cx + r).min(gw as i32 - 1) {
                    let w = v2(origin.x + gx as f32 * CELL, origin.y + gy as f32 * CELL);
                    let d = w.dist(*p);
                    let id = gy as usize * gw + gx as usize;
                    if d < dist[id] {
                        dist[id] = d;
                        near[id] = i as u16;
                    }
                }
            }
        }

        // boost pads: perfectly rectangular, on straight bits of road
        let mut pads = Vec::new();
        for f in [0.12f32, 0.37, 0.62, 0.84] {
            let centre = (f * n as f32) as i32;
            let best = (centre - 70..=centre + 70)
                .filter(|&i| i >= 45 && i < n as i32 - 45)
                .min_by(|&a, &b| straightness(&tang, a as usize, 8, 2).partial_cmp(&straightness(&tang, b as usize, 8, 2)).unwrap())
                .unwrap() as usize;
            pads.push(Pad { c: pts[best], t: tang[best] });
        }
        let mut box_pos = Vec::new();
        for f in [0.06f32, 0.27, 0.48, 0.70, 0.91] {
            let i = (f * n as f32) as usize;
            for off in [-54.0f32, -18.0, 18.0, 54.0] {
                box_pos.push(pts[i] + tang[i].right() * off);
            }
        }
        let mut coin_pos = Vec::new();
        for k in 0..60 {
            let i = (k * n / 60 + n / 120) % n;
            coin_pos.push(pts[i] + tang[i].right() * (45.0 * (k as f32 * 0.55).sin()));
        }
        let mut t = Track {
            name: def.name, theme: &def.theme, pts, n, tang, box_pos, coin_pos, pads, props: Vec::new(), clearings: Vec::new(),
            min: mn, max: mx, origin, gw, gh, dist, near,
        };
        t.scatter_scenery(index);
        t
    }

    fn scatter_scenery(&mut self, seed: usize) {
        let th = self.theme;
        let n = self.n;
        let outer = HALF_W + GRASS_W;
        // picnic-style clearings just outside the track
        let mut clearings = Vec::new();
        for k in 0..7 {
            let i = (k * n / 7 + n / 14 + seed * 11) % n;
            for side in [if k % 2 == 0 { 1.0f32 } else { -1.0 }, if k % 2 == 0 { -1.0 } else { 1.0 }] {
                let c = self.pts[i] + self.tang[i].right() * (side * (outer + 88.0));
                if self.dist_at(c) >= outer + 70.0 && clearings.iter().all(|q: &V2| q.dist(c) > 300.0) {
                    clearings.push(c);
                    break;
                }
            }
        }
        let mut props = Vec::new();
        for (ci, c) in clearings.iter().enumerate() {
            let mut put = |kind: PropKind, dx: f32, dy: f32, scale: f32, yaw: f32| {
                props.push(Prop { pos: *c + v2(dx, dy), kind, scale, yaw });
            };
            match th.scenery {
                Scenery::Forest => {
                    put(PropKind::Table, 0.0, 0.0, 1.0, ci as f32);
                    put(PropKind::Tent, 46.0, -30.0, 1.0, 0.0);
                    put(PropKind::Campfire, -42.0, 26.0, 1.0, 0.0);
                    for f in 0..6 {
                        put(PropKind::Flowers, (hash01(ci, f) - 0.5) * 120.0, (hash01(f, ci + 9) - 0.5) * 120.0, 1.0, 0.0);
                    }
                }
                Scenery::Desert => {
                    put(PropKind::Palm, -30.0, -10.0, 1.1, 0.0);
                    put(PropKind::Palm, 30.0, 14.0, 0.9, 0.0);
                    put(PropKind::Tent, 0.0, 44.0, 1.0, 0.0);
                    put(PropKind::Campfire, -40.0, 40.0, 1.0, 0.0);
                    for f in 0..4 {
                        put(PropKind::Rock, (hash01(ci, f) - 0.5) * 130.0, (hash01(f, ci + 5) - 0.5) * 130.0, 0.8, 0.0);
                    }
                }
                Scenery::Snow => {
                    put(PropKind::Snowman, -20.0, 0.0, 1.0, 0.0);
                    put(PropKind::Snowman, 26.0, 12.0, 0.7, 0.0);
                    put(PropKind::Tent, 0.0, -44.0, 1.0, 0.0);
                    put(PropKind::Campfire, 44.0, 40.0, 1.0, 0.0);
                    for f in 0..3 {
                        put(PropKind::Rock, (hash01(ci, f) - 0.5) * 130.0, (hash01(f, ci + 5) - 0.5) * 130.0, 0.7, 0.0);
                    }
                }
                Scenery::Night => {
                    put(PropKind::Campfire, 0.0, 0.0, 1.3, 0.0);
                    put(PropKind::Tent, 50.0, -20.0, 1.0, 0.0);
                    put(PropKind::Table, -46.0, 24.0, 1.0, ci as f32 * 2.0);
                    put(PropKind::Lamp, 0.0, -50.0, 1.0, 0.0);
                }
            }
        }
        // start/finish arch
        props.push(Prop { pos: self.pts[0], kind: PropKind::Arch, scale: 1.0, yaw: self.tang[0].y.atan2(self.tang[0].x) });
        // trees etc: thinning rows with irregular gaps
        for i in (0..n).step_by(5) {
            for side in [-1.0f32, 1.0] {
                for (row, (off, keep)) in [(outer + 8.0, 0.72f32), (outer + 52.0, 0.45), (outer + 100.0, 0.3)].into_iter().enumerate() {
                    let h = hash01(i * 2 + (side > 0.0) as usize, row + seed * 7);
                    if h > keep {
                        continue;
                    }
                    let jitter = (hash01(i, row + 40) - 0.5) * 30.0;
                    let jit2 = (hash01(i + 3, row + 90) - 0.5) * 30.0;
                    let p = self.pts[i] + self.tang[i].right() * (side * (off + jitter)) + self.tang[i] * jit2;
                    if self.dist_at(p) < outer + 2.0 || clearings.iter().any(|c| c.dist(p) < CLEARING_R + 10.0) {
                        continue;
                    }
                    let pick = hash01(i + 77, row * 3 + 1);
                    let kind = match th.scenery {
                        Scenery::Forest => if pick < 0.5 { PropKind::Pine } else if pick < 0.85 { PropKind::Oak } else { PropKind::Bush },
                        Scenery::Desert => if pick < 0.4 { PropKind::Cactus } else if pick < 0.65 { PropKind::Rock } else if pick < 0.8 { PropKind::Palm } else { PropKind::Bush },
                        Scenery::Snow => if pick < 0.7 { PropKind::SnowPine } else if pick < 0.85 { PropKind::Rock } else { PropKind::Bush },
                        Scenery::Night => if pick < 0.85 { PropKind::Pine } else { PropKind::Bush },
                    };
                    props.push(Prop { pos: p, kind, scale: 0.7 + 0.7 * hash01(i, row + 500), yaw: 0.0 });
                }
            }
        }
        if th.scenery == Scenery::Night {
            for i in (0..n).step_by(25) {
                for side in [-1.0f32, 1.0] {
                    let p = self.pts[i] + self.tang[i].right() * (side * (outer + 6.0));
                    if self.dist_at(p) >= outer + 2.0 - 6.0 {
                        props.push(Prop { pos: p, kind: PropKind::Lamp, scale: 1.0, yaw: 0.0 });
                    }
                }
            }
        }
        self.props = props;
        self.clearings = clearings;
    }

    /// (along, lateral) if `p` is on a boost pad.
    pub fn pad_at(&self, p: V2) -> Option<(f32, f32)> {
        for pad in &self.pads {
            let d = p - pad.c;
            let (a, b) = (d.dot(pad.t), d.dot(pad.t.right()));
            if a.abs() < PAD_HALF_LEN && b.abs() < PAD_HALF_W {
                return Some((a, b));
            }
        }
        None
    }

    /// (along, lateral) if `p` is on the checkered start/finish strip.
    pub fn finish_at(&self, p: V2) -> Option<(f32, f32)> {
        let d = p - self.pts[0];
        let (a, b) = (d.dot(self.tang[0]), d.dot(self.tang[0].right()));
        (a.abs() < FINISH_HALF_LEN && b.abs() < HALF_W - KERB_W).then_some((a, b))
    }

    pub fn in_clearing(&self, p: V2) -> bool {
        self.clearings.iter().any(|c| c.dist(p) < CLEARING_R)
    }

    /// Centerline point (wrapping index).
    pub fn pt(&self, i: i32) -> V2 {
        self.pts[i.rem_euclid(self.n as i32) as usize]
    }

    pub fn tangent(&self, i: i32) -> V2 {
        self.tang[i.rem_euclid(self.n as i32) as usize]
    }

    fn at(&self, gx: i32, gy: i32) -> f32 {
        if gx < 0 || gy < 0 || gx >= self.gw as i32 || gy >= self.gh as i32 {
            FAR
        } else {
            self.dist[gy as usize * self.gw + gx as usize]
        }
    }

    /// Distance to the centerline, bilinearly smoothed.
    pub fn dist_at(&self, p: V2) -> f32 {
        let fx = (p.x - self.origin.x) / CELL;
        let fy = (p.y - self.origin.y) / CELL;
        let (x0, y0) = (fx.floor(), fy.floor());
        let (tx, ty) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i32, y0 as i32);
        let a = lerp(self.at(x0, y0), self.at(x0 + 1, y0), tx);
        let b = lerp(self.at(x0, y0 + 1), self.at(x0 + 1, y0 + 1), tx);
        lerp(a, b, ty)
    }

    /// Unit vector pointing away from the road centre.
    pub fn outward(&self, p: V2) -> V2 {
        let e = 3.0;
        v2(
            self.dist_at(p + v2(e, 0.0)) - self.dist_at(p - v2(e, 0.0)),
            self.dist_at(p + v2(0.0, e)) - self.dist_at(p - v2(0.0, e)),
        )
        .norm()
    }

    pub fn near_idx(&self, p: V2) -> usize {
        let gx = ((p.x - self.origin.x) / CELL).round() as i32;
        let gy = ((p.y - self.origin.y) / CELL).round() as i32;
        if gx < 0 || gy < 0 || gx >= self.gw as i32 || gy >= self.gh as i32 {
            0
        } else {
            self.near[gy as usize * self.gw + gx as usize] as usize
        }
    }

    pub fn surface(&self, p: V2) -> Surf {
        let d = self.dist_at(p);
        if d < HALF_W - KERB_W {
            if self.pad_at(p).is_some() {
                return Surf::Pad;
            }
            Surf::Road
        } else if d < HALF_W {
            Surf::Kerb
        } else if d < HALF_W + GRASS_W {
            Surf::Grass
        } else {
            Surf::Wall
        }
    }

    /// Local nearest-segment search starting from a running index.
    /// Returns (new running index, fraction along segment, lateral distance).
    pub fn locate(&self, p: V2, run: i32) -> (i32, f32, f32) {
        let mut best = (run, 0.0, f32::MAX);
        for d in -4..=14 {
            let cand = run + d;
            let (t, dist) = self.seg_dist(p, cand);
            if dist < best.2 {
                best = (cand, t, dist);
            }
        }
        if best.2 > 300.0 {
            // lost: global search
            let mut bi = 0usize;
            let mut bd = f32::MAX;
            for (i, q) in self.pts.iter().enumerate() {
                let d = q.dist(p);
                if d < bd {
                    bd = d;
                    bi = i;
                }
            }
            let base = run - run.rem_euclid(self.n as i32);
            let mut cand = base + bi as i32;
            if cand - run > self.n as i32 / 2 {
                cand -= self.n as i32;
            } else if run - cand > self.n as i32 / 2 {
                cand += self.n as i32;
            }
            let (t, dist) = self.seg_dist(p, cand);
            return (cand, t, dist);
        }
        best
    }

    fn seg_dist(&self, p: V2, i: i32) -> (f32, f32) {
        let a = self.pt(i);
        let b = self.pt(i + 1);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
        (t, (a + ab * t).dist(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_are_well_separated() {
        for t in Track::all() {
            let n = t.n as i32;
            let mut worst = f32::MAX;
            for i in (0..t.n).step_by(2) {
                for j in (0..t.n).step_by(2) {
                    let sep = ((i as i32 - j as i32).abs()).min(n - (i as i32 - j as i32).abs());
                    if sep > 120 {
                        worst = worst.min(t.pts[i].dist(t.pts[j]));
                    }
                }
            }
            assert!(worst > 330.0, "{}: sections too close: {worst}", t.name);
            assert!(t.box_pos.len() <= 32 && t.coin_pos.len() <= 64);
        }
    }

    #[test]
    fn centerline_is_road_and_pads_are_rectangles() {
        for t in Track::all() {
            assert!(matches!(t.surface(t.pts[500]), Surf::Road | Surf::Pad));
            assert_eq!(t.surface(v2(-5000.0, -5000.0)), Surf::Wall);
            for pad in &t.pads {
                // all four corners of the pad rectangle are on it, and just outside they are not
                let r = pad.t.right();
                for (a, b) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                    assert!(t.pad_at(pad.c + pad.t * (a * (PAD_HALF_LEN - 1.0)) + r * (b * (PAD_HALF_W - 1.0))).is_some());
                    assert!(t.pad_at(pad.c + pad.t * (a * (PAD_HALF_LEN + 1.0)) + r * (b * (PAD_HALF_W + 1.0))).is_none());
                }
                assert!(matches!(t.surface(pad.c), Surf::Pad), "{}: pad not on the road", t.name);
            }
        }
    }
}

