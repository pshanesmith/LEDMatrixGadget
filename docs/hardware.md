# Hardware checklist (Phase 2)

Sources: the Adafruit RGB Matrix Bonnet guide and the hzeller `rpi-rgb-led-matrix` README.

## Already done (confirmed by the owner)
- **E address line:** on the Bonnet's "Address E" pads, bridge the centre **E** pad to the **"8"** pad above it (Adafruit 64x64 panels). Without it the panel shows only half the rows, repeated.
- **PWM bridge:** a wire/solder bridge between **GPIO 4 and GPIO 18**. This enables hardware pulses (less flicker) and is why the mapping is `adafruit-hat-pwm`.
- **Power:** 5 V / 4 A supply to the Bonnet's screw terminals. Do not also power the Pi over USB.

## Before powering up
1. Bonnet fully seated on the 40-pin header; ribbon cable in the panel's **input** connector (check the panel's silkscreen for IN / arrow direction; the ribbon is keyed).
2. Panel power leads: red to +5 V, black to GND, tight in the terminals; the Bonnet's own power lead is the only source.
3. Start with the default brightness (50%); do not run full-white at 100% on a 4 A supply.

## Settings used for this panel
`--led-rows=64 --led-cols=64 --led-gpio-mapping=adafruit-hat-pwm --led-slowdown-gpio=2` (Pi Zero 2 W behaves like a Pi 3; start at 2, try 1 if the image is fine, raise to 3 if it flickers).

## If the picture is wrong
| Symptom | Try |
|---|---|
| Top half repeated / only 32 rows | Re-check the E jumper; then `EXTRA='--led-row-addr-type=1'` (panel with only A/B lines) |
| Red/green/blue swapped | `EXTRA='--led-rgb-sequence=RBG'` (also GBR, BRG) |
| Flicker or shimmering lines | `SLOWDOWN=3`; confirm `isolcpus` is active after reboot; confirm the sound module is not loaded |
| Ghost images / smearing on bright content | Lower `--led-pwm-lsb-nanoseconds` is faster, higher reduces ghosting (default 130; try 200-300) |
| Demo prints a sound warning and exits | Reboot after setup so `snd_bcm2835` is blacklisted |
| Nothing lights, no error | Check the Bonnet's power LED, supply polarity, ribbon orientation |
| `vcgencmd get_throttled` not `0x0` | Undervoltage: the Pi is also fed from the Bonnet; check supply and wiring |
