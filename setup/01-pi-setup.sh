#!/usr/bin/env bash
# Slim down the Pi, cut power use, update, and install build dependencies.
#
#   ./setup/01-pi-setup.sh                 dry run (default): shows everything, changes nothing
#   sudo ./setup/01-pi-setup.sh --apply    do it (asks you to type 'yes' first)
#
# Options:  --aggressive   also remove cloud-init, Raspberry Pi Connect, USB gadget mode
#           --yes          skip the confirmation prompt
#           --only a,b     run only these steps   --skip a,b   skip these steps
# Steps:    purge boot system update verify
# Never reboots; reboot yourself afterwards. Safe to run repeatedly.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib/common.sh
source "$HERE/lib/common.sh"

BOOT_DIR=${BOOT_DIR:-/boot/firmware}
HA_URL=${MATRIX_HA_URL:-http://homeassistant:8123}
ALL_STEPS=(purge boot system update verify)
AGGRESSIVE=0; ASSUME_YES=0; ONLY=""; SKIP=""

while (( $# )); do
  case $1 in
    --apply) DRY_RUN=0 ;;
    --aggressive) AGGRESSIVE=1 ;;
    --yes) ASSUME_YES=1 ;;
    --only) ONLY=$2; shift ;;
    --skip) SKIP=$2; shift ;;
    -h|--help) sed -n '2,12p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $1 (try --help)" ;;
  esac
  shift
done

want() {
  local s=$1
  [[ -z $ONLY || ,$ONLY, == *,$s,* ]] && [[ ,$SKIP, != *,$s,* ]]
}

# --- package lists -------------------------------------------------------------
# Only packages that are actually installed are touched. Reasons are for the reader.
PURGE_CORE=(
  bluez bluez-firmware bluez-obexd pi-bluetooth            # Bluetooth is not used (power)
  linux-image-rpi-2712 linux-headers-rpi-2712              # Pi 5 kernel; Zero 2 W boots the v8 kernel
  linux-headers-rpi-v8                                     # kernel headers; no kernel modules are built
  firmware-atheros firmware-mediatek firmware-realtek firmware-libertas  # USB Wi-Fi dongles; this board uses brcm80211
  rpi-eeprom                                               # Pi 4/5 bootloader updater; Zero 2 W has no EEPROM
  rpicam-apps-lite v4l-utils                               # camera tools; no camera attached (kms++-utils stays: raspi-utils needs it)
  mkvtoolnix p7zip-full ntfs-3g cifs-utils libmtp-runtime udisks2   # unused desktop-style utilities
  gdb apt-listchanges pciutils usb-modeswitch wireless-tools manpages-dev
  triggerhappy modemmanager
)
PURGE_AGGRESSIVE=(
  cloud-init rpi-cloud-init-mods                           # first-boot provisioning, already done
  rpi-connect-lite                                         # Raspberry Pi Connect remote access
  rpi-usb-gadget                                           # USB gadget (Ethernet over USB) mode
)
# apt must never remove any of these (or anything matching), or the run aborts.
PROTECT_RE='^(openssh-.*|ssh|network-manager|wpasupplicant|firmware-brcm80211|raspi-firmware|raspi-config|raspi-utils|raspberrypi-.*|userconf-pi|avahi-daemon|sudo|systemd.*|libsystemd.*|dbus.*|udev|libc6|apt|dpkg|bash|coreutils|login|passwd|initramfs-tools|linux-image-rpi-v8|linux-image-[0-9].*-rpi-v8|python3|python3-minimal|python3-venv|python3-pip|python3-dev|python3-setuptools|python3-gpiozero|python3-rpi-lgpio|python3-libgpiod|python3-spidev|python3-smbus2|build-essential|gcc|gcc-[0-9]+|gcc-[0-9]+-aarch64-linux-gnu|g\+\+|g\+\+-[0-9]+|cpp|libc6-dev|make|git|curl|ca-certificates|cron|iproute2|iputils-ping|nftables|rsync|dhcpcd-base|base-files|netbase|init|kmod|sed|grep|tar|gzip|libnss-.*|libpam.*)$'
DEPS=(git python3-dev python3-venv python3-pil cython3 python3-setuptools)   # python3-pil: needed by the panel bindings (see 02-matrix-setup.sh)

# Lines in config.txt that conflict with the power block below get commented out.
BOOT_BLOCK='dtoverlay=disable-bt
dtparam=audio=off
camera_auto_detect=0
display_auto_detect=0
gpu_mem=16'
BOOT_CONFLICTS='dtparam=audio=|camera_auto_detect=|display_auto_detect=|dtoverlay=vc4-kms-v3d|max_framebuffers=|gpu_mem=|dtoverlay=disable-bt'

DISABLE_UNITS=(e2scrub_all.timer e2scrub_reap.service man-db.timer NetworkManager-wait-online.service hciuart.service bluetooth.service)

# --- steps -----------------------------------------------------------------------
step_purge() {
  step "Remove unneeded packages"
  local list=("${PURGE_CORE[@]}") present=() p removed=() blocked=() sim
  # apt never auto-removes versioned kernel packages, so name the Pi 5 kernel and all
  # kernel headers explicitly. The v8 kernel image (this board's, and the running one) is kept.
  while read -r p; do list+=("$p"); done < <(dpkg-query -W -f='${Package}\n' \
      'linux-image-*-rpi-2712' 'linux-headers-*-rpi-2712' 'linux-headers-*-rpi-v8' 'linux-base-*-rpi-2712' 2>/dev/null || true)
  if (( AGGRESSIVE )); then list+=("${PURGE_AGGRESSIVE[@]}"); fi
  for p in "${list[@]}"; do if installed "$p"; then present+=("$p"); fi; done
  if (( ${#present[@]} == 0 )); then info "nothing to remove"; return 0; fi
  info "requested (installed): ${present[*]}"

  sim=$(apt-get -s -o Debug::NoLocking=1 purge --auto-remove "${present[@]}" 2>&1) \
    || { echo "$sim"; die "apt could not simulate the removal"; }
  mapfile -t removed < <(awk '/^(Remv|Purg) /{print $2}' <<<"$sim")
  for p in "${removed[@]}"; do if [[ $p =~ $PROTECT_RE ]]; then blocked+=("$p"); fi; done
  if (( ${#blocked[@]} )); then
    die "removal would take out protected package(s): ${blocked[*]}. Nothing was changed."
  fi
  info "apt will remove ${#removed[@]} packages in total:"
  printf '%s ' "${removed[@]}" | fold -s -w 100 | sed 's/^/    /'; echo

  run env DEBIAN_FRONTEND=noninteractive apt-get -y purge --auto-remove "${present[@]}"
  local rc; rc=$(dpkg -l | awk '/^rc/{print $2}' | tr '\n' ' ')
  if [[ -n $rc ]]; then run dpkg --purge $rc; fi
}

step_boot() {
  step "Boot configuration (Bluetooth, audio, HDMI, camera off)"
  [[ -f $BOOT_DIR/config.txt ]] || die "$BOOT_DIR/config.txt not found"
  config_txt_update "$BOOT_DIR/config.txt" power "$BOOT_BLOCK" "$BOOT_CONFLICTS"
  info "takes effect after a reboot"
}

step_system() {
  step "Services, LED, journal size"
  local u
  for u in "${DISABLE_UNITS[@]}"; do
    if unit_exists "$u"; then run systemctl disable --now "$u"; else echo "  skip (not installed): $u"; fi
  done

  write_file /etc/systemd/system/led-off.service '[Unit]
Description=Turn off the ACT LED (saves a little power, no blinking in a living space)

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=/bin/sh -c "echo none > /sys/class/leds/ACT/trigger; echo 0 > /sys/class/leds/ACT/brightness"

[Install]
WantedBy=multi-user.target'
  if [[ -d /sys/class/leds/ACT ]]; then
    run systemctl daemon-reload
    run systemctl enable --now led-off.service
  fi

  # Raspberry Pi OS ships 40-rpi-volatile-storage.conf (Storage=volatile), so logs vanish on every
  # reboot. A drop-in whose name sorts later wins. Persistent logs are essential for diagnosing
  # hangs and power loss on an always-on display; the size cap limits SD-card wear.
  write_file /etc/systemd/journald.conf.d/matrix.conf '[Journal]
Storage=persistent
SystemMaxUse=30M
RuntimeMaxUse=15M'

  run systemctl restart systemd-journald
}

step_update() {
  step "Update packages and install build dependencies"
  if (( DRY_RUN )); then
    echo "  [dry-run] apt-get update  (skipped; simulating with the current package lists)"
    apt-get -s -o Debug::NoLocking=1 full-upgrade | awk '/^Inst /{n++} END{print "  would upgrade/install " n+0 " packages"}'
    local missing=() d
    for d in "${DEPS[@]}"; do if ! installed "$d"; then missing+=("$d"); fi; done
    echo "  would install dependencies: ${missing[*]:-(all present)}"
    return 0
  fi
  export DEBIAN_FRONTEND=noninteractive
  run apt-get update
  run apt-get -y -o Dpkg::Options::=--force-confold full-upgrade
  run apt-get -y install "${DEPS[@]}"
  run apt-get clean
}

step_verify() {
  step "Verification"
  local n; n=$(dpkg -l | grep -c '^ii')
  echo "  installed packages: $n (633 on the stock image)"
  echo "  disk free on /: $(df -h --output=avail / | tail -1 | tr -d ' ')"
  echo "  bluetooth service: $(systemctl is-enabled bluetooth.service 2>&1 | head -1)"
  echo "  config.txt power block: $(grep -c 'matrix-setup:power' "$BOOT_DIR/config.txt" 2>/dev/null || true) markers"
  local ha_host; ha_host=$(sed -E 's#^[a-z]+://([^:/]+).*#\1#' <<<"$HA_URL")
  if getent hosts "$ha_host" >/dev/null 2>&1; then
    local code
    code=$(curl -s -o /dev/null -m 5 -w '%{http_code}' "$HA_URL/" || true)
    echo "  Home Assistant $HA_URL: HTTP $code"
  else
    warn "Home Assistant host in $HA_URL does not resolve"
  fi
  if (( ! DRY_RUN )) && [[ -f /var/run/reboot-required || $(grep -c 'matrix-setup:power' "$BOOT_DIR/config.txt") -gt 0 ]]; then
    info "reboot to apply boot changes:  sudo reboot"
  fi
}

# --- main --------------------------------------------------------------------------
require_pi
require_root
if (( DRY_RUN )); then
  info "DRY RUN: nothing will be changed. Re-run with --apply (as root) to execute."
else
  mkdir -p "$BACKUP_DIR"; exec > >(tee -a /var/log/matrix-setup.log) 2>&1
  info "backups and state snapshot go to $BACKUP_DIR; log: /var/log/matrix-setup.log"
  confirm "This removes packages and edits $BOOT_DIR/config.txt."
  snapshot_state
fi

for s in "${ALL_STEPS[@]}"; do
  if want "$s"; then "step_$s"; fi
done
echo
if (( DRY_RUN )); then info "dry run complete; nothing changed."; else info "done. Reboot when ready: sudo reboot"; fi
