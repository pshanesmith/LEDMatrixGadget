#!/usr/bin/env bash
# Phase 2 hardware test, after Adafruit's "Testing Install" page, adapted to a 64x64 panel.
#   sudo ./tools/test-matrix.sh            guided test (asks what you see)
#   sudo ./tools/test-matrix.sh --quick    one demo + the Python smoke test, no questions
# Override settings with environment variables, e.g.  SLOWDOWN=3 MAPPING=adafruit-hat ./tools/test-matrix.sh
# Needs root (GPIO access). Results are appended to docs/hardware-test-results.txt.
set -uo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
SRC=${SRC:-$REPO/vendor/rpi-rgb-led-matrix}
DEMO=$SRC/examples-api-use/demo
PY=${PY:-$REPO/.venv/bin/python}
ROWS=${ROWS:-64}; COLS=${COLS:-64}
MAPPING=${MAPPING:-adafruit-hat-pwm}
SLOWDOWN=${SLOWDOWN:-2}
EXTRA=${EXTRA:-}               # e.g. EXTRA="--led-row-addr-type=1 --led-rgb-sequence=RBG"
SECONDS_EACH=${SECONDS_EACH:-12}
QUICK=0; [[ ${1:-} == --quick ]] && QUICK=1
RESULTS=$REPO/docs/hardware-test-results.txt
FLAGS="--led-rows=$ROWS --led-cols=$COLS --led-gpio-mapping=$MAPPING --led-slowdown-gpio=$SLOWDOWN $EXTRA"

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { mkdir -p "$(dirname "$RESULTS")"; echo "$(date '+%F %T')  $*" >> "$RESULTS"; }
ask() {                         # ask "question" -> returns 0 for yes
  (( QUICK )) && return 0
  local a; read -r -p "$1 [y/n] " a; [[ $a == y* || $a == Y* ]]
}

(( EUID == 0 )) || fail "run with sudo (the panel driver needs GPIO access)"
[[ -x $DEMO ]] || fail "demo binary not found at $DEMO; run setup/02-matrix-setup.sh --apply"
lsmod | grep -q '^snd_bcm2835' && fail "snd_bcm2835 is loaded; reboot after setup/02-matrix-setup.sh"

echo "== pre-checks"
grep -q isolcpus /proc/cmdline && echo "  isolcpus: active" || echo "  isolcpus: NOT active (flicker more likely)"
echo "  power: $(vcgencmd get_throttled 2>/dev/null || echo 'vcgencmd unavailable')   (0x0 is healthy)"
echo "  flags: $FLAGS"
note "START flags: $FLAGS"

run_demo() {                    # run_demo <demo-number> <description> <what to look for>
  echo; echo "== demo -D $1: $2"
  echo "   look for: $3"
  timeout "$SECONDS_EACH" "$DEMO" $FLAGS -D "$1" --led-show-refresh >/tmp/matrix-demo.log 2>&1
  local rc=$?
  [[ $rc -eq 0 || $rc -eq 124 ]] || { cat /tmp/matrix-demo.log; note "demo $1 exited $rc"; fail "demo exited with status $rc (see above)"; }
  tail -n 2 /tmp/matrix-demo.log | tr '\r' '\n' | tail -n 1
}

run_demo 0 "rotating square" "the square centred on the panel; ALL 64 rows lit, not a duplicated half"
ask "Did the whole panel show one rotating square with no duplicated top/bottom half?" \
  && note "demo 0: OK" || { note "demo 0: PROBLEM (duplicated/missing rows?) - check E jumper, try EXTRA=--led-row-addr-type=1"; echo "Try: EXTRA='--led-row-addr-type=1' $0"; exit 1; }

if (( ! QUICK )); then
  run_demo 4 "pulsing colour" "smooth colour pulses; no wrong colours (red/green swapped?)"
  ask "Are the colours correct and smooth?" && note "demo 4: OK" || { note "demo 4: PROBLEM colours - try EXTRA=--led-rgb-sequence=RBG (or GBR, BRG)"; echo "Try: EXTRA='--led-rgb-sequence=RBG' $0"; }
  run_demo 5 "grayscale block" "smooth grey steps without banding or flicker"
  ask "Is it free of flicker and stray lines?" && note "demo 5: OK" || note "demo 5: PROBLEM flicker - raise SLOWDOWN (3), lower pwm bits, check PWM bridge"
fi

echo; echo "== Python smoke test (our HardwareDisplay backend, 20 s)"
[[ -x $PY ]] || fail "python venv missing at $PY"
"$PY" "$REPO/tools/matrix_hello.py" --gpio-slowdown "$SLOWDOWN" --hardware-mapping "$MAPPING" --seconds 20
ask "Did you see: red top half, blue bottom half, white border, RGB bars, and a smoothly moving square?" \
  && note "python smoke test: OK" || note "python smoke test: PROBLEM"
note "END"
echo; echo "Results saved to $RESULTS"
