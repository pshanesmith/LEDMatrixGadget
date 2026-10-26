#!/usr/bin/env bash
# Verify the display is healthy; meant to run after every boot (so after an automatic update and reboot) and daily.
#   ./tools/health-check.sh        prints a verdict, logs it to the journal (tag matrix-health), exits 1 on problems
# Reads the journal, so run it as root (the systemd timer does) or as a member of the adm group.
set -uo pipefail

UNIT=matrix-display
bad=0
msgs=()

ok()   { msgs+=("ok   $*"); }
warn() { msgs+=("warn $*"); }
fail() { msgs+=("FAIL $*"); bad=1; }

# 1. The service is running.
if systemctl is-active --quiet "$UNIT"; then ok "service $UNIT is active"; else fail "service $UNIT is not running (journalctl -u $UNIT -n 50)"; fi

# 2. It is producing frames: the loop logs a performance line each minute; 15 fps is about 900 frames.
last=$(journalctl -u "$UNIT" --since "-3min" --no-pager -o cat -g 'frames=' 2>/dev/null | tail -1)
if [[ -z $last ]]; then
  up=$(systemctl show "$UNIT" -p ActiveEnterTimestampMonotonic --value 2>/dev/null)
  now=$(awk '{printf "%d", $1*1000000}' /proc/uptime)
  if [[ -n $up && $up != 0 ]] && (( now - up < 120000000 )); then warn "service started under 2 minutes ago; no frame statistics yet"
  else fail "no frame statistics in the last 3 minutes (the render loop may be stuck)"; fi
else
  frames=$(sed -n 's/.*frames=\([0-9]*\).*/\1/p' <<<"$last")
  if (( ${frames:-0} >= 800 )); then ok "rendering $frames frames a minute"; else fail "only ${frames:-0} frames in the last minute (expected about 900)"; fi
fi

# 3. Kernel trouble since boot: hung tasks, memory exhaustion, undervoltage, SD card or Wi-Fi chip errors.
k=$(journalctl -k -b --no-pager -g 'blocked for more than|oom-kill|Out of memory|[Uu]nder-voltage|mmc[0-9]: .*(never released|timeout)|brcmf.*failed|I/O error|EXT4-fs error' 2>/dev/null | grep -v '^-- ' | tail -3)
if [[ -z $k ]]; then ok "no kernel problems since boot"; else fail "kernel reported problems since boot: $(echo "$k" | tr '\n' '|' | cut -c1-200)"; fi

# 4. Boot settings the panel needs are still in place (a kernel or firmware update must not undo them).
grep -q 'isolcpus=' /proc/cmdline && ok "reserved CPU core active" || fail "isolcpus missing from /proc/cmdline (flicker likely)"
if lsmod | grep -q '^snd_bcm2835'; then fail "snd_bcm2835 is loaded (it conflicts with the panel)"; else ok "sound driver not loaded"; fi

# 5. The deployed binary is in place and runs (it only prints its version here; the panel is not touched).
BIN=${MATRIX_BIN:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/bin/matrix-display}
if [[ -x $BIN ]]; then ok "binary $("$BIN" --version 2>&1 | head -1)"; else fail "binary $BIN is missing (re-run setup/03-install-service.sh --apply)"; fi

# 6. Information: a reboot is pending, and what the last automatic upgrade did.
[[ -f /var/run/reboot-required ]] && warn "a reboot is pending (automatic reboot is at 04:00 when nobody is logged in)"
held=$(apt-mark showhold 2>/dev/null | tr '\n' ' ')
[[ -n $held ]] && warn "packages held back from upgrades: $held"

printf '%s\n' "${msgs[@]}"
if (( bad )); then
  logger -t matrix-health -p user.err "display health check FAILED: $(printf '%s; ' "${msgs[@]}" | grep -o 'FAIL[^;]*' | tr '\n' ' ')"
  echo "RESULT: problems found"; exit 1
fi
logger -t matrix-health -p user.info "display health check passed"
echo "RESULT: healthy"
