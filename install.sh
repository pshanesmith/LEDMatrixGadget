#!/usr/bin/env bash
# Install the LED matrix display on a Raspberry Pi from a GitHub release (no git, no compiler needed).
#
#   curl -fsSL https://github.com/pshanesmith/LEDMatrixGadget/releases/latest/download/install.sh | sudo bash
#
# Options go after `-s --`, for example:  ... | sudo bash -s -- --version v0.2.1 --yes
# Or download first and read it (it runs as root):  curl -fsSLO <url>/install.sh && less install.sh && sudo bash install.sh
#
# It downloads the newest release (a program built for 64-bit Raspberry Pi OS), checks its SHA-256,
# unpacks it and runs the bundled setup/03-install-service.sh, which installs it into /opt/matrix,
# creates /etc/matrix, writes the systemd units and starts the service. Run it again to update.
#
# Options: --version TAG   install that release instead of the latest (for example v0.2.1)
#          --from-source   clone the repository and compile on the Pi instead (slow; installs the build
#                          tools if missing; takes --ref NAME for a branch or tag, default main)
#          --no-start --no-drop --yes   passed on to setup/03-install-service.sh
#          --dry-run       download, verify and unpack, then show what would be installed
# The repository can be changed with MATRIX_REPO=OWNER/NAME (default pshanesmith/LEDMatrixGadget).
#
# Everything is inside main(), which runs only after the whole script has been read: a download that is cut
# short can never execute half of it.

die() { echo "error: $*" >&2; exit 1; }
info() { echo "==> $*"; }

usage() {
  echo "usage: curl -fsSL <release-url>/install.sh | sudo bash -s -- [--version TAG] [--from-source [--ref NAME]]"
  echo "                                                              [--no-start] [--no-drop] [--yes] [--dry-run]"
  echo "See the top of install.sh (or README.md) for what each option does."
}

main() {
  set -euo pipefail
  local REPO=${MATRIX_REPO:-pshanesmith/LEDMatrixGadget}
  local ASSET=matrix-display-aarch64.tar.gz
  local VERSION=latest FROM_SOURCE=0 DRY=0 REF=main
  local PASS=()
  while (( $# )); do
    case $1 in
      --version) VERSION=${2:?--version needs a tag}; shift ;;
      --from-source) FROM_SOURCE=1 ;;
      --ref) REF=${2:?--ref needs a name}; shift ;;
      --dry-run) DRY=1 ;;
      --no-start|--no-drop|--yes) PASS+=("$1") ;;
      -h|--help) usage; return 0 ;;
      *) usage >&2; die "unknown option: $1" ;;
    esac
    shift
  done

  # Where the files come from. MATRIX_RELEASE_BASE overrides it (tests use a file:// directory).
  local BASE
  if [[ -n ${MATRIX_RELEASE_BASE:-} ]]; then
    BASE=$MATRIX_RELEASE_BASE
  elif [[ $VERSION == latest ]]; then
    BASE=https://github.com/$REPO/releases/latest/download
  else
    BASE=https://github.com/$REPO/releases/download/$VERSION
  fi

  if (( ! DRY )); then
    (( EUID == 0 )) || die "run as root: curl -fsSL <url>/install.sh | sudo bash   (or: sudo bash install.sh)"
    [[ $(uname -m) == aarch64 ]] || die "this release is for 64-bit Raspberry Pi OS (aarch64); this system is $(uname -m)"
    grep -q "Raspberry Pi" /proc/device-tree/model 2>/dev/null || die "this does not look like a Raspberry Pi"
  fi
  command -v curl >/dev/null || die "curl is needed (sudo apt-get install curl)"

  # When this script is piped into bash, stdin is the script itself. The installer's questions must not
  # read it: use the terminal if there is one, otherwise answer them (--yes) and read nothing.
  local INPUT=/dev/null
  if [[ -t 0 ]]; then
    INPUT=/dev/stdin
  elif ( : </dev/tty ) 2>/dev/null; then
    INPUT=/dev/tty
  else
    PASS+=(--yes)
  fi

  # TMP is global on purpose: the EXIT trap runs after main() has returned.
  TMP=$(mktemp -d)
  trap 'rm -rf "$TMP"' EXIT

  if (( FROM_SOURCE )); then
    if ! command -v git >/dev/null; then
      (( DRY )) && die "git is needed for --from-source (sudo apt-get install git)"
      info "installing git (needed to fetch the source)"
      DEBIAN_FRONTEND=noninteractive apt-get -y install git </dev/null
    fi
    info "cloning https://github.com/$REPO ($REF)"
    git clone --quiet --depth 1 --branch "$REF" "https://github.com/$REPO.git" "$TMP/src" </dev/null
    if (( DRY )); then info "dry run: would run $TMP/src/setup/03-install-service.sh --apply --repo https://github.com/$REPO.git --ref $REF ${PASS[*]:-}"; return 0; fi
    # The checkout is only the installer: 03 clones into /opt/matrix itself and builds there. No exec: the
    # temporary directory is removed when this script ends.
    "$TMP/src/setup/03-install-service.sh" --apply --repo "https://github.com/$REPO.git" --ref "$REF" ${PASS[@]+"${PASS[@]}"} <"$INPUT"
    return
  fi

  info "downloading $ASSET ($VERSION) from $BASE"
  curl -fsSL "$BASE/$ASSET" -o "$TMP/$ASSET" || die "download failed: $BASE/$ASSET (is there a release yet?)"
  curl -fsSL "$BASE/$ASSET.sha256" -o "$TMP/$ASSET.sha256" || die "download failed: $BASE/$ASSET.sha256"
  info "checking the checksum"
  if command -v sha256sum >/dev/null; then (cd "$TMP" && sha256sum -c "$ASSET.sha256" >/dev/null) || die "checksum mismatch: the download is corrupt or was tampered with"
  else (cd "$TMP" && shasum -a 256 -c "$ASSET.sha256" >/dev/null) || die "checksum mismatch: the download is corrupt or was tampered with"
  fi
  tar -C "$TMP" -xzf "$TMP/$ASSET"
  local REL=$TMP/${ASSET%.tar.gz}
  [[ -x $REL/setup/03-install-service.sh && -f $REL/VERSION ]] || die "the archive does not look like a release"
  info "release $(cat "$REL/VERSION")"

  if (( DRY )); then
    info "dry run: would run $REL/setup/03-install-service.sh --release $REL --apply ${PASS[*]:-}"
    echo "contents:"; (cd "$REL" && find . -type f | sort | sed 's/^/  /')
    return 0
  fi
  "$REL/setup/03-install-service.sh" --release "$REL" --apply ${PASS[@]+"${PASS[@]}"} <"$INPUT"
}

main "$@"
