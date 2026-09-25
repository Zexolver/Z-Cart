//! LAN networking: plain UDP over IPv6 link-local addresses, no admin rights
//! needed. One peer hosts (and simulates); the others send inputs and render
//! the snapshots they receive. Hosts are found via link-local multicast
//! (ff02::1) discovery.

use crate::math::*;
use crate::sim::*;
use crate::track::Track;
use std::io::ErrorKind;
use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6, UdpSocket};
use std::time::{Duration, Instant};

pub const PORT: u16 = 47777;
const MAGIC: [u8; 3] = [b'Z', b'C', 1];
const T_QUERY: u8 = 1;
const T_REPLY: u8 = 2;
const T_JOIN: u8 = 3;
const T_WELCOME: u8 = 4;
const T_REJECT: u8 = 5;
const T_INPUT: u8 = 6;
const T_SNAP: u8 = 7;
const T_LEAVE: u8 = 8;
const PEER_TIMEOUT: Duration = Duration::from_secs(5);

/// Only link-local (fe80::/10) and loopback (for local testing) peers are served.
pub fn is_lan_addr(a: &SocketAddr) -> bool {
    match a {
        SocketAddr::V6(v) => v.ip().is_loopback() || (v.ip().segments()[0] & 0xffc0) == 0xfe80,
        _ => false,
    }
}

// ---------------------------------------------------------------- wire helpers

struct W(Vec<u8>);
impl W {
    fn new(t: u8) -> W {
        let mut v = MAGIC.to_vec();
        v.push(t);
        W(v)
    }
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn timer(&mut self, v: f32) {
        self.u8((v * 10.0).clamp(0.0, 255.0) as u8);
    }
    fn str(&mut self, s: &str) {
        let b = s.as_bytes();
        let n = b.len().min(NAME_LEN);
        self.u8(n as u8);
        self.0.extend_from_slice(&b[..n]);
    }
}

struct R<'a> {
    b: &'a [u8],
    p: usize,
}
impl<'a> R<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let s = self.b.get(self.p..self.p + n)?;
        self.p += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        let v = f32::from_le_bytes(self.take(4)?.try_into().ok()?);
        if v.is_finite() { Some(v) } else { None }
    }
    fn timer(&mut self) -> Option<f32> {
        Some(self.u8()? as f32 / 10.0)
    }
    fn str(&mut self) -> Option<String> {
        let n = self.u8()? as usize;
        Some(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

/// Splits a datagram into (type, payload) if it carries our magic.
fn parse(buf: &[u8]) -> Option<(u8, R<'_>)> {
    if buf.len() < 4 || buf[..3] != MAGIC {
        return None;
    }
    Some((buf[3], R { b: buf, p: 4 }))
}

// ---------------------------------------------------------------- snapshots

const TIERS: [f32; 4] = [0.0, 0.9, 1.8, 3.0];

fn encode_snapshot(gs: &GameState, seq: u32, your_id: usize) -> Vec<u8> {
    let mut w = W::new(T_SNAP);
    w.u32(seq);
    w.u8(gs.phase as u8);
    w.f32(gs.timer);
    w.u8(gs.laps);
    w.u8(gs.bots);
    w.u8(gs.track);
    w.u8(gs.track_sel);
    w.u8(gs.karts.len() as u8);
    for k in &gs.karts {
        w.str(&k.name);
        w.u8(k.is_bot as u8 | (k.finished as u8) << 1);
        for v in [k.pos.x, k.pos.y, k.vel.x, k.vel.y, k.heading] {
            w.f32(v);
        }
        w.u8(k.drift_dir as u8);
        w.u8(k.drift_tier());
        for t in [k.boost, k.star, k.giant, k.shrunk, k.inked, k.rocket] {
            w.timer(t);
        }
        w.u8((k.spin * 100.0).clamp(0.0, 255.0) as u8);
        for s in k.slots {
            w.u8(s.0);
            w.u8(s.1);
        }
        w.u8(k.coins);
        w.u8(k.place);
        w.f32(k.prog);
        w.f32(k.finish_time);
    }
    w.u8(gs.ents.len().min(MAX_ENTS) as u8);
    for e in gs.ents.iter().take(MAX_ENTS) {
        w.u8(e.kind as u8);
        w.f32(e.pos.x);
        w.f32(e.pos.y);
        let (a, b) = match e.kind {
            EntKind::Blast => ((e.age / 2.0) as u8, (e.timer * 200.0) as u8),
            EntKind::Bomb => (angle_byte(e.vel), (e.timer * 50.0) as u8),
            _ => (angle_byte(e.vel), 0),
        };
        w.u8(a);
        w.u8(b);
    }
    w.u32(gs.boxes);
    w.u64(gs.coins);
    w.u8(your_id as u8);
    w.0
}

fn angle_byte(v: V2) -> u8 {
    ((v.y.atan2(v.x) + std::f32::consts::PI) / std::f32::consts::TAU * 255.0) as u8
}

pub fn byte_angle(b: u8) -> f32 {
    b as f32 / 255.0 * std::f32::consts::TAU - std::f32::consts::PI
}

/// Rebuilds `gs` from a snapshot body. Returns (seq, your kart id).
fn decode_snapshot(mut r: R, gs: &mut GameState) -> Option<(u32, usize)> {
    let seq = r.u32()?;
    let phase = Phase::from_u8(r.u8()?);
    let timer = r.f32()?;
    let laps = r.u8()?;
    let bots = r.u8()?;
    let track = r.u8()?;
    let track_sel = r.u8()?;
    let nk = r.u8()? as usize;
    if nk > MAX_KARTS {
        return None;
    }
    let mut karts = Vec::with_capacity(nk);
    for _ in 0..nk {
        let mut k = Kart::new(&r.str()?, false);
        let fl = r.u8()?;
        k.is_bot = fl & 1 != 0;
        k.finished = fl & 2 != 0;
        k.pos = v2(r.f32()?, r.f32()?);
        k.vel = v2(r.f32()?, r.f32()?);
        k.heading = r.f32()?;
        k.drift_dir = r.u8()? as i8;
        k.drift_charge = TIERS[(r.u8()? as usize).min(3)];
        k.boost = r.timer()?;
        k.star = r.timer()?;
        k.giant = r.timer()?;
        k.shrunk = r.timer()?;
        k.inked = r.timer()?;
        k.rocket = r.timer()?;
        k.spin = r.u8()? as f32 / 100.0;
        for s in 0..2 {
            k.slots[s] = (r.u8()?, r.u8()?);
        }
        k.coins = r.u8()?;
        k.place = r.u8()?;
        k.prog = r.f32()?;
        k.finish_time = r.f32()?;
        karts.push(k);
    }
    let ne = r.u8()? as usize;
    if ne > MAX_ENTS {
        return None;
    }
    let mut ents = Vec::with_capacity(ne);
    for _ in 0..ne {
        let kind = EntKind::from_u8(r.u8()?)?;
        let pos = v2(r.f32()?, r.f32()?);
        let (a, b) = (r.u8()?, r.u8()?);
        let mut vel = V2::from_angle(byte_angle(a));
        let (mut age, mut timer) = (0.0, 0.0);
        match kind {
            EntKind::Blast => {
                age = a as f32 * 2.0;
                timer = b as f32 / 200.0;
                vel = V2::ZERO;
            }
            EntKind::Bomb => timer = b as f32 / 50.0,
            _ => {}
        }
        let mut e = Ent { kind, pos, vel, owner: 255, age, timer, target: 255, run: 0, bounces: 0, rev: false };
        e.age = age;
        ents.push(e);
    }
    let boxes = r.u32()?;
    let coins = r.u64()?;
    let id = r.u8()? as usize;
    gs.phase = phase;
    gs.timer = timer;
    gs.laps = laps;
    gs.bots = bots;
    gs.track = track;
    gs.track_sel = track_sel;
    gs.karts = karts;
    gs.ents = ents;
    gs.boxes = boxes;
    gs.coins = coins;
    Some((seq, id))
}

fn seq_newer(a: u32, b: u32) -> bool {
    a.wrapping_sub(b) < u32::MAX / 2 && a != b
}

// ---------------------------------------------------------------- discovery

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub addr: SocketAddr,
    pub name: String,
    pub players: u8,
    pub max: u8,
    pub phase: Phase,
    pub seen: Instant,
}

impl HostInfo {
    pub fn joinable(&self) -> bool {
        self.phase == Phase::Lobby && self.players < self.max
    }
}

pub fn new_socket(port: u16) -> std::io::Result<UdpSocket> {
    let s = UdpSocket::bind(SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0)))?;
    s.set_nonblocking(true)?;
    Ok(s)
}

/// Interface indices carrying an IPv6 link-local address.
fn link_local_ifaces() -> Vec<u32> {
    let mut v: Vec<u32> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V6(a) if (a.ip.segments()[0] & 0xffc0) == 0xfe80 => i.index,
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

pub struct Browser {
    pub sock: UdpSocket,
    ifaces: Vec<u32>,
    pub hosts: Vec<HostInfo>,
    last_query: Option<Instant>,
    port: u16,
}

impl Browser {
    pub fn new() -> std::io::Result<Browser> {
        Browser::with_port(PORT)
    }

    pub fn with_port(port: u16) -> std::io::Result<Browser> {
        Ok(Browser { sock: new_socket(0)?, ifaces: link_local_ifaces(), hosts: Vec::new(), last_query: None, port })
    }

    pub fn query(&mut self) {
        self.ifaces = link_local_ifaces();
        let pkt = W::new(T_QUERY).0;
        let all_nodes = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
        for &idx in &self.ifaces {
            let _ = self.sock.send_to(&pkt, SocketAddr::V6(SocketAddrV6::new(all_nodes, self.port, 0, idx)));
        }
        // same-machine hosts
        let _ = self.sock.send_to(&pkt, SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, self.port, 0, 0)));
        self.last_query = Some(Instant::now());
    }

    /// Re-query every couple of seconds, collect replies, expire stale hosts.
    pub fn poll(&mut self) {
        if self.last_query.map_or(true, |t| t.elapsed() > Duration::from_secs(2)) {
            self.query();
        }
        let mut buf = [0u8; 512];
        for _ in 0..64 {
            match self.sock.recv_from(&mut buf) {
                Ok((n, from)) => {
                    if !is_lan_addr(&from) {
                        continue;
                    }
                    if let Some((T_REPLY, mut r)) = parse(&buf[..n]) {
                        let info = (|| {
                            Some(HostInfo {
                                addr: from,
                                phase: Phase::from_u8(r.u8()?),
                                players: r.u8()?,
                                max: r.u8()?,
                                name: r.str()?,
                                seen: Instant::now(),
                            })
                        })();
                        if let Some(info) = info {
                            match self.hosts.iter_mut().find(|h| h.addr == from) {
                                Some(h) => *h = info,
                                None => self.hosts.push(info),
                            }
                        }
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => continue,
            }
        }
        self.hosts.retain(|h| h.seen.elapsed() < Duration::from_secs(6));
    }
}

// ---------------------------------------------------------------- host

pub struct Peer {
    pub addr: SocketAddr,
    pub kart: usize,
    last_seen: Instant,
    last_input: u16,
    have_input: bool,
}

pub struct Host {
    sock: UdpSocket,
    pub peers: Vec<Peer>,
    seq: u32,
}

impl Host {
    pub fn bind(port: u16) -> std::io::Result<Host> {
        Ok(Host { sock: new_socket(port)?, peers: Vec::new(), seq: 0 })
    }

    #[cfg(test)]
    pub fn port(&self) -> u16 {
        self.sock.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    pub fn poll(&mut self, gs: &mut GameState, tr: &Track) {
        let mut buf = [0u8; 512];
        for _ in 0..512 {
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => continue,
            };
            if !is_lan_addr(&from) {
                continue;
            }
            let Some((t, mut r)) = parse(&buf[..n]) else { continue };
            match t {
                T_QUERY => {
                    let mut w = W::new(T_REPLY);
                    w.u8(gs.phase as u8);
                    w.u8(gs.humans() as u8);
                    w.u8(MAX_KARTS as u8);
                    w.str(gs.karts.first().map_or("Host", |k| k.name.as_str()));
                    let _ = self.sock.send_to(&w.0, from);
                }
                T_JOIN => {
                    if let Some(p) = self.peers.iter().find(|p| p.addr == from) {
                        self.welcome(from, p.kart);
                        continue;
                    }
                    let name = r.str().unwrap_or_else(|| "Player".into());
                    if gs.phase != Phase::Lobby {
                        self.reject(from, 1);
                    } else if let Some(k) = gs.add_human(tr, &name) {
                        self.peers.push(Peer { addr: from, kart: k, last_seen: Instant::now(), last_input: 0, have_input: false });
                        self.welcome(from, k);
                    } else {
                        self.reject(from, 2);
                    }
                }
                T_INPUT => {
                    let Some(p) = self.peers.iter_mut().find(|p| p.addr == from) else { continue };
                    p.last_seen = Instant::now();
                    let parsed = (|| Some((r.u16()?, r.u8()? as i8, r.u8()?, r.u8()?, r.u8()?)))();
                    let Some((seq, steer, flags, use_seq, swap_seq)) = parsed else { continue };
                    if p.have_input && !(seq != p.last_input && seq.wrapping_sub(p.last_input) < 32768) {
                        continue;
                    }
                    p.have_input = true;
                    p.last_input = seq;
                    if let Some(k) = gs.karts.get_mut(p.kart) {
                        k.input = Input {
                            steer: (steer as f32 / 127.0).clamp(-1.0, 1.0),
                            throttle: flags & 1 != 0,
                            brake: flags & 2 != 0,
                            drift: flags & 4 != 0,
                            flip: flags & 8 != 0,
                            use_seq,
                            swap_seq,
                        };
                    }
                }
                T_LEAVE => {
                    if let Some(i) = self.peers.iter().position(|p| p.addr == from) {
                        self.drop_peer(i, gs, tr);
                    }
                }
                _ => {}
            }
        }
        let mut i = 0;
        while i < self.peers.len() {
            if self.peers[i].last_seen.elapsed() > PEER_TIMEOUT {
                self.drop_peer(i, gs, tr);
            } else {
                i += 1;
            }
        }
    }

    fn drop_peer(&mut self, i: usize, gs: &mut GameState, tr: &Track) {
        let p = self.peers.remove(i);
        let lobby = gs.phase == Phase::Lobby;
        gs.remove_human(tr, p.kart);
        if lobby {
            for q in &mut self.peers {
                if q.kart > p.kart {
                    q.kart -= 1;
                }
            }
        }
    }

    fn welcome(&self, to: SocketAddr, id: usize) {
        let mut w = W::new(T_WELCOME);
        w.u8(id as u8);
        let _ = self.sock.send_to(&w.0, to);
    }

    fn reject(&self, to: SocketAddr, why: u8) {
        let mut w = W::new(T_REJECT);
        w.u8(why);
        let _ = self.sock.send_to(&w.0, to);
    }

    pub fn broadcast(&mut self, gs: &GameState) {
        self.seq = self.seq.wrapping_add(1);
        for p in &self.peers {
            let pkt = encode_snapshot(gs, self.seq, p.kart);
            let _ = self.sock.send_to(&pkt, p.addr);
        }
    }

    /// Tell everyone the session is over.
    pub fn shutdown(&mut self) {
        let pkt = W::new(T_LEAVE).0;
        for p in &self.peers {
            let _ = self.sock.send_to(&pkt, p.addr);
        }
    }
}

// ---------------------------------------------------------------- client

pub struct Client {
    sock: UdpSocket,
    host: SocketAddr,
    name: String,
    pub id: Option<usize>,
    pub state: GameState,
    pub snap_at: Option<Instant>,
    last_seq: Option<u32>,
    out_seq: u16,
    last_join: Instant,
    pub rejected: Option<u8>,
    pub host_gone: bool,
    started: Instant,
}

impl Client {
    pub fn new(sock: UdpSocket, host: SocketAddr, name: &str, tr: &Track) -> Client {
        let mut c = Client {
            sock,
            host,
            name: name.to_string(),
            id: None,
            state: GameState::new(tr),
            snap_at: None,
            last_seq: None,
            out_seq: 0,
            last_join: Instant::now() - Duration::from_secs(1),
            rejected: None,
            host_gone: false,
            started: Instant::now(),
        };
        c.send_join();
        c
    }

    fn send_join(&mut self) {
        let mut w = W::new(T_JOIN);
        w.str(&self.name);
        let _ = self.sock.send_to(&w.0, self.host);
        self.last_join = Instant::now();
    }

    pub fn poll(&mut self) {
        let mut buf = [0u8; 2048];
        for _ in 0..256 {
            let (n, from) = match self.sock.recv_from(&mut buf) {
                Ok(v) => v,
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => continue,
            };
            if from != self.host {
                continue;
            }
            let Some((t, mut r)) = parse(&buf[..n]) else { continue };
            match t {
                T_WELCOME => self.id = r.u8().map(|v| v as usize),
                T_REJECT => self.rejected = r.u8(),
                T_LEAVE => self.host_gone = true,
                T_SNAP => {
                    let Some(seq) = r.b.get(4..8).map(|b| u32::from_le_bytes(b.try_into().unwrap())) else { continue };
                    if self.last_seq.map_or(true, |l| seq_newer(seq, l)) {
                        if let Some((seq, id)) = decode_snapshot(r, &mut self.state) {
                            self.last_seq = Some(seq);
                            self.id = Some(id);
                            self.snap_at = Some(Instant::now());
                        }
                    }
                }
                _ => {}
            }
        }
        if self.id.is_none() && self.rejected.is_none() && self.last_join.elapsed() > Duration::from_millis(500) {
            self.send_join();
        }
        let silent = self.snap_at.map_or(self.started, |t| t).elapsed();
        if silent > Duration::from_secs(5) {
            self.host_gone = true;
        }
    }

    pub fn send_input(&mut self, inp: Input) {
        if self.id.is_none() {
            return;
        }
        self.out_seq = self.out_seq.wrapping_add(1);
        let mut w = W::new(T_INPUT);
        w.u16(self.out_seq);
        w.u8((inp.steer.clamp(-1.0, 1.0) * 127.0) as i8 as u8);
        w.u8(inp.throttle as u8 | (inp.brake as u8) << 1 | (inp.drift as u8) << 2 | (inp.flip as u8) << 3);
        w.u8(inp.use_seq);
        w.u8(inp.swap_seq);
        let _ = self.sock.send_to(&w.0, self.host);
    }

    pub fn leave(&self) {
        let _ = self.sock.send_to(&W::new(T_LEAVE).0, self.host);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_join_and_sync_over_loopback() {
        let tr = Track::new();
        let mut gs = GameState::new(&tr);
        gs.add_human(&tr, "HostGuy");
        let mut host = Host::bind(0).expect("bind");
        let port = host.port();

        let mut br = Browser::with_port(port).unwrap();
        br.query();
        std::thread::sleep(Duration::from_millis(50));
        host.poll(&mut gs, &tr);
        std::thread::sleep(Duration::from_millis(50));
        br.poll();
        assert_eq!(br.hosts.len(), 1, "host not discovered");
        assert_eq!(br.hosts[0].name, "HostGuy");
        assert!(br.hosts[0].joinable());

        let addr = br.hosts[0].addr;
        let mut cl = Client::new(br.sock, addr, "Guest", &tr);
        std::thread::sleep(Duration::from_millis(50));
        host.poll(&mut gs, &tr);
        assert_eq!(gs.humans(), 2);
        host.broadcast(&gs);
        std::thread::sleep(Duration::from_millis(50));
        cl.poll();
        assert_eq!(cl.id, Some(1));
        assert_eq!(cl.state.karts.len(), 2);
        assert_eq!(cl.state.karts[1].name, "Guest");

        // race: input goes up, snapshots come down
        gs.start_race(&tr);
        for _ in 0..300 {
            gs.step(&tr);
        }
        cl.send_input(Input { steer: 0.5, throttle: true, use_seq: 3, ..Default::default() });
        std::thread::sleep(Duration::from_millis(50));
        host.poll(&mut gs, &tr);
        assert!(gs.karts[1].input.throttle);
        assert_eq!(gs.karts[1].input.use_seq, 3);
        host.broadcast(&gs);
        std::thread::sleep(Duration::from_millis(50));
        cl.poll();
        assert_eq!(cl.state.phase, Phase::Racing);
        assert_eq!(cl.state.karts.len(), gs.karts.len());
        assert!((cl.state.karts[0].pos.x - gs.karts[0].pos.x).abs() < 1e-3);

        // late joiners are turned away
        let mut late = Client::new(new_socket(0).unwrap(), addr, "Late", &tr);
        std::thread::sleep(Duration::from_millis(50));
        host.poll(&mut gs, &tr);
        std::thread::sleep(Duration::from_millis(50));
        late.poll();
        assert_eq!(late.rejected, Some(1));
    }

    #[test]
    fn snapshot_fits_in_ipv6_min_mtu() {
        let tr = Track::new();
        let mut gs = GameState::new(&tr);
        for i in 0..8 {
            gs.add_human(&tr, &format!("player{i:02}xxxx"));
        }
        gs.start_race(&tr);
        for i in 0..MAX_ENTS {
            gs.ents.push(Ent { kind: EntKind::Peel, pos: V2::ZERO, vel: V2::ZERO, owner: 0, age: 0.0, timer: 0.0, target: 0, run: 0, bounces: 0, rev: false });
            let _ = i;
        }
        let len = encode_snapshot(&gs, 1, 0).len();
        assert!(len < 1232, "snapshot is {len} bytes");
    }
}
