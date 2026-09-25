//! LAN networking: plain UDP over IPv6 link-local addresses, no admin rights
//! needed. One peer hosts (and simulates); the others send inputs and render
//! the snapshots they receive. Hosts are found via link-local multicast
//! (ff02::1) discovery.

use crate::math::*;
use crate::sim::*;
use crate::track::Track;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
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

fn lan_v4(ip: &Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

/// Only local-network peers are served: IPv6 link-local / loopback, and private IPv4 ranges.
pub fn is_lan_addr(a: &SocketAddr) -> bool {
    match a {
        SocketAddr::V6(v) => {
            let ip = v.ip();
            ip.is_loopback() || (ip.segments()[0] & 0xffc0) == 0xfe80 || ip.to_ipv4_mapped().map_or(false, |m| lan_v4(&m))
        }
        SocketAddr::V4(v) => lan_v4(v.ip()),
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

// ---------------------------------------------------------------- sockets

/// One IPv6 and one IPv4 UDP socket, both non-blocking. Either may be missing.
pub struct Socks {
    v6: Option<UdpSocket>,
    v4: Option<UdpSocket>,
}

fn bind6(port: u16) -> std::io::Result<UdpSocket> {
    let s = UdpSocket::bind(SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, port, 0, 0)))?;
    s.set_nonblocking(true)?;
    Ok(s)
}

fn bind4(port: u16) -> std::io::Result<UdpSocket> {
    let s = UdpSocket::bind(SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port)))?;
    s.set_nonblocking(true)?;
    let _ = s.set_broadcast(true);
    Ok(s)
}

impl Socks {
    pub fn bind(port: u16) -> std::io::Result<Socks> {
        let v6 = bind6(port);
        let p = v6.as_ref().ok().and_then(|s| s.local_addr().ok()).map_or(port, |a| a.port());
        // On Linux the IPv6 socket is dual-stack, so this can fail with "in use"; that's fine.
        let v4 = bind4(p);
        match (v6, v4) {
            (Err(e), Err(_)) => Err(e),
            (a, b) => Ok(Socks { v6: a.ok(), v4: b.ok() }),
        }
    }

    /// Two independent sockets on any free ports (clients and the browser).
    pub fn bind_ephemeral() -> std::io::Result<Socks> {
        match (bind6(0), bind4(0)) {
            (Err(e), Err(_)) => Err(e),
            (a, b) => Ok(Socks { v6: a.ok(), v4: b.ok() }),
        }
    }

    pub fn port(&self) -> u16 {
        self.v6.as_ref().or(self.v4.as_ref()).and_then(|s| s.local_addr().ok()).map_or(0, |a| a.port())
    }

    pub fn send(&self, buf: &[u8], to: SocketAddr) {
        match to {
            SocketAddr::V6(_) => {
                if let Some(s) = &self.v6 {
                    let _ = s.send_to(buf, to);
                }
            }
            SocketAddr::V4(a) => {
                if let Some(s) = &self.v4 {
                    let _ = s.send_to(buf, to);
                } else if let Some(s) = &self.v6 {
                    let mapped = SocketAddr::V6(SocketAddrV6::new(a.ip().to_ipv6_mapped(), a.port(), 0, 0));
                    let _ = s.send_to(buf, mapped);
                }
            }
        }
    }

    /// Next pending datagram from either socket.
    pub fn recv(&self, buf: &mut [u8]) -> Option<(usize, SocketAddr)> {
        for s in [&self.v6, &self.v4].into_iter().flatten() {
            for _ in 0..8 {
                match s.recv_from(buf) {
                    Ok((n, from)) => {
                        // a dual-stack IPv6 socket reports IPv4 peers in mapped form; undo that
                        let from = match from {
                            SocketAddr::V6(a) => match a.ip().to_ipv4_mapped() {
                                Some(m) => SocketAddr::V4(SocketAddrV4::new(m, a.port())),
                                None => from,
                            },
                            v4 => v4,
                        };
                        return Some((n, from));
                    }
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(_) => continue, // e.g. Windows "connection reset" from an earlier send
                }
            }
        }
        None
    }
}

// ---------------------------------------------------------------- discovery

#[derive(Clone, Debug)]
pub struct HostInfo {
    /// Every address this host answered from (IPv6 link-local first, then IPv4).
    pub addrs: Vec<SocketAddr>,
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

/// Directed-broadcast addresses of the local IPv4 networks.
fn v4_broadcasts() -> Vec<Ipv4Addr> {
    let mut v: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(a) if !a.ip.is_loopback() && lan_v4(&a.ip) => {
                Some(a.broadcast.unwrap_or_else(|| Ipv4Addr::from(u32::from(a.ip) | !u32::from(a.netmask))))
            }
            _ => None,
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

pub struct Browser {
    pub socks: Socks,
    pub ifaces: Vec<u32>,
    pub v4_nets: usize,
    /// Discovery replies received so far (diagnostics).
    pub replies: u32,
    pub hosts: Vec<HostInfo>,
    last_query: Option<Instant>,
    port: u16,
}

impl Browser {
    pub fn new() -> std::io::Result<Browser> {
        Browser::with_port(PORT)
    }

    pub fn with_port(port: u16) -> std::io::Result<Browser> {
        Ok(Browser { socks: Socks::bind_ephemeral()?, ifaces: link_local_ifaces(), v4_nets: 0, replies: 0, hosts: Vec::new(), last_query: None, port })
    }

    pub fn query(&mut self) {
        self.ifaces = link_local_ifaces();
        let pkt = W::new(T_QUERY).0;
        // IPv6 link-local: all-nodes multicast on every interface
        let all_nodes = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);
        for &idx in &self.ifaces {
            self.socks.send(&pkt, SocketAddr::V6(SocketAddrV6::new(all_nodes, self.port, 0, idx)));
        }
        // IPv4 LAN broadcast, in case link-local multicast is filtered
        let nets = v4_broadcasts();
        self.v4_nets = nets.len();
        for b in nets {
            self.socks.send(&pkt, SocketAddr::V4(SocketAddrV4::new(b, self.port)));
        }
        self.socks.send(&pkt, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::BROADCAST, self.port)));
        // same-machine hosts
        self.socks.send(&pkt, SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, self.port, 0, 0)));
        self.socks.send(&pkt, SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port)));
        self.last_query = Some(Instant::now());
    }

    /// Re-query every couple of seconds, collect replies, expire stale hosts.
    pub fn poll(&mut self) {
        if self.last_query.map_or(true, |t| t.elapsed() > Duration::from_secs(2)) {
            self.query();
        }
        let mut buf = [0u8; 512];
        for _ in 0..64 {
            let Some((n, from)) = self.socks.recv(&mut buf) else { break };
            if !is_lan_addr(&from) {
                continue;
            }
            if let Some((T_REPLY, mut r)) = parse(&buf[..n]) {
                self.replies += 1;
                let parsed = (|| Some((Phase::from_u8(r.u8()?), r.u8()?, r.u8()?, r.str()?)))();
                let Some((phase, players, max, name)) = parsed else { continue };
                match self.hosts.iter_mut().find(|h| h.name == name) {
                    Some(h) => {
                        h.phase = phase;
                        h.players = players;
                        h.max = max;
                        h.seen = Instant::now();
                        if !h.addrs.contains(&from) {
                            h.addrs.push(from);
                            // IPv6 first: it needs no configuration
                            h.addrs.sort_by_key(|a| a.is_ipv4());
                        }
                    }
                    None => self.hosts.push(HostInfo { addrs: vec![from], name, players, max, phase, seen: Instant::now() }),
                }
            }
        }
        self.hosts.retain(|h| h.seen.elapsed() < Duration::from_secs(6));
    }
}

/// This machine's LAN addresses as friends should type them ("fe80::1%17", "192.168.1.5").
pub fn local_addr_strings() -> Vec<String> {
    let mut out = Vec::new();
    for i in if_addrs::get_if_addrs().unwrap_or_default() {
        match i.addr {
            if_addrs::IfAddr::V6(a) if (a.ip.segments()[0] & 0xffc0) == 0xfe80 => {
                out.push(format!("{}%{}", a.ip, i.index.map_or(i.name.clone(), |x| x.to_string())));
            }
            if_addrs::IfAddr::V4(a) if !a.ip.is_loopback() && lan_v4(&a.ip) => out.push(a.ip.to_string()),
            _ => {}
        }
    }
    out
}

/// Parses "192.168.1.5", "fe80::1%17", "fe80::1%eth0" or "[fe80::1%17]:47777"
/// (zone = interface number or name). Only LAN addresses are accepted.
pub fn parse_addr(text: &str) -> Result<SocketAddr, String> {
    let t = text.trim();
    let (host, port) = match t.strip_prefix('[') {
        Some(rest) => {
            let (h, tail) = rest.split_once(']').ok_or("missing ]")?;
            let port = match tail.strip_prefix(':') {
                Some(p) => p.parse::<u16>().map_err(|_| "bad port")?,
                None => PORT,
            };
            (h, port)
        }
        None if t.matches(':').count() == 1 => {
            let (h, p) = t.split_once(':').unwrap();
            (h, p.parse::<u16>().map_err(|_| "bad port")?)
        }
        None => (t, PORT),
    };
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        return if lan_v4(&v4) {
            Ok(SocketAddr::V4(SocketAddrV4::new(v4, port)))
        } else {
            Err("only local-network IPv4 addresses (192.168.x.x, 10.x.x.x, ...) are supported".into())
        };
    }
    let (ip_s, zone_s) = match host.split_once('%') {
        Some((a, z)) => (a, Some(z)),
        None => (host, None),
    };
    let ip: Ipv6Addr = ip_s.parse().map_err(|_| "not an IPv4 or IPv6 address".to_string())?;
    let link_local = (ip.segments()[0] & 0xffc0) == 0xfe80;
    if !link_local && !ip.is_loopback() {
        return Err("only fe80:: link-local IPv6 addresses are supported".into());
    }
    let zone = match zone_s {
        Some(z) => match z.parse::<u32>() {
            Ok(n) => n,
            Err(_) => if_addrs::get_if_addrs()
                .unwrap_or_default()
                .into_iter()
                .find(|i| i.name == z)
                .and_then(|i| i.index)
                .ok_or_else(|| format!("unknown interface '{z}'"))?,
        },
        None if link_local => return Err("add the interface after %, e.g. fe80::1%17".into()),
        None => 0,
    };
    Ok(SocketAddr::V6(SocketAddrV6::new(ip, port, 0, zone)))
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
    socks: Socks,
    pub peers: Vec<Peer>,
    seq: u32,
}

impl Host {
    pub fn bind(port: u16) -> std::io::Result<Host> {
        Ok(Host { socks: Socks::bind(port)?, peers: Vec::new(), seq: 0 })
    }

    #[cfg(test)]
    pub fn port(&self) -> u16 {
        self.socks.port()
    }

    pub fn poll(&mut self, gs: &mut GameState, tr: &Track) {
        let mut buf = [0u8; 512];
        for _ in 0..512 {
            let Some((n, from)) = self.socks.recv(&mut buf) else { break };
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
                    self.socks.send(&w.0, from);
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
                            back: flags & 8 != 0,
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
        self.socks.send(&w.0, to);
    }

    fn reject(&self, to: SocketAddr, why: u8) {
        let mut w = W::new(T_REJECT);
        w.u8(why);
        self.socks.send(&w.0, to);
    }

    pub fn broadcast(&mut self, gs: &GameState) {
        self.seq = self.seq.wrapping_add(1);
        for p in &self.peers {
            let pkt = encode_snapshot(gs, self.seq, p.kart);
            self.socks.send(&pkt, p.addr);
        }
    }

    /// Tell everyone the session is over.
    pub fn shutdown(&mut self) {
        let pkt = W::new(T_LEAVE).0;
        for p in &self.peers {
            self.socks.send(&pkt, p.addr);
        }
    }
}

// ---------------------------------------------------------------- client

pub struct Client {
    socks: Socks,
    /// Addresses to try for the host (e.g. its link-local IPv6 first, then its IPv4).
    cands: Vec<SocketAddr>,
    cur: usize,
    locked: bool,
    last_switch: Instant,
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
    pub fn new(cands: Vec<SocketAddr>, name: &str, tr: &Track) -> std::io::Result<Client> {
        let mut c = Client {
            socks: Socks::bind_ephemeral()?,
            cands,
            cur: 0,
            locked: false,
            last_switch: Instant::now(),
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
        Ok(c)
    }

    fn host(&self) -> SocketAddr {
        self.cands[self.cur]
    }

    fn send_join(&mut self) {
        let mut w = W::new(T_JOIN);
        w.str(&self.name);
        self.socks.send(&w.0, self.host());
        self.last_join = Instant::now();
    }

    pub fn poll(&mut self) {
        let mut buf = [0u8; 2048];
        for _ in 0..256 {
            let Some((n, from)) = self.socks.recv(&mut buf) else { break };
            let Some(ci) = self.cands.iter().position(|c| *c == from) else { continue };
            if !self.locked {
                // first address that answers wins
                self.locked = true;
                self.cur = ci;
            }
            if ci != self.cur {
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
        if self.id.is_none() && self.rejected.is_none() {
            if !self.locked && self.cands.len() > 1 && self.last_switch.elapsed() > Duration::from_millis(1200) {
                self.cur = (self.cur + 1) % self.cands.len();
                self.last_switch = Instant::now();
                self.send_join();
            } else if self.last_join.elapsed() > Duration::from_millis(400) {
                self.send_join();
            }
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
        w.u8(inp.throttle as u8 | (inp.brake as u8) << 1 | (inp.drift as u8) << 2 | (inp.back as u8) << 3);
        w.u8(inp.use_seq);
        w.u8(inp.swap_seq);
        self.socks.send(&w.0, self.host());
    }

    pub fn leave(&self) {
        self.socks.send(&W::new(T_LEAVE).0, self.host());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_session(via_v4: bool) {
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

        // pick the address family under test
        let addrs = br.hosts[0].addrs.clone();
        let cands: Vec<SocketAddr> = if via_v4 {
            vec![SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))]
        } else {
            addrs.iter().copied().filter(|a| a.is_ipv6()).collect()
        };
        assert!(!cands.is_empty());
        let mut cl = Client::new(cands.clone(), "Guest", &tr).unwrap();
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
        let mut late = Client::new(cands, "Late", &tr).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        host.poll(&mut gs, &tr);
        std::thread::sleep(Duration::from_millis(50));
        late.poll();
        assert_eq!(late.rejected, Some(1));
    }

    #[test]
    fn discover_join_and_sync_over_ipv6() {
        run_session(false);
    }

    #[test]
    fn join_and_sync_over_ipv4() {
        run_session(true);
    }

    #[test]
    fn client_falls_back_to_second_address() {
        let tr = Track::new();
        let mut gs = GameState::new(&tr);
        gs.add_human(&tr, "HostGuy");
        let mut host = Host::bind(0).expect("bind");
        let port = host.port();
        // first candidate goes nowhere (nothing listens on that port), second is the host
        let dead = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, if port == 1 { 2 } else { 1 }, 0, 0));
        let live = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
        let mut cl = Client::new(vec![dead, live], "Guest", &tr).unwrap();
        for _ in 0..40 {
            std::thread::sleep(Duration::from_millis(50));
            host.poll(&mut gs, &tr);
            host.broadcast(&gs);
            cl.poll();
            if cl.id.is_some() {
                break;
            }
        }
        assert_eq!(cl.id, Some(1), "client never reached the host through its fallback address");
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

    #[test]
    fn parses_typed_addresses() {
        let a = parse_addr("fe80::845b:a627:1a5e:9af4%17").unwrap();
        assert_eq!(a.port(), PORT);
        match a {
            SocketAddr::V6(v) => assert_eq!(v.scope_id(), 17),
            _ => panic!(),
        }
        let b = parse_addr(" [fe80::1%3]:5000 ").unwrap();
        assert_eq!(b.port(), 5000);
        assert!(parse_addr("fe80::1").is_err(), "zone required");
        assert!(parse_addr("2001:db8::1").is_err());
        assert!(parse_addr("::1").is_ok());
        assert_eq!(parse_addr("192.168.1.20").unwrap(), "192.168.1.20:47777".parse::<SocketAddr>().unwrap());
        assert_eq!(parse_addr("10.0.0.5:5000").unwrap().port(), 5000);
        assert!(parse_addr("8.8.8.8").is_err(), "public addresses are refused");
    }
}

