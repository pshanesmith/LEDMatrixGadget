# Operating the display

Everyday commands (the service is `matrix-display`).

**Where things live:** the code runs from `/opt/matrix`, a clone of the GitHub repository (with its own `.venv/` and `vendor/` inside); config and the token are in `/etc/matrix`; the weather and Home Assistant cache is in `/var/lib/matrix-display`. Your checkout in your home folder is for development only.

**Inspecting the deployment:** `/opt/matrix` is root-owned, so use `sudo git -C /opt/matrix log --oneline` (without sudo git reports "dubious ownership").

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
- **Gates, lights, locks:** `python3 tools/ha-discover.py --toml` lists entities and prints a snippet for `[homeassistant]`. `exclude_lights` takes glob patterns on entity ids. `power_tier_entity` (a sensor reporting `discount`, `off_peak`, `on_peak` or `critical_peak`) adds the rate bolt to the bottom row.
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

## Automatic updates
The Pi upgrades itself nightly (`unattended-upgrades`): Debian security and stable updates plus the Raspberry Pi repository, run at about 03:00. If a kernel or firmware update needs a reboot it happens at **04:00 and only when nobody is logged in**, so an open SSH session postpones it a night; the display is dark for about a minute. Application code is not updated this way: use `sudo /opt/matrix/setup/03-install-service.sh --apply`.

| Task | Command |
|---|---|
| What did the last automatic run do? | `sudo tail -40 /var/log/unattended-upgrades/unattended-upgrades.log` |
| Preview what would be upgraded | `sudo unattended-upgrade --dry-run -d` |
| Is a reboot pending? | `ls /var/run/reboot-required` |
| Did the display come back healthy? | `journalctl -t matrix-health -n 5` or run `./tools/health-check.sh` |
| Turn automatic updates off | `sudo systemctl disable --now apt-daily-upgrade.timer` (re-enable with `enable --now`) |
| Hold a package back | `sudo apt-mark hold PACKAGE` (undo: `unhold`); nothing is held by default |

The health check runs ten minutes after every boot and daily at 08:00; a failure shows in `systemctl --failed` and the journal.

## Pillow upgrades
Pillow upgrades with the rest of the system. The panel bindings' fast image path reads Pillow's internal memory layout, so frames are pushed through Pillow's public API instead (`[display] safe_image_copy = true`, the default), which a Pillow upgrade cannot break. It costs about 8 ms a frame; check the `render avg` figure in `journalctl -u matrix-display` after a change. If you ever set `safe_image_copy = false`, rebuild the bindings after any Pillow upgrade: `sudo ./setup/02-matrix-setup.sh --apply --rebuild` (10+ minutes; close Claude Code first).

## Backups
`sudo cp -a /etc/matrix ~/matrix-etc-backup` (contains the token; keep it private). The repository holds everything else; `config/config.toml`, `secrets.env`, `vendor/` and `.venv/` are gitignored, and `.githooks/pre-commit` blocks commits that contain a token (enable with `git config core.hooksPath .githooks`, already set).

## Rebuilding from scratch
`setup/01-pi-setup.sh --apply`, reboot, `setup/02-matrix-setup.sh --apply`, reboot, `tools/test-matrix.sh`, then `setup/03-install-service.sh --apply`.
