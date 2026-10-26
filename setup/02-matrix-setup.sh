#!/usr/bin/env bash
# Build the display (Rust, with the pure Rust rpi-led-panel crate as the panel driver), free the
# hardware from the sound driver, and reserve a CPU core for the panel refresh.
# Automates the system side of https://learn.adafruit.com/adafruit-rgb-matrix-bonnet-for-raspberry-pi/install-manually
# (updated for Debian 13).
#
#   ./setup/02-matrix-setup.sh                 dry run (default)
#   sudo ./setup/02-matrix-setup.sh --apply    do it
#
# Options: --yes  --only a,b  --skip a,b
#          --no-isolcpus keep all four cores for the OS
#          --rebuild     discard the previous build and compile everything again
# Steps:   deps build boot verify
# Never reboots. Safe to run repeatedly.
#
# The panel library is not cloned or pinned here: it is a Rust crate and Cargo.lock pins the version.
# Compiling on the Pi's 460 MB of RAM is slow (expect 15+ minutes the first time) and needs about 1 GB
# including swap; tools/cross-build.sh builds on another machine
# instead, and setup/03-install-service.sh --binary installs the result.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(dirname "$HERE")
# shellcheck source=lib/common.sh
source "$HERE/lib/common.sh"

BOOT_DIR=${BOOT_DIR:-/boot/firmware}
ISOLCPUS=1; REBUILD=0; ASSUME_YES=0; ONLY=""; SKIP=""
ALL_STEPS=(deps build boot verify)
# build-essential: a C compiler for the TLS library's C parts; cargo and rustc come from Debian (Rust 1.85
# on Debian 13, which is the minimum the project needs).
DEPS=(git build-essential cargo rustc)
# hzeller's recommendation for reserving core 3 for the refresh thread.
CMDLINE_TOKENS=("isolcpus=domain,managed_irq,3" "nohz_full=3" "rcu_nocbs=3" "irqaffinity=0,1,2")

while (( $# )); do
  case $1 in
    --apply) DRY_RUN=0 ;;
    --yes) ASSUME_YES=1 ;;
    --only) ONLY=$2; shift ;;
    --skip) SKIP=$2; shift ;;
    --no-isolcpus) ISOLCPUS=0 ;;
    --rebuild) REBUILD=1 ;;
    -h|--help) sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

want() { [[ -z $ONLY || ,$ONLY, == *,$1,* ]] && [[ ,$SKIP, != *,$1,* ]]; }

# Files in target/ must belong to the person who owns the repo, not root.
owner() { stat -c %U "$REPO"; }
as_owner() { if (( EUID == 0 )) && [[ $(owner) != root ]]; then sudo -u "$(owner)" -H "$@"; else "$@"; fi; }

step_deps() {
  step "Build dependencies"
  local missing=() d
  for d in "${DEPS[@]}"; do if ! installed "$d"; then missing+=("$d"); fi; done
  if (( ${#missing[@]} )); then
    run env DEBIAN_FRONTEND=noninteractive apt-get -y install "${missing[@]}"
  else
    echo "  all present: ${DEPS[*]}"
  fi
}

step_build() {
  step "Build the display with cargo (profile 'pi', features 'hardware')"
  local avail swap
  avail=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)
  swap=$(awk '/SwapFree/{print int($2/1024)}' /proc/meminfo)
  if (( avail + swap < 900 )); then
    warn "only ${avail} MB RAM and ${swap} MB swap free. Compiling can exhaust memory and hang this Pi."
    warn "Run it from a plain SSH session (no Claude Code or other big programs), add swap, or build elsewhere"
    warn "with tools/cross-build.sh and install the result with setup/03-install-service.sh --binary."
  fi
  if (( REBUILD )); then run as_owner cargo clean --manifest-path "$REPO/Cargo.toml" --profile pi; fi
  # One job at low priority: a parallel build of the TLS and image crates needs more RAM than this board has.
  run as_owner nice -n 10 cargo build --manifest-path "$REPO/Cargo.toml" --profile pi --features hardware --locked --bins -j1
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
  if [[ -x $REPO/target/pi/matrix-display ]]; then
    echo "  display binary: $REPO/target/pi/matrix-display ($("$REPO/target/pi/matrix-display" --version))"
  else
    echo "  display binary: not built yet (step 'build')"
  fi
  echo "  next: sudo reboot, then  ./tools/test-matrix.sh"
}

require_pi
require_root
if (( DRY_RUN )); then
  info "DRY RUN: nothing will be changed. Re-run with --apply (as root) to execute."
else
  mkdir -p "$BACKUP_DIR"; exec > >(tee -a /var/log/matrix-setup.log) 2>&1
  confirm "This builds the display, blacklists the sound driver, and edits $BOOT_DIR/cmdline.txt."
fi
for s in "${ALL_STEPS[@]}"; do if want "$s"; then "step_$s"; fi; done
echo
if (( DRY_RUN )); then info "dry run complete; nothing changed."; else info "done. Reboot, then run tools/test-matrix.sh"; fi
