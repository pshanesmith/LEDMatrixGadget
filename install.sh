#!/usr/bin/env bash
# Install the LED matrix display on a Raspberry Pi from a GitHub release (no git, no compiler needed).
#
#   curl -fsSL https://github.com/pshanesmith/LEDMatrixGadget/releases/latest/download/install.sh -o install.sh
#   sudo bash install.sh
#
# It downloads the newest release (a program built for 64-bit Raspberry Pi OS), checks its SHA-256,
# unpacks it and runs the bundled setup/03-install-service.sh, which installs it into /opt/matrix,
# creates /etc/matrix, writes the systemd units and starts the service. Run it again to update.
# Read the script before running it as root: it is short.
#
# Options: --version TAG   install that release instead of the latest (for example v0.2.0)
#          --from-source   clone the repository and compile on the Pi instead (slow; needs build tools;
#                          takes --ref NAME for a branch or tag, default main)
#          --no-start --no-drop --yes   passed on to setup/03-install-service.sh
#          --dry-run       download, verify and unpack, then show what would be installed
# The repository can be changed with MATRIX_REPO=OWNER/NAME (default pshanesmith/LEDMatrixGadget).
set -euo pipefail

REPO=${MATRIX_REPO:-pshanesmith/LEDMatrixGadget}
ASSET=matrix-display-aarch64.tar.gz
VERSION=latest
FROM_SOURCE=0
DRY=0
REF=main
PASS=()

die() { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*"; }

while (( $# )); do
  case $1 in
    --version) VERSION=${2:?--version needs a tag}; shift ;;
    --from-source) FROM_SOURCE=1 ;;
    --ref) REF=${2:?--ref needs a name}; shift ;;
    --dry-run) DRY=1 ;;
    --no-start|--no-drop|--yes) PASS+=("$1") ;;
    -h|--help) sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

# Where the files come from. MATRIX_RELEASE_BASE overrides it (tests use a file:// directory).
if [[ -n ${MATRIX_RELEASE_BASE:-} ]]; then
  BASE=$MATRIX_RELEASE_BASE
elif [[ $VERSION == latest ]]; then
  BASE=https://github.com/$REPO/releases/latest/download
else
  BASE=https://github.com/$REPO/releases/download/$VERSION
fi

if (( ! DRY )); then
  (( EUID == 0 )) || die "run as root: sudo bash $0 $*"
  [[ $(uname -m) == aarch64 ]] || die "this release is for 64-bit Raspberry Pi OS (aarch64); this system is $(uname -m)"
  grep -q "Raspberry Pi" /proc/device-tree/model 2>/dev/null || die "this does not look like a Raspberry Pi"
fi
command -v curl >/dev/null || die "curl is needed (sudo apt-get install curl)"

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

if (( FROM_SOURCE )); then
  if ! command -v git >/dev/null; then
    (( DRY )) && die "git is needed for --from-source (sudo apt-get install git)"
    info "installing git (needed to fetch the source)"
    DEBIAN_FRONTEND=noninteractive apt-get -y install git
  fi
  info "cloning https://github.com/$REPO ($REF)"
  git clone --quiet --depth 1 --branch "$REF" "https://github.com/$REPO.git" "$TMP/src"
  if (( DRY )); then info "dry run: would run $TMP/src/setup/03-install-service.sh --apply --repo https://github.com/$REPO.git --ref $REF ${PASS[*]:-}"; exit 0; fi
  # The checkout is only the installer: 03 clones into /opt/matrix itself and builds there.
  exec "$TMP/src/setup/03-install-service.sh" --apply --repo "https://github.com/$REPO.git" --ref "$REF" ${PASS[@]+"${PASS[@]}"}
fi

info "downloading $ASSET ($VERSION) from $BASE"
curl -fsSL "$BASE/$ASSET" -o "$TMP/$ASSET" || die "download failed: $BASE/$ASSET (is there a release yet?)"
curl -fsSL "$BASE/$ASSET.sha256" -o "$TMP/$ASSET.sha256" || die "download failed: $BASE/$ASSET.sha256"
info "checking the checksum"
if command -v sha256sum >/dev/null; then (cd "$TMP" && sha256sum -c "$ASSET.sha256" >/dev/null) || die "checksum mismatch: the download is corrupt or was tampered with"
else (cd "$TMP" && shasum -a 256 -c "$ASSET.sha256" >/dev/null) || die "checksum mismatch: the download is corrupt or was tampered with"
fi
tar -C "$TMP" -xzf "$TMP/$ASSET"
REL=$TMP/${ASSET%.tar.gz}
[[ -x $REL/setup/03-install-service.sh && -f $REL/VERSION ]] || die "the archive does not look like a release"
info "release $(cat "$REL/VERSION")"

if (( DRY )); then
  info "dry run: would run $REL/setup/03-install-service.sh --release $REL --apply ${PASS[*]:-}"
  echo "contents:"; (cd "$REL" && find . -type f | sort | sed 's/^/  /')
  exit 0
fi
"$REL/setup/03-install-service.sh" --release "$REL" --apply ${PASS[@]+"${PASS[@]}"}
