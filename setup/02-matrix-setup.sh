#!/usr/bin/env bash
# Install the hzeller rpi-rgb-led-matrix library and Python bindings, free the
# hardware from the sound driver, and reserve a CPU core for the panel refresh.
# Automates https://learn.adafruit.com/adafruit-rgb-matrix-bonnet-for-raspberry-pi/install-manually
# (updated for the library's 2026 CMake/pip build and for Debian 13).
#
#   ./setup/02-matrix-setup.sh                 dry run (default)
#   sudo ./setup/02-matrix-setup.sh --apply    do it
#
# Options: --yes  --only a,b  --skip a,b
#          --venv PATH   virtualenv that receives the bindings (default: <repo>/.venv)
#          --src PATH    where the library is cloned (default: <repo>/vendor/rpi-rgb-led-matrix)
#          --no-isolcpus keep all four cores for the OS
# Steps:   deps library python boot verify
# Never reboots. Safe to run repeatedly.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(dirname "$HERE")
# shellcheck source=lib/common.sh
source "$HERE/lib/common.sh"

BOOT_DIR=${BOOT_DIR:-/boot/firmware}
# Pinned upstream commit (tested 2026-10-23). Bump deliberately.
RGBMATRIX_REPO=${RGBMATRIX_REPO:-https://github.com/hzeller/rpi-rgb-led-matrix}
RGBMATRIX_REF=${RGBMATRIX_REF:-14464b3ad8974665f75a3c82355cf872311fec21}
SRC=${SRC:-$REPO/vendor/rpi-rgb-led-matrix}
VENV=${VENV:-$REPO/.venv}
ISOLCPUS=1; ASSUME_YES=0; ONLY=""; SKIP=""
ALL_STEPS=(deps library python boot verify)
# python3-pil matters twice: it ships Pillow's private Imaging.h (the bindings' C shim reads Pillow's
# in-memory image struct), and the bindings must run against the SAME Pillow those headers came from.
DEPS=(git python3-dev python3-venv python3-pil cython3 build-essential)
# hzeller's recommendation for reserving core 3 for the refresh thread.
CMDLINE_TOKENS=("isolcpus=domain,managed_irq,3" "nohz_full=3" "rcu_nocbs=3" "irqaffinity=0,1,2")

while (( $# )); do
  case $1 in
    --apply) DRY_RUN=0 ;;
    --yes) ASSUME_YES=1 ;;
    --only) ONLY=$2; shift ;;
    --skip) SKIP=$2; shift ;;
    --venv) VENV=$2; shift ;;
    --src) SRC=$2; shift ;;
    --no-isolcpus) ISOLCPUS=0 ;;
    -h|--help) sed -n '2,15p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

want() { [[ -z $ONLY || ,$ONLY, == *,$1,* ]] && [[ ,$SKIP, != *,$1,* ]]; }

# Files in the venv must belong to the person who owns the repo, not root.
owner() { stat -c %U "$REPO"; }
as_owner() { if (( EUID == 0 )) && [[ $(owner) != root ]]; then sudo -u "$(owner)" -H "$@"; else "$@"; fi; }

step_deps() {
  step "Build dependencies"
  local avail; avail=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)
  if (( avail < 250 )); then
    warn "only ${avail} MB RAM available. Compiling can exhaust memory and hang this Pi. Run this from"
    warn "a plain SSH session (not alongside Claude Code or other big programs), or stop them first."
  fi
  local missing=() d
  for d in "${DEPS[@]}"; do if ! installed "$d"; then missing+=("$d"); fi; done
  if (( ${#missing[@]} )); then
    run env DEBIAN_FRONTEND=noninteractive apt-get -y install "${missing[@]}"
  else
    echo "  all present: ${DEPS[*]}"
  fi
}

step_library() {
  step "Library source at $SRC (pinned ${RGBMATRIX_REF:0:10})"
  # Fetch only the pinned commit (a few MB). A full clone failed with TLS errors on this
  # connection; HTTP/1.1 plus retries is more robust.
  if [[ -d $SRC/.git ]] && [[ $(git -C "$SRC" rev-parse HEAD 2>/dev/null) == "$RGBMATRIX_REF" ]]; then
    echo "  already at pinned commit"
  else
    run as_owner mkdir -p "$SRC"
    run as_owner git init -q "$SRC"
    run as_owner git -C "$SRC" remote add origin "$RGBMATRIX_REPO" 2>/dev/null || true
    local attempt
    for attempt in 1 2 3 4 5; do
      if (( DRY_RUN )); then echo "  [dry-run] git fetch --depth 1 origin ${RGBMATRIX_REF:0:10} (retrying up to 5 times)"; break; fi
      as_owner git -C "$SRC" -c http.version=HTTP/1.1 fetch -q --depth 1 origin "$RGBMATRIX_REF" && break
      warn "fetch attempt $attempt failed; retrying"; sleep 5
      (( attempt < 5 )) || die "could not fetch the library; check the network and rerun"
    done
    run as_owner git -C "$SRC" checkout -q FETCH_HEAD
  fi
  # The C++ demo binaries are used by tools/test-matrix.sh (Adafruit "testing install").
  if [[ -x $SRC/examples-api-use/demo ]]; then
    echo "  demo binary already built"
  else
    run as_owner nice -n 10 make -C "$SRC" -j1      # one job: this Pi has ~460 MB RAM and LTO links are memory hungry
  fi
}

step_python() {
  step "Python bindings into $VENV"
  if [[ ! -x $VENV/bin/python ]]; then run as_owner python3 -m venv --system-site-packages "$VENV"; fi
  # The venv must see the apt Pillow (python3-pil), not a pip wheel: the bindings are compiled against
  # the apt package's headers and read Pillow's internal struct layout, which differs between versions.
  if [[ -f $VENV/pyvenv.cfg ]] && ! grep -q '^include-system-site-packages = true' "$VENV/pyvenv.cfg"; then
    run as_owner sed -i 's/^include-system-site-packages = .*/include-system-site-packages = true/' "$VENV/pyvenv.cfg"
  fi
  if compgen -G "$VENV/lib/python3*/site-packages/PIL" >/dev/null; then
    run as_owner "$VENV/bin/pip" uninstall -y pillow
  fi
  if [[ -x $VENV/bin/python ]] && "$VENV/bin/python" -c 'import rgbmatrix' 2>/dev/null; then
    echo "  rgbmatrix already importable"
    return 0
  fi
  # Building compiles C++ on a Zero 2 W: expect 10+ minutes. A single job avoids exhausting RAM.
  run as_owner nice -n 10 env CMAKE_BUILD_PARALLEL_LEVEL=1 "$VENV/bin/pip" install "$SRC"
}

step_boot() {
  step "Boot settings (sound off, reserved CPU core)"
  write_file /etc/modprobe.d/blacklist-rgb-matrix.conf \
'# The panel driver needs the PWM hardware that the onboard sound driver uses.
blacklist snd_bcm2835'
  if grep -qs '^dtparam=audio=off' "$BOOT_DIR/config.txt"; then
    echo "  config.txt already has dtparam=audio=off"
  else
    warn "dtparam=audio=off is missing from $BOOT_DIR/config.txt; run setup/01-pi-setup.sh --only boot --apply"
  fi
  if (( ISOLCPUS )); then
    [[ -f $BOOT_DIR/cmdline.txt ]] || die "$BOOT_DIR/cmdline.txt not found"
    cmdline_ensure "$BOOT_DIR/cmdline.txt" "${CMDLINE_TOKENS[@]}"
  else
    echo "  skipping isolcpus (--no-isolcpus)"
  fi
  if (( ! DRY_RUN )) && command -v update-initramfs >/dev/null; then
    run update-initramfs -u    # so the blacklist also applies in early boot
  fi
  info "boot changes take effect after a reboot"
}

step_verify() {
  step "Verification"
  if lsmod | grep -q '^snd_bcm2835'; then warn "snd_bcm2835 is loaded (blacklist applies after reboot)"; else echo "  sound module: not loaded"; fi
  if grep -q 'isolcpus=' /proc/cmdline; then echo "  isolcpus active: $(tr ' ' '\n' </proc/cmdline | grep -E '^(isolcpus|nohz_full)' | tr '\n' ' ')"; else echo "  isolcpus: not active yet"; fi
  [[ -x $SRC/examples-api-use/demo ]] && echo "  demo binary: $SRC/examples-api-use/demo" || echo "  demo binary: not built"
  if [[ -x $VENV/bin/python ]] && "$VENV/bin/python" -c 'import rgbmatrix' 2>/dev/null; then
    echo "  python bindings: OK ($VENV); Pillow from: $("$VENV/bin/python" -c 'import PIL,os;print(os.path.dirname(PIL.__file__))')"
  else
    echo "  python bindings: not importable yet"
  fi
  echo "  next: sudo reboot, then  ./tools/test-matrix.sh"
}

require_pi
require_root
if (( DRY_RUN )); then
  info "DRY RUN: nothing will be changed. Re-run with --apply (as root) to execute."
else
  mkdir -p "$BACKUP_DIR"; exec > >(tee -a /var/log/matrix-setup.log) 2>&1
  confirm "This builds the matrix library, blacklists the sound driver, and edits $BOOT_DIR/cmdline.txt."
fi
for s in "${ALL_STEPS[@]}"; do if want "$s"; then "step_$s"; fi; done
echo
if (( DRY_RUN )); then info "dry run complete; nothing changed."; else info "done. Reboot, then run tools/test-matrix.sh"; fi
