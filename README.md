# Z-Cart

A top-down kart racer in Rust with peer-hosted LAN play over **IPv6 link-local** addresses.
Software-rendered (no GPU engine); no admin/sudo needed on Linux or Windows.

Dependencies: `minifb` (window), `if-addrs` (interface list), `font8x8` (font data). Networking is `std::net` only.

## Run
    cargo run --release

## Controls
| Action | Input |
|---|---|
| Drive | W A S D / arrow keys |
| Drift (mini-turbo on release) | hold Shift while steering |
| Use primary item | Left click |
| Swap primary/secondary | Right click |

## Multiplayer
* **H** on the title screen hosts (UDP port 47777); the host also runs the simulation and can add bots (**B**) / change laps (**L**) in the lobby.
* **J** lists games found on the LAN (link-local multicast `ff02::1` discovery). Enter joins.
* Only `fe80::/10` link-local (and loopback, for local testing) peers are accepted.
* Windows may show a firewall prompt the first time you host; allow it on *Private* networks (no admin required for the game itself).
* Up to 8 karts (players + bots). Late joiners can't enter a race in progress.

## Items
| Item | Behaviour |
|---|---|
| Peel (x3) | banana: drop behind, spins whoever hits it |
| Bouncer (x3) | green shell: fires straight, bounces off walls |
| Seeker | red shell: follows the track, homes in on the kart ahead |
| Nova | blue shell: hunts the race leader, area blast |
| Turbo (x3) | speed mushroom |
| Nitro | golden mushroom: 5 boosts |
| Giant | mega mushroom: huge, crushes karts and hazards |
| Star | invincible, faster, knocks karts over |
| Bomb | thrown; explodes on contact or after a fuse |
| Decoy | fake item box |
| Zap | lightning: spins and shrinks everyone else |
| Rocket | bullet: auto-drives at top speed, invincible |
| Ink | blinds everyone ahead of you |

Also: coins (+top speed), boost pads, drift mini-turbos (blue/orange/purple sparks).

## Tests / debugging
    cargo test --release          # sim, track, and loopback networking tests
    cargo run --release -- --shot frame.ppm   # render one frame headlessly
