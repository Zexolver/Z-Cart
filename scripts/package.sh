#!/bin/sh
# Builds dist/z-cart_<ver>_arm64.deb (debug build + debug tools) and dist/z-cart-amd64.exe.
# Cross-compiling uses `zig cc` (pip install ziglang) as the linker so the binaries stay
# compatible with older glibc / need no MinGW install. See the wrappers described below.
set -e
cd "$(dirname "$0")/.."
VER=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
mkdir -p dist

# --- arm64 .deb (debug profile, debug-tools feature, glibc >= 2.31)
cat > /tmp/zig-aarch64-gnu <<'EOS'
#!/bin/sh
exec python3 -m ziglang cc -target aarch64-linux-gnu.2.31 "$@"
EOS
chmod +x /tmp/zig-aarch64-gnu
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=/tmp/zig-aarch64-gnu \
  cargo build --target aarch64-unknown-linux-gnu --features debug-tools
D=$(mktemp -d)/z-cart_${VER}_arm64
mkdir -p "$D/DEBIAN" "$D/usr/bin" "$D/usr/share/applications"
install -m755 target/aarch64-unknown-linux-gnu/debug/z-cart "$D/usr/bin/z-cart"
cat > "$D/DEBIAN/control" <<EOC
Package: z-cart
Version: $VER
Architecture: arm64
Maintainer: Zexolver <Zexolver@disroot.org>
Depends: libc6 (>= 2.31), libgcc-s1
Recommends: libx11-6, libxcursor1, libxkbcommon0, libwayland-client0
Section: games
Priority: optional
Description: Z-Cart - LAN kart racer (debug build)
 Top-down kart racer with peer-hosted IPv6 link-local LAN play.
 This is a debug build with extra test keys (F1 add bot, F2 remove bot,
 F3/F4 cycle item, F5 jump to last lap, F6 max coins, F8 restart).
EOC
cat > "$D/usr/share/applications/z-cart.desktop" <<EOC
[Desktop Entry]
Type=Application
Name=Z-Cart
Exec=z-cart
Categories=Game;
Terminal=false
EOC
dpkg-deb --root-owner-group --build "$D" "dist/z-cart_${VER}_arm64.deb"

# --- Windows exe (release). Needs x86_64-w64-mingw32-{gcc,dlltool} on PATH; with zig these are
# thin wrappers around `python3 -m ziglang cc -target x86_64-windows-gnu` / `ziglang dlltool`
# (the gcc wrapper must drop -lmsvcrt / -l:*.a and turn -lwinapi_* into the libwinapi_*.a path).
cargo build --release --target x86_64-pc-windows-gnu
cp target/x86_64-pc-windows-gnu/release/z-cart.exe dist/z-cart-amd64.exe
