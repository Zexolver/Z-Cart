# Z-Cart

A top-down kart racer in Rust with peer-hosted LAN play over **IPv6 link-local** addresses.
Two views, both software-rendered (no GPU engine): a 3D chase camera (default) and the original 2D top-down view. Switch with **V** on the title screen or in game; no admin/sudo needed on Linux or Windows.

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

## Debug build / test keys
Build with `--features debug-tools` (the arm64 `.deb` release asset is built this way, in the dev profile):

| Key | Action |
|---|---|
| F1 / F2 | add / remove a bot mid-race |
| F3 / F4 | cycle the primary slot through every item |
| F5 | jump to just before the final finish line |
| F6 | max coins |
| F8 | restart the race |

Solo testing: press H, then Enter in the lobby (set bots with B first). The window is freely resizable/maximizable; the
view is scaled to fit while keeping its aspect ratio.

## Packaging
`scripts/package.sh` builds `dist/z-cart_<ver>_arm64.deb` and `dist/z-cart-amd64.exe` (cross-compiled with `zig cc` as linker).

## Tracks
Meadow Circuit, Dune Drift, Frost Ridge and Neon Nights, each with its own scenery and picnic-style clearings. The host picks a track (or **Random**) with **T** in the lobby.

## Aiming items
Left click uses the primary item. Peels and Decoys are dropped behind, Bouncers, Seekers and Bombs go forward. Hold **Space** (or use middle-click) while using an item to throw it the other way: lob a Peel/Decoy forward, or fire a shell/bomb backward. Shells destroy each other on contact, and a Peel or Decoy stops a shell (both break).

## Gameplay notes
* Spin-outs are cosmetic: the kart whirls in place and always ends up facing the way it was going.
* A flashing WRONG WAY warning appears when you're driving against the track direction.
* Using a speed boost while driving onto a boost pad gives a brief extra burst of speed.
