#!/usr/bin/env bash
# Hardware test, after Adafruit's "Testing Install" page, adapted to a 64x64 panel. It uses matrix-hello,
# which drives the panel through the same code as the service (the rpi-led-panel crate).
#   sudo ./tools/test-matrix.sh            guided test (asks what you see)
#   sudo ./tools/test-matrix.sh --quick    one pattern + the frame test, no questions
# Override settings with environment variables, e.g.  SLOWDOWN=3 MAPPING=adafruit-hat ./tools/test-matrix.sh
# Needs root (GPIO access). Results are appended to docs/hardware-test-results.txt.
set -uo pipefail

REPO=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
# The installed copy, a cargo build in this checkout, or a cross-built one (tools/cross-build.sh).
HELLO=${HELLO:-}
for candidate in "$REPO/bin/matrix-hello" "$REPO/target/pi/matrix-hello" "$REPO/target/release/matrix-hello" "$REPO/dist/matrix-hello"; do
  if [[ -z $HELLO && -x $candidate ]]; then HELLO=$candidate; fi
done
ROWS=${ROWS:-64}; COLS=${COLS:-64}
MAPPING=${MAPPING:-adafruit-hat-pwm}
SLOWDOWN=${SLOWDOWN:-2}
EXTRA=${EXTRA:-}               # e.g. EXTRA="--row-address-type=1 --rgb-sequence=RBG"
SECONDS_EACH=${SECONDS_EACH:-12}
QUICK=0; [[ ${1:-} == --quick ]] && QUICK=1
RESULTS=$REPO/docs/hardware-test-results.txt
FLAGS="--rows $ROWS --cols $COLS --hardware-mapping $MAPPING --gpio-slowdown $SLOWDOWN $EXTRA"

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { mkdir -p "$(dirname "$RESULTS")"; echo "$(date '+%F %T')  $*" >> "$RESULTS"; }
ask() {                         # ask "question" -> returns 0 for yes
  (( QUICK )) && return 0
  local a; read -r -p "$1 [y/n] " a; [[ $a == y* || $a == Y* ]]
}

(( EUID == 0 )) || fail "run with sudo (the panel driver needs GPIO access)"
[[ -n $HELLO ]] || fail "matrix-hello not found; run setup/02-matrix-setup.sh --apply (or tools/cross-build.sh)"
lsmod | grep -q '^snd_bcm2835' && fail "snd_bcm2835 is loaded; reboot after setup/02-matrix-setup.sh"

echo "== pre-checks"
grep -q isolcpus /proc/cmdline && echo "  isolcpus: active" || echo "  isolcpus: NOT active (flicker more likely)"
echo "  power: $(vcgencmd get_throttled 2>/dev/null || echo 'vcgencmd unavailable')   (0x0 is healthy)"
echo "  flags: $FLAGS"
note "START flags: $FLAGS"

run_pattern() {                 # run_pattern <pattern> <description> <what to look for>
  echo; echo "== pattern $1: $2"
  echo "   look for: $3"
  # shellcheck disable=SC2086
  "$HELLO" $FLAGS --pattern "$1" --seconds "$SECONDS_EACH" --show-refresh >/tmp/matrix-hello.log 2>&1
  local rc=$?
  [[ $rc -eq 0 ]] || { cat /tmp/matrix-hello.log; note "pattern $1 exited $rc"; fail "matrix-hello exited with status $rc (see above)"; }
  tail -n 2 /tmp/matrix-hello.log | tr '\r' '\n' | tail -n 1
}

run_pattern square "rotating square" "the square centred on the panel; ALL 64 rows lit, not a duplicated half"
ask "Did the whole panel show one rotating square with no duplicated top/bottom half?" \
  && note "square: OK" || { note "square: PROBLEM (duplicated/missing rows?) - check E jumper, try EXTRA=--row-address-type=1"; echo "Try: EXTRA='--row-address-type=1' $0"; exit 1; }

if (( ! QUICK )); then
  run_pattern pulse "pulsing colour" "smooth colour pulses; no wrong colours (red/green swapped?)"
  ask "Are the colours correct and smooth?" && note "pulse: OK" || { note "pulse: PROBLEM colours - try EXTRA=--rgb-sequence=RBG (or GBR, BRG)"; echo "Try: EXTRA='--rgb-sequence=RBG' $0"; }
  run_pattern gray "grayscale steps" "smooth grey steps without banding or flicker"
  ask "Is it free of flicker and stray lines?" && note "gray: OK" || note "gray: PROBLEM flicker - raise SLOWDOWN (3), lower pwm bits, check PWM bridge"
fi

echo; echo "== Frame test (matrix-hello, 20 s)"
# shellcheck disable=SC2086
"$HELLO" $FLAGS --pattern frame --seconds 20
ask "Did you see: red top half, blue bottom half, white border, RGB bars, and a smoothly moving square?" \
  && note "frame test: OK" || note "frame test: PROBLEM"
note "END"
echo; echo "Results saved to $RESULTS"
