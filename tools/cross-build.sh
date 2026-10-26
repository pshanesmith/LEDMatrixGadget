#!/usr/bin/env bash
# Build the display for the Raspberry Pi on this (faster) machine instead of on the Pi's 460 MB of RAM.
#
#   ./tools/cross-build.sh                       glibc build for Raspberry Pi OS  -> dist/matrix-display
#   ./tools/cross-build.sh --target aarch64-unknown-linux-musl    static build (see the DNS note below)
#
# Then copy it to the Pi and install it:
#   scp dist/matrix-display dist/matrix-hello pi:/tmp/
#   ssh pi 'sudo /opt/matrix/setup/03-install-service.sh --apply --binary /tmp/matrix-display'
#
# Needs: rustup, zig (brew install zig), cargo-zigbuild (brew install cargo-zigbuild) and the target
# (rustup target add aarch64-unknown-linux-gnu). The panel driver (rpi-led-panel) is pure Rust, so zig
# only has to supply the C compiler and linker for a few dependencies (TLS).
#
# glibc is built against 2.36 (Debian 12), so the binary runs on Debian 12 and 13 alike. The musl
# build is fully static, but musl's resolver cannot use mDNS (".local" names); use glibc if Home
# Assistant is reached that way.
set -euo pipefail

TARGET=aarch64-unknown-linux-gnu
while (( $# )); do
  case $1 in
    --target) TARGET=$2; shift ;;
    -h|--help) sed -n '2,15p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

command -v zig >/dev/null || python3 -m ziglang version >/dev/null 2>&1 || { echo "zig not found (brew install zig, or pip install ziglang)" >&2; exit 1; }
command -v cargo-zigbuild >/dev/null || { echo "cargo-zigbuild not found (brew install cargo-zigbuild)" >&2; exit 1; }
rustup target list --installed | grep -qx "$TARGET" || { echo "missing target: rustup target add $TARGET" >&2; exit 1; }

case $TARGET in
  *-gnu) ZIG_SUFFIX=.2.36 ;;   # target glibc version
  *-musl) ZIG_SUFFIX= ;;
  *) echo "unsupported target $TARGET" >&2; exit 2 ;;
esac

cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo zigbuild --release --locked --target "$TARGET$ZIG_SUFFIX" --features hardware --bins

mkdir -p dist
for bin in matrix-display matrix-hello matrix-geocode matrix-set-config matrix-ha-discover; do
  cp "target/$TARGET/release/$bin" "dist/$bin"
done
file dist/matrix-display
ls -l dist
