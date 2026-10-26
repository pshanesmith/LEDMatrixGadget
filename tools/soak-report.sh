#!/usr/bin/env bash
# Summarise how the display has behaved since a start time (default: the last 24 hours).
#   ./tools/soak-report.sh                     last 24 h
#   ./tools/soak-report.sh "2026-10-24 01:20"  since a given time
# Reads the persistent journal; no root needed if you are in the 'adm' or 'systemd-journal' group.
set -uo pipefail
SINCE=${1:-"24 hours ago"}
UNIT=matrix-display
bad=0
flag() { echo "  !! $*"; bad=1; }

echo "== Soak report since: $SINCE"
echo "now: $(date)   uptime: $(uptime -p)"
echo

echo "== Boots and resets (a watchdog or power reset leaves 'orphan cleanup' in the next boot)"
journalctl --list-boots --no-pager | tail -6
echo "  reset status register: $(vcgencmd get_rsts 2>/dev/null)   (0x20 = software/watchdog reset, 0x1000 = power-on)"
echo

echo "== Service"
echo "  state: $(systemctl is-active $UNIT), enabled: $(systemctl is-enabled $UNIT 2>&1)"
echo "  started: $(systemctl show $UNIT -p ActiveEnterTimestamp --value)"
restarts=$(systemctl show $UNIT -p NRestarts --value)
echo "  restarts since boot: $restarts"
[[ ${restarts:-0} -gt 0 ]] && flag "service restarted $restarts time(s); see the errors below"
echo

echo "== Warnings and errors from the service"
journalctl -u $UNIT --since "$SINCE" -p warning --no-pager -o short-iso | grep -v 'No entries' | cut -c1-200 | tail -15
echo

echo "== Kernel problems (hung tasks, SD card, Wi-Fi chip, memory, undervoltage)"
k=$(journalctl -k --since "$SINCE" --no-pager -g 'blocked for more than|oom-kill|Out of memory|Under-voltage|under-voltage|mmc[0-9]: .*(never released|timeout|error)|brcmf.*(failed|error)|I/O error|soft lockup|EXT4-fs error' -o short-iso | grep -v '^-- ' | cut -c1-200)
if [[ -n $k ]]; then echo "$k" | tail -15; flag "kernel reported problems"; else echo "  none"; fi
echo

echo "== Performance lines (one per minute): worst values in the period"
journalctl -u $UNIT --since "$SINCE" --no-pager -o cat -g 'frames=' | awk '
  function num(key, unit,   m) { if (match($0, key "=[0-9.]+" unit)) { m = substr($0, RSTART + length(key) + 1, RLENGTH - length(key) - 1 - length(unit)); return m + 0 } return "" }
  /frames=/ {
    n++
    v = num("frames", ""); if (v != "" && (fmin == "" || v < fmin)) fmin = v
    v = num("avg", "ms");  if (v != "" && v > avg) avg = v
    v = num("max", "ms");  if (v != "" && v > mx) mx = v
    v = num("overruns", ""); if (v != "") over += v
    v = num("rss", "MB");  if (v != "" && v > rss) rss = v
    v = num("mem_avail", "MB"); if (v != "" && (mem == "" || v < mem)) mem = v
    v = num("temp", "C");  if (v != "" && v > temp) temp = v
    if (match($0, /throttled=0x[0-9a-f]+/)) seen[substr($0, RSTART + 10, RLENGTH - 10)] = 1
  }
  END {
    if (!n) { print "  no performance lines yet"; exit }
    printf "  samples: %d minutes\n", n
    printf "  %-26s%.0f\n", "lowest frames per minute", fmin
    printf "  %-26s%.1f\n", "render avg, worst (ms)", avg
    printf "  %-26s%.1f\n", "render max, worst (ms)", mx
    printf "  %-26s%.0f\n", "overruns, total", over
    printf "  %-26s%.1f\n", "app memory, max (MB)", rss
    printf "  %-26s%.0f\n", "free memory, lowest (MB)", mem
    printf "  %-26s%.1f\n", "temperature, max (C)", temp
    t = ""; for (k in seen) t = t (t == "" ? "" : ", ") k
    print "  throttle flags seen:       " t
  }'
echo

echo "== Power / throttling since boot (the service cannot read this after dropping root, so it is read here)"
echo "  $(vcgencmd get_throttled 2>&1)   (0x0 = never under-voltage, capped or throttled since boot)"
echo

echo "== Memory right now"
free -m | sed -n 1,3p
echo
if (( bad )); then echo "RESULT: needs attention (see !! lines)"; else echo "RESULT: clean"; fi
exit $bad
