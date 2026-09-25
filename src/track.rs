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

pub struct Track {
    pub pts: Vec<V2>,
    pub n: usize,
    pub tang: Vec<V2>,
    pub box_pos: Vec<V2>,
    pub coin_pos: Vec<V2>,
    pub pad_ranges: Vec<(usize, usize)>,
    pub min: V2,
    pub max: V2,
    origin: V2,
    gw: usize,
    gh: usize,
    dist: Vec<f32>,
    near: Vec<u16>,
}

const CONTROL: [(f32, f32); 15] = [
    (500.0, 500.0),
    (1300.0, 420.0),
    (2100.0, 500.0),
    (2800.0, 800.0),
    (3200.0, 1400.0),
    (3000.0, 2000.0),
    (2400.0, 2300.0),
    (1900.0, 2000.0),
    (1500.0, 1500.0),
    (1100.0, 1800.0),
    (1200.0, 2400.0),
    (700.0, 2700.0),
    (200.0, 2300.0),
    (200.0, 1500.0),
    (300.0, 900.0),
];

fn catmull(p0: V2, p1: V2, p2: V2, p3: V2, t: f32) -> V2 {
    let t2 = t * t;
    let t3 = t2 * t;
    (p1 * 2.0
        + (p2 - p0) * t
        + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
        + (p1 * 3.0 - p0 - p2 * 3.0 + p3) * t3)
        * 0.5
}

impl Track {
    pub fn new() -> Track {
        let ctrl: Vec<V2> = CONTROL.iter().map(|&(x, y)| v2(x, y)).collect();
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
        let tang: Vec<V2> = (0..n).map(|i| (pts[(i + 1) % n] - pts[i]).norm()).collect();

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

        let pad_ranges = [0.12f32, 0.37, 0.62, 0.84]
            .iter()
            .map(|f| {
                let s = (f * n as f32) as usize;
                (s, s + 9)
            })
            .collect();
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
        Track { pts, n, tang, box_pos, coin_pos, pad_ranges, min: mn, max: mx, origin, gw, gh, dist, near }
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
            if d < 30.0 {
                let i = self.near_idx(p);
                if self.pad_ranges.iter().any(|&(a, b)| i >= a && i < b) {
                    return Surf::Pad;
                }
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
    fn track_is_well_separated() {
        let t = Track::new();
        let n = t.n as i32;
        let mut worst = f32::MAX;
        for i in 0..t.n {
            for j in 0..t.n {
                let sep = ((i as i32 - j as i32).abs()).min(n - (i as i32 - j as i32).abs());
                if sep > 120 {
                    worst = worst.min(t.pts[i].dist(t.pts[j]));
                }
            }
        }
        assert!(worst > 330.0, "sections too close: {worst}");
    }

    #[test]
    fn centerline_is_road() {
        let t = Track::new();
        assert!(matches!(t.surface(t.pts[500]), Surf::Road | Surf::Pad));
        assert_eq!(t.surface(v2(-5000.0, -5000.0)), Surf::Wall);
    }
}
