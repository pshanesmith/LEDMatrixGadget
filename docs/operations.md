# Operating the display

Everyday commands (the service is `matrix-display`).

**Where things live:** the code runs from `/opt/matrix`, a clone of the GitHub repository (with its own `.venv/` and `vendor/` inside); config and the token are in `/etc/matrix`; the weather and Home Assistant cache is in `/var/lib/matrix-display`. Your checkout in your home folder is for development only.

**Update from GitHub:** push your changes, then on the Pi run `sudo /opt/matrix/setup/03-install-service.sh --apply` (pulls the latest `main`, reinstalls, restarts). `--ref v1.0` deploys a tag; `--repo URL` changes the source.

| Task | Command |
|---|---|
| Is it running? | `systemctl status matrix-display` |
| Live log (one performance line a minute) | `journalctl -u matrix-display -f` |
| Restart | `sudo systemctl restart matrix-display` |
| Re-read config without restarting | `sudo systemctl reload matrix-display` |
| Stop / start the panel | `sudo systemctl stop matrix-display` / `start` |
| Health summary since a time | `./tools/soak-report.sh "2026-10-24 01:20"` (default: last 24 h) |
| Remove the service | `sudo ./setup/03-install-service.sh --uninstall` |

## Changing settings
- **Edit `/etc/matrix/config.toml`** (a copy; the repo's `config/config.toml` is only the source used at install time), then `reload`.
- **Location:** `python3 tools/geocode.py <ZIP>` updates `config/config.toml`; copy the `[location]` section into `/etc/matrix/config.toml`.
- **Gates, lights, locks:** `python3 tools/ha-discover.py --toml` lists entities and prints a snippet for `[homeassistant]`. `exclude_lights` takes glob patterns on entity ids.
- **Home Assistant URL** defaults to `http://homeassistant:8123`; change `[homeassistant].url`, or set `MATRIX_HA_URL` in `/etc/matrix/secrets.env`.
- **Token:** only in `/etc/matrix/secrets.env` (`MATRIX_HA_TOKEN=`, mode 600, root). If it leaks, delete it in Home Assistant (profile > Security) and create a new one.
- **Brightness:** `[display] brightness` (day) and `max_brightness` (hard ceiling, 70). After sunset the display is halved automatically. The supply is rated 4 A; do not raise the ceiling much.

## Flicker
The panel is refreshed by software timing, so flicker is a trade between colour depth and refresh rate. With the defaults (`pwm_bits = 11`) the measured refresh was about 108 Hz; each bit removed roughly doubles it. Try in this order, restarting after each (`sudo systemctl restart matrix-display`; a reload does not change panel timing):
1. `pwm_bits = 10` (about 200 Hz), then `9` (about 400 Hz) if still visible. Fewer bits means slightly coarser dark gradients.
2. `limit_refresh_hz = 150` (or a value below the stable rate) so the rate does not wander with CPU load.
3. `gpio_slowdown = 3` if you also see shimmering lines or odd pixels.
Set each value with the helper (safe to repeat, and it repairs duplicate `[display]` sections): `sudo python3 tools/set-config.py /etc/matrix/config.toml display pwm_bits 10`, then restart. Already ruled out: interrupts are kept off the reserved core, and `isolcpus` is active.

## What the panel shows
Date; a scene with the current condition (sun/moon, clouds, rain, snow, fog, thunder, day/night) and the temperature; today's low and high; the next change in conditions within 24 h (or `STEADY 24H`); then lock, light and gate status. Locks: number of unlocked ones (red when nonzero, an amber `?` if one is offline). Lights: room lights on (status LEDs and groups are excluded). Gate: green closed, blinking red open, amber unknown. A dot in the bottom-right corner appears when data is stale (amber: Home Assistant older than 5 minutes; red: weather older than 3 hours).

## Troubleshooting
| Symptom | Check |
|---|---|
| Panel blank | `systemctl status matrix-display`; `journalctl -u matrix-display -n 50`; supply connected; `sudo ./tools/test-matrix.sh` after stopping the service |
| Flicker | `vcgencmd get_throttled` (want `0x0`); `isolcpus` active in `/proc/cmdline`; try `gpio_slowdown = 3` |
| Wrong colours | `rgb_sequence` in `[display]` |
| No weather | log shows `weather refresh failed`; check network and `[location]` |
| Home Assistant row stale or amber dot | log shows 401 (token) or unreachable (URL/network) |
| Pi unresponsive or rebooted | `./tools/soak-report.sh`; the journal is persistent. Memory exhaustion (see below) was the cause of the freezes seen during development |

## Memory (important)
The Pi has about 460 MB of RAM. The display itself needs about 35 MB. Claude Code and other large programs can exhaust memory; the symptoms were SSH dropping, the Wi-Fi chip logging `brcmfmac ... failed`, and the hardware watchdog resetting the board after about a minute. Keep big jobs and extra sessions off the Pi while the display runs, or develop from another machine.

## Updating
`sudo apt update && sudo apt full-upgrade`, then reboot. After a kernel update confirm `isolcpus` and the sound blacklist still apply (`./setup/02-matrix-setup.sh --only verify`). No unattended upgrades are installed by design.

## After a Pillow upgrade
The panel bindings were compiled against Debian's `python3-pil` and read Pillow's internal memory layout. If `apt` upgrades `python3-pil`, rebuild: `sudo ./setup/02-matrix-setup.sh --apply` (the pip install step recompiles), restart the service, and check the picture before leaving it.

## Backups
`sudo cp -a /etc/matrix ~/matrix-etc-backup` (contains the token; keep it private). The repository holds everything else; `config/config.toml`, `secrets.env`, `vendor/` and `.venv/` are gitignored, and `.githooks/pre-commit` blocks commits that contain a token (enable with `git config core.hooksPath .githooks`, already set).

## Rebuilding from scratch
`setup/01-pi-setup.sh --apply`, reboot, `setup/02-matrix-setup.sh --apply`, reboot, `tools/test-matrix.sh`, then `setup/03-install-service.sh --apply`.
