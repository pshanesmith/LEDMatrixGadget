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
journalctl -u $UNIT --since "$SINCE" --no-pager -o cat -g 'frames=' | python3 -I -c '
import re, sys
rows = [l for l in sys.stdin if "frames=" in l]
if not rows:
    print("  no performance lines yet"); sys.exit()
def vals(key, unit=""):
    return [float(m.group(1)) for l in rows for m in [re.search(key + r"=([0-9.]+)" + unit, l)] if m]
def show(label, key, unit="", fn=max, fmt="{:.1f}"):
    v = vals(key, unit)
    if v: print(f"  {label:<26}" + fmt.format(fn(v)) + (f"  (of {len(v)} samples)" if label.startswith("frames") else ""))
print(f"  samples: {len(rows)} minutes")
show("lowest frames per minute", "frames", fn=min, fmt="{:.0f}")
show("render avg, worst (ms)", "avg", "ms")
show("render max, worst (ms)", "max", "ms")
show("overruns, total", "overruns", fn=sum, fmt="{:.0f}")
show("app memory, max (MB)", "rss", "MB")
show("free memory, lowest (MB)", "mem_avail", "MB", fn=min, fmt="{:.0f}")
show("temperature, max (C)", "temp", "C")
th = {m.group(1) for l in rows for m in [re.search(r"throttled=(0x[0-9a-f]+)", l)] if m}
print("  throttle flags seen:       " + ", ".join(sorted(th)))
' 
echo

echo "== Power / throttling since boot (the service cannot read this after dropping root, so it is read here)"
echo "  $(vcgencmd get_throttled 2>&1)   (0x0 = never under-voltage, capped or throttled since boot)"
echo

echo "== Memory right now"
free -m | sed -n 1,3p
echo
if (( bad )); then echo "RESULT: needs attention (see !! lines)"; else echo "RESULT: clean"; fi
exit $bad
