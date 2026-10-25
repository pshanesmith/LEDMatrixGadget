# LEDMatrixGadget: Design

A 64x64 RGB LED panel driven by a Raspberry Pi Zero 2 W that shows the date, an animated weather scene, and Home Assistant status at a glance. This document describes the system as built and the reasons behind its main choices. Day-to-day operation is in [docs/operations.md](docs/operations.md); wiring and panel checks are in [docs/hardware.md](docs/hardware.md).

## 1. Goals and non-goals

Goals
1. Glanceable: date, current weather, today's low and high, the next change in conditions, and three Home Assistant indicators (unlocked locks, lights on, back-yard gates).
2. Always on, self-recovering, low maintenance, low power.
3. Develop and test almost everything **without the panel attached** (emulator backend).

Non-goals: touch or button input, voice, multiple pages, cloud accounts for weather, anything that needs more than a read-only Home Assistant token.

## 2. Hardware and host

| Item | Detail |
|---|---|
| Host | Raspberry Pi Zero 2 W (4x Cortex-A53 at 1 GHz, about 462 MB usable RAM with `gpu_mem=16`) |
| OS | Debian 13 "trixie", aarch64, Raspberry Pi OS Lite (no desktop), Python 3.13 |
| Driver board | Adafruit RGB Matrix Bonnet (#3211) |
| Panel | 64x64 HUB75, 2 mm pitch (#5362), 1/32 scan, uses the **E address line** |
| Library | [hzeller/rpi-rgb-led-matrix](https://github.com/hzeller/rpi-rgb-led-matrix) C++ library and Python bindings, pinned to one commit |

Hardware requirements for this panel and Bonnet:

- **E address line:** bridge the Bonnet's centre "E" pad to the "8" pad above it. Without it the panel shows only half the rows, repeated.
- **PWM bridge (flicker):** bridge GPIO 4 to GPIO 18 on the Bonnet and use `--led-gpio-mapping=adafruit-hat-pwm`. `adafruit-hat` remains selectable as a fallback.
- **Onboard sound off:** it uses the same PWM hardware. `dtparam=audio=off` and a `snd_bcm2835` blacklist.
- **Reserved CPU core:** `isolcpus=domain,managed_irq,3 nohz_full=3 rcu_nocbs=3 irqaffinity=0,1,2` gives the panel refresh thread core 3 to itself. The library pins and prioritises that thread itself, so the service must not restrict CPU affinity.
- **Panel geometry:** `rows=64`, `cols=64` (the library counts the panel's full height). Default row-address type, multiplexing and RGB order are correct for this panel.
- **Power:** a 64x64 panel can draw about 4 A at full white. The supply is rated 4 A, feeds the Bonnet's terminal block, and powers the Pi through the Bonnet (never also connect USB power). Brightness is capped (default 50%, halved after sunset, hard ceiling 70%) and the scene avoids large white areas.

## 3. Screen layout (64x64)

The whole frame is composed with Pillow in an RGB image and pushed to the panel with a vsync swap. Regions are fixed in `layout.py`.

```
y=0  +----------------------------------------------------------+
     | SAT 10/24 (date overlay)          61 (temperature)       |
     |   animated Blue Ridge scene, 64 x 35, behind both labels |
     |   sky + sun/moon/stars + clouds + mountains + weather FX |
y=35 +----------------------------------------------------------+
     | v40  ^56    low (cool blue), high (warm orange)    8 rows |
y=43 +----------------------------------------------------------+
     | [icon 9x9]  SNOW 6P   next change: icon + hour    10 rows |
y=53 +----------------------------------------------------------+
     | [lock] 0     [bulb] 6      [gate]                 11 rows |
y=63 +----------------------------------------------------------+
```

- **Fonts and icons:** an own 5-row proportional bitmap font defined in code (upper case, digits, `/ : . - + ?` and arrows) and palette-character icons, so there are no font files or antialiasing. The temperature in the scene uses built-in 3x5 digits.
- **Date:** weekday and month/day (`SAT 10/24`) overlaid at the scene's top left with the same white-on-dark-outline treatment and 1 px margin as the temperature, so it reads over day and night skies; recomputed every frame from the forecast location's local time.
- **Temperature row:** low and high from the day's forecast, centred in each half so `-12` and `105` fit.
- **Next change:** the first forecast hour within 24 h whose condition category differs from the current one, shown as the new condition's 9x9 icon and the hour (`6P`, `11A`). If nothing changes, `STEADY 24H` with the current condition's icon. A clear sky is drawn as a moon between sunset and sunrise.
- **Home Assistant row:** three cells about 21 px wide. Lock: count of locks that are unlocked, jammed or in motion (red when nonzero, dim at zero, an amber `?` if a lock is offline). Bulb: room lights on. Gate: green when closed, slowly blinking red when any configured gate is open, amber when its state is unknown. Counts clamp at 99.
- **Staleness dot** (bottom-right pixel): amber when Home Assistant data is older than 5 minutes, red when weather is older than 3 hours (red wins), grey when a source has never produced data. Missing data draws a dim `--`.
- **Brightness:** `brightness_for()` returns the configured level by day, half of it between sunset and sunrise, never above `max_brightness`; the loop applies it every 30 s.

## 4. Weather scene

Procedural pixel art with no image assets. Layers, back to front:

1. **Sky:** vertical gradient from sun elevation (sunrise and sunset come from the forecast) and cloud cover; dark navy with stars at night.
2. **Sun or moon:** the sun moves along an arc between sunrise and sunset; the moon follows the date's phase. Both fade under heavy cloud.
3. **Clouds:** drifting sprites scaled by cloud cover and wind; overcast and precipitation use wrap-around cloud decks.
4. **Mountains:** three parallax Blue Ridge-style ridges (rounded, forested, fading to blue haze in the distance) and pine silhouettes, from seeded shapes so the skyline never changes. Foliage is a seeded per-pixel mix of tree colours whose proportions follow the day of the year (green in summer; red, orange, yellow and green in October; brown and bare in winter; fresh greens and blossom in spring). Snow only dusts the crests while it is snowing.
5. **Weather effects:** per condition.

| Condition | WMO codes (Open-Meteo) | Effect |
|---|---|---|
| clear | 0, 1 | Sun or moon, twinkling stars, a sparkling sun, one slow wisp of cloud, a shooting star every 23 s at night |
| partly cloudy | 2 | Several drifting clouds |
| overcast | 3 | Dense grey cloud decks, dim sky |
| fog | 45, 48 | Drifting translucent bands, faded mountains |
| rain | 51-67, 80-82 | Falling streaks; count and length scale with the code's intensity |
| snow | 71-77, 85, 86 | Drifting flakes |
| thunderstorm | 95-99 | Heavy rain, dark sky, lightning flashes |

- **Pure function:** `render_state(state, t)` depends only on its inputs, which makes it testable and lets the emulator export previews (`docs/previews/`). Static layers are cached per (sun elevation in 1/40 steps, condition); the temperature is drawn top right.
- **Cross-fade:** `SceneRenderer` remembers the last drawn state so a condition change blends into the new picture over 2 s; a backwards or over-5-second jump in time resets without fading.
- **Ambient motion:** a clear sky once looked frozen (0% of scene pixels changed over 10 s), so stars twinkle at individual rates, the sun's eight rays alternate twice a second, and one cloud always drifts.
- **Cost:** a frame renders in about 3-5 ms in the emulator and about 11 ms in the live loop (which includes pushing the frame to the panel), against 66 ms at 15 fps.
- Wind from the forecast biases rain, snow and cloud drift.

## 5. Data sources

### Weather: Open-Meteo
- No key, free for non-commercial use, one request returns everything:
  `GET https://api.open-meteo.com/v1/forecast?latitude=..&longitude=..&current=temperature_2m,weather_code,cloud_cover,wind_speed_10m,is_day&hourly=weather_code,temperature_2m,precipitation_probability&daily=temperature_2m_max,temperature_2m_min,sunrise,sunset&temperature_unit=fahrenheit&timezone=auto&forecast_days=2`
- `WeatherFetcher` refreshes on a daemon thread every `[weather].refresh_seconds` (900), backs off 15/30/60/120/240/300 s after failures, keeps the last good snapshot, and writes it atomically to a JSON cache so a boot without network still has data. Datetimes carry the location's UTC offset.

### Location: ZIP lookup, once
The user enters a ZIP or postal code; `tools/geocode.py ZIP [--country US] [--units ...] [--yes]` resolves it **once, at setup time** and writes `[location]` (postal code, latitude and longitude rounded to 3 decimals, a label, units) into `config.toml`. The running service never geocodes. A ZIP centroid is accurate enough for a forecast and no street address is sent anywhere.
- **Providers (free, no key):** Open-Meteo geocoding first, then OpenStreetMap Nominatim (structured `postalcode` + `country`; at most 1 request per second, descriptive User-Agent; no personal contact is embedded, `MATRIX_CONTACT` optionally appends one).
- **Country filter:** a bare postal code is ambiguous across countries (for example `10001` also matches Spain and France), so results are filtered by `countryCode` and again client side, and the user confirms the match.
- **Manual override:** `--lat/--lon/--label`, or edit the file. Input is validated before any request.
- **Editing:** only the `[location]` table is rewritten (comments and other sections untouched) and the result is re-parsed before saving. Timezone is not configured: Open-Meteo's `timezone=auto` derives it.
- If both providers fail the tool says so; the service refuses to run weather without a location rather than guessing.

### Home Assistant
- **Token:** a read-only long-lived access token (profile > Security > Long-lived access tokens), supplied only as `MATRIX_HA_TOKEN` in the environment or an env file: `secrets.env` during development, `/etc/matrix/secrets.env` (mode 0600, root) on the Pi, loaded by systemd. It is never in code or git (`.gitignore` excludes `secrets.env`, `*.env` other than examples, `*.token` and the local `config.toml`; `.githooks/pre-commit` blocks a commit that contains a secrets file or a JWT-shaped string). Tools never accept it as an argument, and nothing prints it: errors are raised without URL or headers, and the config object omits it from `repr`. If it leaks, delete it in Home Assistant and create a new one.
- **Base URL:** configurable, never hard-coded. Resolution order: `MATRIX_HA_URL`, then `[homeassistant].url`, then `http://homeassistant:8123`. The bare hostname resolves through local DNS (mDNS `homeassistant.local` did not). Scheme defaults to http, a trailing slash is stripped, and invalid values fail at startup.
- **Poll:** `HomeAssistantPoller` makes one `POST /api/template` every `poll_seconds` (15). The template returns locks that are not `locked` as `[id, state]`, ids of lights that are `on` and have no `entity_id` attribute (this drops light groups), and the `states()` of the configured gates. Glob exclusions (`exclude_locks`, `exclude_lights`, `fnmatch` semantics on entity ids) are applied in Python, so no user text enters the template; gate ids are the only interpolated values and must match `^[a-z0-9_]+\.[a-z0-9_]+$`. The payload is a few hundred bytes.
- **Counting rules:** lock states `unlocked`, `jammed`, `unlocking`, `locking` count as unlocked; `unavailable` and `unknown` count separately (`locks_unavailable`) because an offline lock is not known to be locked. Gates are open on `on`, `open`, `opening`; `unavailable` or `unknown` (including a misspelt id) means `gates_unavailable`. Unavailable lights are not counted as on.
- **Lights:** Home Assistant reports many `light` entities that are not room lights (for example switch status-LED bars and network-gear LEDs, plus groups). The default `exclude_lights` globs are `light.*_rgb_indicator`, `light.*_ap_led`, `light.*_led`; on the reference system they cut about 114 entities to about 56 real lights.
- **Discovery:** `tools/ha-discover.py [--toml] [--all-lights] [--all-doors]` lists locks, room lights (noise hidden) and gate candidates (door-class `binary_sensor` and `cover` entities, preferring ones named "gate") and can print a config snippet.
- **Failures:** 5 s timeout; the delay doubles from `poll_seconds` up to 60 s; the last good snapshot is kept and cached to disk. A 401/403 logs one error (naming `MATRIX_HA_TOKEN`) and retries every 300 s; unreachable host and bad responses log on the first failure and every tenth.

## 6. Software architecture

```
 +------------------+     +-----------------+
 | weather fetcher  |---->|                 |
 | (thread, 15 min) |     |  latest()       |   immutable snapshots,
 +------------------+     |  snapshots      |   JSON cache on disk
 | HA poller        |---->|  (per source)   |   for restarts
 | (thread, 15 s)   |     +--------+--------+
 +------------------+              |
                                   v
                  +----------------------------+
                  | Runner (main thread)       |
                  |  AppState -> Compositor    |-->  Display backend
                  |  scene + widgets, 15 fps   |      - hardware (rgbmatrix)
                  +----------------------------+      - emulator (PNG/GIF)
```

| # | Decision | Rationale |
|---|---|---|
| D1 | Python 3 with hzeller's bindings and Pillow | Official bindings, fast iteration, Pillow is plenty at 64x64. |
| D2 | `Display` abstraction with hardware and emulator backends | Everything except the final push can be developed and tested without the panel. |
| D3 | Fetchers run in threads and publish immutable snapshots; the render loop never waits on the network | A slow Wi-Fi call must not stutter the animation. |
| D4 | Last good data is cached to disk | A boot with no network still shows data. |
| D5 | One systemd service, `Type=notify`, watchdog, `Restart=always` | Self-healing: a hung loop or crash is restarted. |
| D6 | Non-secret config in `config.toml`, secrets only in the environment | Keeps the token out of the repository. |
| D7 | The service starts as root for GPIO timing; the library drops to `daemon` after setup | Least privilege for the network-facing code. Needs code readable by `daemon`, hence `/opt/matrix`. |
| D8 | Minimal dependencies: Pillow (system package) only; stdlib `urllib` and `tomllib` | Small update surface and RAM. |
| D9 | Brightness capped and dimmed after sunset | Power budget, heat, night glare. |
| D10 | App memory budget under about 100 MB | Measured about 35 MB of about 460 MB total. |

**Runner (`runner.py`).** Builds an `AppState` (clock in the forecast location's timezone, latest weather, latest home snapshot) every frame and draws it through the `Compositor`, which calls one drawer per layout region. Slow frames are dropped, never queued. A widget exception shows the last good frame and logs; after about 10 s of continuous failure it raises so systemd restarts the process. The loop notifies systemd (`READY=1`, `WATCHDOG=1` about once a second, `STOPPING=1`), logs a performance line every minute (render average and maximum, overruns, RSS, free memory, temperature, throttle flags), and re-reads the config on `SIGHUP`. The display is created first (privileges drop on hardware) and the network threads start afterwards, so a down network never delays the first frame.

**Configuration** (`config.toml`; see `config/config.example.toml`): `[location]`, `[display]` (`rows`, `cols`, `chain_length`, `parallel`, `hardware_mapping`, `gpio_slowdown`, `pwm_bits`, `pwm_lsb_nanoseconds`, `limit_refresh_hz`, `row_address_type`, `multiplexing`, `rgb_sequence`, `brightness`, `max_brightness`, `scene_fps`, `safe_image_copy`, `drop_privileges`), `[weather]`, `[homeassistant]` (`url`, `poll_seconds`, `gates`, `exclude_locks`, `exclude_lights`). Unknown keys are rejected. `MATRIX_HA_URL`, `MATRIX_HA_TOKEN` and `MATRIX_DROP_PRIVILEGES` come from the environment. `tools/set-config.py` changes one value safely (idempotent, repairs duplicate sections, validates before saving).

**Repository layout**

```
README.md  DESIGN.md
setup/        01-pi-setup.sh  02-matrix-setup.sh  03-install-service.sh  lib/common.sh
tools/        test-matrix.sh  matrix_hello.py  preview.py  geocode.py  ha-discover.py
              set-config.py  soak-report.sh
src/matrix_display/
  __main__.py  runner.py  app.py  config.py  state.py  layout.py  forecast.py  samples.py
  display/{base,emulator,hardware}.py      data/{weather,geocode,homeassistant}.py
  scene/{sky,mountains,clouds,particles,palette,digits,scene,previews}.py
  ui/{fonts,icons,widgets,variants}.py
config/       config.example.toml  secrets.env.example
systemd/      matrix-display.service
docs/         operations.md  hardware.md  previews/
tests/        unit tests, fixtures/, golden/
.githooks/    pre-commit
```

## 7. Raspberry Pi setup (`setup/01-pi-setup.sh`)

Idempotent; **dry run by default** (`--apply` needs root and a typed confirmation); refuses to run on a non-Pi; snapshots the package and service state and timestamped copies of the boot files before changing anything; never reboots. Steps (`--only` / `--skip` select them): `purge boot system update autoupdate verify`. `--aggressive` adds a second tier of removals. Shared helpers (diff-then-write, marked blocks, backups, dry-run wrapper) are in `setup/lib/common.sh`.

- **Purge:** explicit package lists, never wildcards. Default tier (about 125 packages including dependents): Bluetooth (`bluez`, `bluez-firmware`); the Pi 5 kernel and all kernel headers (this board runs the `rpi-v8` kernel, which is kept; apt never auto-removes kernels, so they are named); firmware for USB Wi-Fi dongles (`firmware-brcm80211`, which drives this board's Wi-Fi, is kept); `rpi-eeprom` (Pi 4/5 bootloader); camera and V4L tools; unused desktop-style utilities; `gdb`, `apt-listchanges` and similar. `--aggressive`: also `cloud-init`, `rpi-connect-lite` and `rpi-usb-gadget`, left off by default because they may be how you reach the Pi.
- **Safety net:** `apt-get -s` simulates the removal including dependents first; if anything on the protect list (SSH, NetworkManager, wpa_supplicant, Wi-Fi firmware, the running kernel, Python, build tools, `git`, `sudo`, systemd, `avahi-daemon`, `raspi-config`, `raspi-utils` and similar) would go, the script aborts with nothing changed.
- **Kept on purpose:** `avahi-daemon` (`hostname.local` SSH), NetworkManager, SSH, `raspi-config`, build tools, `htop`, `nano`, `strace`.
- **`config.txt`:** conflicting lines are commented out with a `#matrix-setup#` prefix and a marked block in `[all]` sets `dtoverlay=disable-bt`, `dtparam=audio=off`, `camera_auto_detect=0`, `display_auto_detect=0`, `gpu_mem=16` (the KMS display overlay is dropped; the board is headless).
- **System:** disables `e2scrub`, `man-db.timer`, `NetworkManager-wait-online` (faster boot; the app tolerates starting offline) and Bluetooth units; `led-off.service` turns the ACT LED off; the journal is made **persistent** and capped at 30 MB. Raspberry Pi OS ships `Storage=volatile`, which erased logs on every reboot and made freezes undiagnosable; the drop-in overrides it. The hardware watchdog needs no change: `raspberrypi-sys-mods` already enables it (60 s).
- **Update:** `apt-get update && full-upgrade` (keeping existing config files), then `git python3-dev python3-venv python3-pil cython3 python3-setuptools`. 
- **Automatic updates (`autoupdate` step, `--no-auto-updates` skips it):** installs `unattended-upgrades` (11 small packages) and configures it through apt drop-ins. Allowed sources: Debian stable, its security and stable-updates suites, and the Raspberry Pi repository (kernel, firmware, raspi tools); never a new Debian release. The daily apt timer is moved to 03:00 (it was 06:00 plus up to an hour, and the periodic apt settings were unset, so nothing ran at all). A kernel or firmware update needs a reboot, which happens at **04:00 and only if nobody is logged in** (an open SSH session postpones it a night); the display is dark for about a minute. Config files are kept (`--force-confold`) and unused kernels and dependencies are removed. Nothing is held back: Pillow upgrades like any other package, because frames no longer depend on its internals (section 8). Application code is not updated automatically: it comes from GitHub when `setup/03-install-service.sh` is run.
- Swap is already a zram device and `/` is already `noatime`; nothing to change. No `libnss-mdns`: `homeassistant` resolves through DNS.

## 8. Panel library and boot settings (`setup/02-matrix-setup.sh`)

Automates Adafruit's "Install Manually" guide, adapted to Debian 13 and the library's 2026 build system. Idempotent, dry run by default. Steps: `deps library python boot verify`.

- **Source:** a shallow fetch of one pinned commit (`14464b3ad8`, `RGBMATRIX_REF`) into `<repo>/vendor/`. A full clone failed with TLS errors on this network, so the fetch uses HTTP/1.1 with retries. `make` builds the library and the demo binaries.
- **Python bindings:** upstream builds them with scikit-build-core and CMake, installed with `pip install <source dir>` into the project's virtualenv (the older `make install-python` flow is gone). Builds run single-job at low priority because a compile nearly exhausts RAM, and the script warns when under 250 MB is free.
- **Pillow must come from apt:** the bindings' C shim includes Pillow's private `Imaging.h` and reads its in-memory image struct, so it must be compiled against, and run with, the same Pillow (pip wheels ship no such header, and the struct layout differs between versions). The script installs `python3-pil` (Debian 11.1.0, which ships the header), makes the venv `--system-site-packages` and removes any pip-installed Pillow from it.
- **Boot edits:** `/etc/modprobe.d/blacklist-rgb-matrix.conf` (`snd_bcm2835`) plus an initramfs refresh; the `isolcpus` set from section 2 appended to `cmdline.txt` only for keys not already present (`--no-isolcpus` skips it). Tested on copies of the real files.
- `config.txt` audio settings come from the Pi setup script; the script checks for them.
- **Pillow independence:** the bindings' default fast path reads Pillow's private image struct, whose layout can change between Pillow versions, so an automatic Pillow upgrade could make it draw garbage. `HardwareDisplay` therefore pushes frames with `SetImage(..., unsafe=False)`, which uses only Pillow's public API (`[display] safe_image_copy`, default true). Cost: a 4,096-pixel loop per frame, measured at about 8 ms as plain Python on this Pi (the bindings compile it, so it is no slower) against a 66 ms frame budget; it runs on the application's cores, not the reserved panel core. The compiled bindings still contain the shim, which only needs Pillow's header at build time. The Pillow version used for a build is recorded in `/var/lib/matrix-display/pillow-version` for information; with `safe_image_copy = false` a mismatch would matter and the bindings should be rebuilt (`sudo ./setup/02-matrix-setup.sh --apply --rebuild`).

## 9. Hardware test and flicker tuning

`tools/test-matrix.sh` (run with `sudo`) follows Adafruit's "Testing Install": it runs the library's `demo` (`-D 0` rotating square, `-D 4` pulsing colour, `-D 5` grayscale) with this panel's flags, asks what you see, then runs `tools/matrix_hello.py`, which pushes a diagnostic pattern (red top half, blue bottom half, border, RGB bars, moving square) through the project's own `HardwareDisplay`. Settings can be overridden by environment (`SLOWDOWN`, `MAPPING`, `EXTRA`); results append to `docs/hardware-test-results.txt`. `docs/hardware.md` has the symptom table.

The panel is refreshed by software timing, so flicker is a trade between colour depth and refresh rate. With `pwm_bits = 11` the measured refresh was about 108 Hz; each bit removed roughly doubles it. Tuning order: lower `pwm_bits`, set `limit_refresh_hz` to hold a steady rate, raise `gpio_slowdown` for shimmering lines.

## 10. Service and deployment (`setup/03-install-service.sh`)

The service runs a **copy of the repository deployed from GitHub**, not a working checkout.

- **Layout:** `/opt/matrix` is a `git clone` (root-owned, world-readable) with `.venv/` (system-site-packages, editable install of the app, the compiled `rgbmatrix`) and `vendor/` inside, both gitignored. Config and token are in `/etc/matrix` (`config.toml`, `secrets.env`, and `deploy_key` for a private repository). Caches are in `/var/lib/matrix-display` (owned by `daemon`).
- **Installer:** dry run by default. It clones or fast-forwards `/opt/matrix` to `--ref` (default `main`) from `--repo` (default: the checkout's `origin`), creates the venv, installs the app, writes the unit and (re)starts the service. **Re-running it is the update procedure.** It is wrapped in a function so a `git pull` cannot rewrite the script while it runs. It never overwrites an existing `/etc/matrix` file. `--no-drop` keeps the service as root; `--uninstall` removes the unit.
- **Reusing the build:** a library compile takes 10+ minutes and nearly exhausts RAM, so when started from a checkout that already has a finished build the installer copies `vendor/` and the compiled `rgbmatrix` package instead (`--rebuild` forces a build).
- **Private repositories:** use an SSH URL; the installer creates a read-only per-repository deploy key and prints the public half to add on GitHub. A public repository needs nothing.
- **Unit (`systemd/matrix-display.service`):** `Type=notify` with `WatchdogSec=30`, `Restart=always`, `RestartSec=5`, `OOMScoreAdjust=-500`, `EnvironmentFile=/etc/matrix/secrets.env`, `NoNewPrivileges`, `PrivateTmp`, `ProtectHome=yes`, `ProtectSystem=full`, writable only in the state directory. No `CPUAffinity`.
- **Privilege drop:** the process starts as root and the panel library drops to `daemon` after GPIO setup, so the network-facing code is unprivileged; the code must stay readable to `daemon` afterwards, which is why the deployment is in `/opt` and not a private home folder. The installer checks the path and falls back to root if needed. After the drop the process can no longer read `vcgencmd` (`/dev/vchiq` is group `video`), so throttle flags are read by `tools/soak-report.sh` instead of the service log.
- **Logs:** journald (persistent, 30 MB cap), INFO. `SIGHUP` reloads configuration. On clean shutdown the panel is blanked.
- **Health check:** `tools/health-check.sh`, run by `matrix-health.timer` ten minutes after every boot (so after an automatic update and reboot) and daily at 08:00, verifies that the service is running and producing about 900 frames a minute, that the kernel logged no hung tasks, memory exhaustion, undervoltage, SD-card or Wi-Fi-chip errors since boot, that `isolcpus` and the sound blacklist survived the update, and and (informationally) whether Pillow changed since the build. It logs under the tag `matrix-health` (errors at error priority) and fails the unit so `systemctl --failed` shows it.
- **Health review:** `tools/soak-report.sh [since]` summarises boots, service restarts, warnings, kernel problems (hung tasks, SD card, Wi-Fi chip, memory, undervoltage), the worst performance values, throttle flags and free memory.

## 11. Testing

About 216 tests run in a few seconds with `.venv/bin/pytest`, none needing the panel or network:
- **Units:** condition mapping, next-change search, config parsing and validation, URL normalisation, the config setter, the geocoder (recorded Open-Meteo and Nominatim responses, offline failures), the weather parser and fetcher (a recorded forecast), the Home Assistant poller (a fake local HTTP server: locks, gates, 401, refused connection, bad JSON, backoff, token never in logs or `repr`), the runner (pacing, dropped frames, error resilience, reload, systemd notifications), and the setup scripts' file-editing helpers on copies of real boot files.
- **Rendering:** golden-image tests for the widgets (`tests/golden/`, regenerate with `UPDATE_GOLDEN=1` and review the PNGs), text-overflow tests over value ranges, brightness tests, and scene tests (every condition renders, determinism, per-frame cost, ambient motion).
- **On the device:** `tools/test-matrix.sh`, then a 24-48 hour soak reviewed with `tools/soak-report.sh`.

## 12. Risks and open questions

| Risk / question | Status |
|---|---|
| **Flicker.** The default refresh was only about 108 Hz and minor flicker was seen. | **Open.** `pwm_bits = 9` (several times faster refresh) is applied here; confirmation pending. Fallbacks: `limit_refresh_hz`, `gpio_slowdown = 3`, `pwm_bits = 10`. |
| **Memory.** About 460 MB total; the app needs about 35 MB, but large programs (Claude Code and helper sessions during development) filled RAM and swap, and the Pi thrashed: SSH drops, Wi-Fi chip errors, and hardware-watchdog resets. | **Mitigated:** `gpu_mem=16`, single-job builds, `OOMScoreAdjust=-500`, persistent journal. Keep large programs off the Pi. Confirm in the soak. |
| **Unexplained resets or freezes.** Early logs were lost to the volatile journal. | **Open until the soak is clean** (`tools/soak-report.sh`). |
| **Power (4 A supply).** No throttling or undervoltage seen; not measured with large white areas at higher brightness. | **Partly open:** watch `throttled` during snow and lightning. |
| **Bindings tied to Debian's Pillow.** The fast image path reads Pillow's private struct. | **Mitigated:** frames use Pillow's public API by default (`safe_image_copy`), so Pillow can upgrade freely. Cost about 8 ms per frame, to be confirmed on the panel after the next deployment (watch the `render avg` log line). |
| **Automatic upgrades and reboots.** Kernel and firmware updates from the Raspberry Pi repository arrive unattended and reboot the Pi at 04:00. A bad update could change panel timing or leave the display down. | **Open, untested:** the first automatic run has not happened yet. The health check runs after the reboot and logs a failure; `--no-auto-updates` or disabling `apt-daily-upgrade.timer` backs out. Review `journalctl -t matrix-health` and `/var/log/unattended-upgrades/` after the first runs. |
| **Pinned panel library.** Commit `14464b3ad8`; the bindings' build system changed in February 2026. | Bump deliberately and retest (`tools/test-matrix.sh`). |
| **SD card wear, unbranded card.** Persistent journal is capped, swap is zram, `/` is `noatime`; no I/O errors seen. | Watch; back up `/etc/matrix`. |
| **Behaviour not yet seen on the panel:** the open-gate blink, stale-data dots, Home Assistant 401 or unreachable, an internet outage. | **Open:** covered by tests and fixtures only; exercise during the soak. |
