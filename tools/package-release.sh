#!/usr/bin/env bash
# Pack a release for the Pi: the cross-built programs plus everything install.sh needs on the device.
#
#   ./tools/package-release.sh [--bin-dir dist] [--out dist] [--version X.Y.Z]
#
# Writes <out>/matrix-display-aarch64.tar.gz, its .sha256, and a copy of install.sh. The file names
# never change (the version lives in the tarball's VERSION file), so
#   https://github.com/OWNER/REPO/releases/latest/download/matrix-display-aarch64.tar.gz
# always points at the newest release. The binaries come from tools/cross-build.sh; the GitHub
# Actions release workflow runs both steps, and so can you on a Mac.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
BIN_DIR=$ROOT/dist
OUT=
VERSION=
while (( $# )); do
  case $1 in
    --bin-dir) BIN_DIR=$2; shift ;;
    --out) OUT=$2; shift ;;
    --version) VERSION=$2; shift ;;
    -h|--help) sed -n '2,11p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done
OUT=${OUT:-$BIN_DIR}
[[ -n $VERSION ]] || VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
[[ -n $VERSION ]] || { echo "cannot find the version in Cargo.toml" >&2; exit 1; }

PROGRAMS=(matrix-display matrix-hello matrix-geocode matrix-set-config matrix-ha-discover)
for p in "${PROGRAMS[@]}"; do
  [[ -f $BIN_DIR/$p ]] || { echo "missing $BIN_DIR/$p (run tools/cross-build.sh first)" >&2; exit 1; }
done

NAME=matrix-display-aarch64
STAGE=$(mktemp -d)
trap 'rm -rf "$STAGE"' EXIT
PKG=$STAGE/$NAME
mkdir -p "$PKG/bin" "$PKG/setup/lib" "$PKG/systemd" "$PKG/config" "$PKG/tools" "$PKG/docs"
for p in "${PROGRAMS[@]}"; do install -m 755 "$BIN_DIR/$p" "$PKG/bin/$p"; done
install -m 755 "$ROOT"/setup/0*.sh "$PKG/setup/"
install -m 644 "$ROOT/setup/lib/common.sh" "$PKG/setup/lib/"
install -m 644 "$ROOT"/systemd/* "$PKG/systemd/"
install -m 644 "$ROOT/config/config.example.toml" "$ROOT/config/secrets.env.example" "$PKG/config/"
install -m 755 "$ROOT/tools/health-check.sh" "$ROOT/tools/soak-report.sh" "$ROOT/tools/test-matrix.sh" "$PKG/tools/"
install -m 644 "$ROOT/docs/operations.md" "$ROOT/docs/hardware.md" "$PKG/docs/"
install -m 644 "$ROOT/README.md" "$ROOT/DESIGN.md" "$ROOT/LICENSE" "$PKG/"
printf '%s\n' "$VERSION" > "$PKG/VERSION"

mkdir -p "$OUT"
export COPYFILE_DISABLE=1   # no macOS ._ files in the archive
if tar --version 2>/dev/null | grep -q GNU; then OWNER=(--owner=0 --group=0); else OWNER=(--uid 0 --gid 0); fi
tar -C "$STAGE" "${OWNER[@]}" -czf "$OUT/$NAME.tar.gz" "$NAME"
if command -v sha256sum >/dev/null; then SUM=(sha256sum); else SUM=(shasum -a 256); fi
(cd "$OUT" && "${SUM[@]}" "$NAME.tar.gz" > "$NAME.tar.gz.sha256")
install -m 755 "$ROOT/install.sh" "$OUT/install.sh"

echo "version $VERSION"
ls -l "$OUT/$NAME.tar.gz" "$OUT/$NAME.tar.gz.sha256" "$OUT/install.sh"
