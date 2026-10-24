# Home Status Matrix: Design

A 64x64 RGB LED panel driven by a Raspberry Pi Zero 2 W that shows the date, an animated weather scene, and Home Assistant status at a glance.

## 1. Hardware and host facts

| Item | Detail |
|---|---|
| Host | Raspberry Pi Zero 2 W (4x Cortex-A53 at 1 GHz, **~462 MB usable RAM** (415 MB before `gpu_mem=16`)) |
| OS (observed) | Debian 13 "trixie", aarch64, Pi OS Lite style (no desktop), Python 3.13 |
| Boot files | `/boot/firmware/config.txt`, `/boot/firmware/cmdline.txt` (Bookworm/Trixie layout) |
| Driver board | Adafruit RGB Matrix Bonnet (#3211) |
| Panel | 64x64 HUB75, 2 mm pitch (#5362), 1/32 scan, needs the **E address line** |
| Library | [hzeller/rpi-rgb-led-matrix](https://github.com/hzeller/rpi-rgb-led-matrix) + Python bindings |

State on the host today (before setup): 633 packages, Bluetooth active (`bluez` installed), `dtparam=audio=on`, `vc4-kms-v3d` overlay loaded, HDMI and camera auto-detect on, no `isolcpus`. The setup scripts must change each of these.

Hardware notes to carry into the scripts and the wiring checklist (to be re-verified against the Adafruit guides in Phase 2):

- **64x64 needs the E line.** The Bonnet needs a solder jumper bridging the pad for address line E (pin 8 to E). Without it the panel shows only half the rows, duplicated. **Status: done by the user.**
- **Flicker-free output needs the PWM mod.** Solder a bridge between GPIO 4 and GPIO 18 on the Bonnet, then use `--led-gpio-mapping=adafruit-hat-pwm`. **Status: done by the user**, so the default mapping is `adafruit-hat-pwm`; `adafruit-hat` stays selectable by config as a fallback for debugging.
- **Onboard sound must be off** (`dtparam=audio=off` and blacklist `snd_bcm2835`). It uses the same PWM hardware.
- **Reserve one core:** `isolcpus=3` in `cmdline.txt`, and `--led-slowdown-gpio` tuned (start at 2; the Zero 2 W is Pi 3 class).
- **Power:** a 64x64 panel can draw about 4 A at full white. Feed 5 V (the user's supply is **rated 4 A**, which is the minimum; 5 A would be preferred) to the Bonnet's terminal block; it powers the Pi too. Never also connect USB power. Cap brightness (default 50%) and cap the scene palette so average draw stays low.
- **Privilege:** the library drops root after init (`--led-no-drop-privs` off), so the service runs as root with privilege drop to a `daemon` user, or as a dedicated user with the needed caps. Decision in section 6.

## 2. Goals and non-goals

Goals
1. Glanceable: date, weather now, today's low and high, next weather change, and three Home Assistant indicators.
2. Always on, self-recovering, low maintenance, low power.
3. Develop and test most of the code **without the panel attached** (emulator backend).

Non-goals (v1): touch or button input, voice, multiple pages or rotation, cloud accounts for weather, anything that needs more than a read-only HA token.

## 3. Screen layout (64x64)

All coordinates are pixels; the whole frame is composed with Pillow in an RGB image and pushed with a vsync swap.

```
y=0  +----------------------------------------------------------+
     | FRI 10/23                          (date, 4x6 font) 7 rows |
y=7  +----------------------------------------------------------+
     |                                                          |
     |   animated mountain scene, 64 x 28                       |
     |   sky + sun/moon/stars + clouds + mountains + weather FX |
     |                                                          |
y=35 +----------------------------------------------------------+
     | v48  ^71    low (cool blue), high (warm orange)  8 rows   |
y=43 +----------------------------------------------------------+
     | [icon 9x9]  RAIN 3P    next change: icon + time 10 rows   |
y=53 +----------------------------------------------------------+
     | [lock] 2     [bulb] 7      [gate] OPEN/SHUT    11 rows   |
y=63 +----------------------------------------------------------+
```

- **Date** shows weekday and numeric month/day, for example `FRI 10/23`. Midnight rollover is handled by recomputing every frame from the local clock.
- **Current temperature** is drawn inside the scene's top-right corner so the scene doubles as the "now" display.
- **Temp row** uses small up/down arrows and two digits each; values over 99 or below 0 fall back to a narrower format.
- **Next change** compares the hourly forecast's condition category with the current one and shows the first hour it differs within the next 24 h (icon of the *new* condition and the hour in `3P` / `11A` style). If there is no change in 24 h, show `CLEAR ALL DAY` style text or a steady icon with `24H+`.
- **Home Assistant row** has three equal cells, about 21 px each. Lock and bulb cells are a 7x9 icon plus a one or two digit count. Zero is dimmed, nonzero locks are red or amber so an unlocked house stands out. The gate cell is an icon: green closed gate, or red open gate that blinks slowly.
- A small **staleness dot** in the bottom-right pixel turns amber when HA data is older than 5 min and red when weather is older than 3 h, so a dead network is visible rather than silently showing old data.

Fonts: bitmap fonts only (BDF from the rgbmatrix repo `fonts/`, 4x6 and 5x7). No antialiasing, no TTF at this size.

## 4. Weather scene

Procedural pixel art, no image assets, so it is easy to tweak and tiny in memory. Layers, back to front:

1. **Sky:** vertical gradient chosen from time of day (sunrise and sunset from the forecast API) and cloud cover. Night is dark blue with a handful of twinkling stars unless overcast.
2. **Celestial body:** sun by day, moon (with phase approximated) by night; hidden under heavy cloud.
3. **Far clouds:** slow horizontal drift; density from cloud cover percentage.
4. **Mountains:** two or three parallax ridgelines in fixed procedural shapes (seeded noise so the skyline never changes). Snow caps on the top ridge; snow accumulates (white fill) when the condition is snow.
5. **Foreground:** a few pine silhouettes.
6. **Weather FX:** per-condition particle systems.

| Condition category | WMO codes (Open-Meteo) | Effect |
|---|---|---|
| clear | 0, 1 | Sun or moon and stars, optional light shimmer |
| partly cloudy | 2 | A few drifting clouds |
| overcast | 3 | Dense gray cloud layer, dim sky |
| fog | 45, 48 | Translucent horizontal bands that drift, mountains faded |
| drizzle / rain | 51-67, 80-82 | Falling streaks; count and length scale with intensity |
| snow | 71-77, 85, 86 | Slow drifting flakes, accumulation on ridges |
| thunderstorm | 95-99 | Heavy rain, dark clouds, occasional lightning flash |

Animation: fixed 15 fps for the scene (enough for pixel art, saves CPU), a scene time step independent of frame time, and a 2-second cross-fade when the condition category changes after a refresh. The scene renderer is a pure function `(state, t) -> Image`, which makes it testable and lets the emulator export PNG and GIF previews.

Wind (from the forecast) adds a horizontal bias to rain, snow and cloud drift.

## 5. Data sources

### Weather: Open-Meteo
- No API key, no account, free for non-commercial use, and one call returns everything needed:
  `GET https://api.open-meteo.com/v1/forecast?latitude=..&longitude=..&current=temperature_2m,weather_code,cloud_cover,wind_speed_10m,is_day&hourly=weather_code,temperature_2m,precipitation_probability&daily=temperature_2m_max,temperature_2m_min,sunrise,sunset&temperature_unit=fahrenheit&timezone=auto&forecast_days=2`
- Refresh every **15 min** (current conditions) from one request. On failure keep the last good payload and retry with backoff up to 5 min.
- Location and units come from config. An alternative provider (for example NWS for US-only accuracy) can slot in behind the same interface, but is not planned.

### Location: ZIP code lookup (free, one-time)
The user enters a **ZIP / postal code** instead of coordinates. It is geocoded **once, at setup time**, and the resulting latitude, longitude and display label are written to `config.toml`. The running service never geocodes and never stores the ZIP lookup traffic, so there is no runtime dependency or ongoing cost. A ZIP centroid is plenty accurate for a weather forecast, and it avoids sending a street address to anyone.

Both providers are free and need no API key, so the paid-API exception is not needed:

| Order | Provider | Query | Notes |
|---|---|---|---|
| 1 | Open-Meteo Geocoding | Postal code or place name | Same vendor as the weather API. |
| 2 | OpenStreetMap Nominatim | Structured `postalcode` + `country` | Usage policy requires a descriptive `User-Agent` and at most 1 request/second, which a one-time lookup satisfies. |

Behavior
- `tools/geocode.py 10001` (optionally `--country US`, default from config) tries the providers in order, prints the top matches with their labels (for example "New York, New York"), and asks the user to confirm. Several places can share a postal code outside the US, hence the confirmation. The install script calls it when `[location]` is not yet set.
- Result is stored as:
  ```toml
  [location]
  postal_code = "10001"
  latitude = 40.748
  longitude = -73.994
  label = "New York, NY"        # for humans reading the config; not shown on the panel
  units = "fahrenheit"
  ```
- **Manual override:** if `latitude` and `longitude` are present the geocoder is skipped, and the user can also type a city name instead of a ZIP.
- **Country filter (verified):** a bare postal code is ambiguous across countries. Open-Meteo returned New York plus matches in Spain (Cáceres) and France (Troyes) for `10001`; the tool therefore always filters by country (`countryCode` plus a client-side check). The tool therefore always filters by country (`[location].country`, default `US`) and still asks for confirmation. Nominatim with `postalcode=10001&country=US` returned a single correct result (New York, NY). Both endpoints worked without a key during planning.
- Failure handling: if both providers fail (offline), the tool says so and tells the user to retry or enter coordinates; the service refuses to start without a location rather than guessing.
- The timezone is not configured: Open-Meteo's `timezone=auto` derives it from the coordinates.

I have not exercised these APIs yet; the Phase 3 agent verifies the endpoints and response shapes (including postal-code support) against their documentation before relying on them.

### Home Assistant
- Read-only **long-lived access token**, handled as follows:
  - **Create:** Home Assistant profile (bottom-left) > Security > Long-lived access tokens > Create token. Name it `matrix-display`. For least privilege, create a separate non-admin HA user for it if that suits you.
  - **Store:** as `MATRIX_HA_TOKEN=...` in `secrets.env` in the project root during development, or `/etc/matrix/secrets.env` (`chmod 600`, root-owned) on the Pi. The systemd unit loads it with `EnvironmentFile=`. `config/secrets.env.example` documents the format with an empty value.
  - **Never in code or git:** the token is read only from the environment or those files. `.gitignore` excludes `secrets.env`, `*.env` (except `*.env.example`), `.env`, `*.token` and the local `config.toml`. A `.githooks/pre-commit` hook (enable with `git config core.hooksPath .githooks`) also blocks a commit that contains a secrets file or anything shaped like a JWT, which is what HA tokens are.
  - **Never on a command line or in logs:** tools do not accept the token as an argument (shell history), the app and tools never print it, and error messages omit the Authorization header. If it ever leaks, delete the token in HA and create a new one.
- Use a single `POST /api/template` call each poll. It does the counting on the HA side so the Pi transfers only a few bytes:
  ```jinja
  {{ {
    "locks_unlocked": states.lock | selectattr('state','eq','unlocked') | list | count,
    "lights_on": states.light | selectattr('state','eq','on') | list | count,
    "gates": [<configured gate entity ids> ] | map('states') | list
  } | tojson }}
  ```
- Poll every **15 s**. Sticking with REST polling for v1: simpler and robust. Upgrade path is the WebSocket `subscribe_entities` API if sub-second latency is ever wanted.
- **Gates:** exact entities are unknown. The config lists gate entity ids explicitly (binary sensors, covers or similar). A gate counts as open when its state is in `{open, opening, on}`. Phase 5 includes a discovery helper that lists candidates (`binary_sensor` and `cover` entities with "gate" in the name or device class `door`/`garage_door`) so the user can pick.
- **Locks:** `unlocked`, `jammed`, `unlocking` and `locking` count toward the number (attention needed). `unavailable`/`unknown` locks are counted separately (`locks_unavailable`) and shown with an amber `?`, because an offline lock is not known to be locked. Exclusions are fnmatch globs on entity ids.
- **Lights (found on the real system):** HA reports 114 `light` entities, but about half are not room lights: Inovelli switch status LEDs (`*_rgb_indicator`) and network gear LEDs (`*_ap_led`, `usw_*_led`). Counting them would roughly double the number. The default `exclude_lights` globs are `light.*_rgb_indicator`, `light.*_ap_led`, `light.*_led`; light groups are excluded too. About 56 real lights remain. `unavailable` lights are not counted as on.
- **Entities on the reference system:** a handful of door locks (`lock.*`) and door-class binary sensors for the back-yard gates (`on` = open). `tools/ha-discover.py` finds them; the entity ids are listed in `[homeassistant].gates` (the real ones are in your private `config.toml`).
- **Base URL:** `http://homeassistant:8123` (the bare hostname resolves via the local network's DNS; it resolved to the Home Assistant host during planning). `homeassistant.local` (mDNS) did not resolve here, so it is not used. The host is **configurable**, never hard-coded:
  ```toml
  # config.toml
  [homeassistant]
  url = "http://homeassistant:8123"   # scheme, host (name or IP) and port
  ```
  Resolution order: `MATRIX_HA_URL` environment variable (from `/etc/matrix/secrets.env`), then `[homeassistant].url` in `config.toml`, then the default above. A trailing slash is stripped, a missing scheme defaults to `http://`, and the value is validated at startup with a clear error. A change takes effect on `SIGHUP` (no reinstall). The install script prompts for the URL (default shown) and writes it to `config.toml`. Phase 1 re-checks resolution after the debloat.

## 6. Software architecture

```
 +------------------+     +-----------------+
 | weather fetcher  |---->|                 |
 | (thread, 15 min) |     |  State store    |   immutable snapshot,
 +------------------+     |  (in memory +   |   atomic swap, JSON cache
 | HA poller        |---->|   JSON cache)   |   on disk for restart
 | (thread, 15 s)   |     +--------+--------+
 +------------------+              |
                                   v
                  +----------------------------+
                  | Renderer (main thread)     |
                  |  layout + scene + widgets  |-->  Display backend
                  |  pure functions of state   |      - RGBMatrix (hardware)
                  +----------------------------+      - Emulator (PNG/GIF/window)
```

Decisions

| # | Decision | Rationale |
|---|---|---|
| D1 | **Python 3** with hzeller's Python bindings, Pillow for compositing | Official bindings, quick to iterate, Pillow is fine at 64x64. |
| D2 | **Display abstraction** with `hardware` and `emulator` backends | Lets agents build and test everything without the panel. |
| D3 | Fetchers run in **threads** and publish immutable snapshots; the render loop never blocks on network | A slow Wi-Fi call must not stutter the animation. |
| D4 | Last good data is **cached to disk** | Boot while the network is down still shows something. |
| D5 | One **systemd service**, `Restart=always`, plus a watchdog ping | Self-heal on crashes. |
| D6 | Config in `config.toml` (non-secret) and `/etc/matrix/secrets.env` (token, `0600`) | Keep secrets out of the repo. |
| D7 | Run the service as **root** with the library's privilege drop to `daemon` after init | Required for GPIO timing; simplest supported path. |
| D8 | Minimal dependencies: `pillow` only; HTTP uses stdlib `urllib`, TOML uses stdlib `tomllib` | Small update surface, low RAM. |
| D9 | Brightness: fixed default 50%, with optional time-of-day dimming from sunrise/sunset in config | Power, heat and night-time glare. |
| D10 | Memory budget under ~100 MB RSS for the app | Only ~460 MB total RAM. Measured: about 34 MB. |

Repository layout (proposed)

```
matrix/
  DESIGN.md  PLAN.md  README.md
  setup/
    01-pi-setup.sh          # phase 1: debloat, power, update
    02-matrix-setup.sh      # phase 2: library, boot config
    03-install-service.sh   # phase 6: app + systemd
    lib/common.sh           # logging, dry-run, idempotent helpers
  tools/
    test-matrix.sh          # phase 2 hardware test
    preview.py              # render PNG/GIF of any state
  src/matrix_display/
    __main__.py  app.py  config.py  state.py
    display/{base,hardware,emulator}.py
    data/{weather,homeassistant}.py
    ui/{layout,fonts,icons}.py
    scene/{sky,mountains,clouds,particles,scene}.py
  config/config.example.toml  secrets.env.example
  systemd/matrix-display.service
  tests/
```

## 7. Pi setup script design (`01-pi-setup.sh`)

Properties: idempotent, **`--dry-run` by default for removals**, logs to a file, refuses to run on a non-Pi, takes a package list snapshot first (`dpkg --get-selections > backup`), and **keeps the SSH session alive**.

1. **Debloat.** `apt-get purge` from an explicit list (not wildcards), then `autoremove --purge`. Candidates: `bluez`, `bluez-firmware`, `pi-bluetooth`, `triggerhappy`, `modemmanager`, `cups*`, `avahi` is **kept**, desktop/X/Wayland packages if present, `pulseaudio`/`pipewire` (if present), docs and man pages via dpkg `path-exclude`, unused locales (`localepurge` approach or dpkg excludes), `rpicam-apps`/camera stack, `piwiz`/`rpi-connect` if present. A **protect list** (`openssh-server`, `network-manager`, `wpasupplicant`, `firmware-brcm80211`, `raspi-firmware`, `libnss-mdns`, `avahi-daemon`, `sudo`, `systemd*`, kernel) is checked before any purge. Final list is reviewed against `apt-mark showmanual` on the real host in Phase 1 before enabling removal.
2. **Power.**
   - Bluetooth off: `dtoverlay=disable-bt`, disable `hciuart.service`, `bluetooth.service`.
   - Audio off: `dtparam=audio=off` (also required for the matrix).
   - HDMI and display stack off for a headless board: `display_auto_detect=0`, drop `vc4-kms-v3d`, `camera_auto_detect=0`, `max_framebuffers=0`; optional `hdmi_blanking=2`.
   - Activity LED off (`dtparam=act_led_trigger=none`), power LED off where supported.
   - Disable unneeded services/timers: `apt-daily` timers become a controlled weekly unattended-upgrade or manual, `man-db`, `e2scrub`, `ModemManager`.
   - CPU governor `ondemand`/`schedutil`; no overclock. Wi-Fi power save left **on** unless it causes visible HA polling lag. Verify with `vcgencmd get_throttled` and `pmic`-less estimates (the Zero 2 W has no PMIC readout, so power is validated with a USB meter by the user).
   - Swap: already a zram device (about 460 MB) and `/` is already mounted `noatime`; nothing to change.
   - Journal: kept persistent (logs survive a crash reboot, which matters when debugging) but capped at `SystemMaxUse=30M`.
3. **Update and dependencies.** `apt-get update && full-upgrade` (keeping existing config files), then install `git python3-dev python3-venv cython3 python3-setuptools`. Unattended security upgrades are deliberately not installed (extra packages, surprise changes on an always-on display); `apt-daily` timers stay as they are.
4. Prints a summary and requires an explicit reboot at the end (never reboots by itself).

Risks: purging can break a remote box. Mitigations are the protect list, dry-run default, backup of the package selection and of `config.txt` and `cmdline.txt` (timestamped copies) before edits.

## 8. Matrix setup script design (`02-matrix-setup.sh`)

Automates [Adafruit: Install Manually](https://learn.adafruit.com/adafruit-rgb-matrix-bonnet-for-raspberry-pi/install-manually), adapted for Trixie (Python 3.13, PEP 668 means no system pip, so use a venv or build into a venv):

1. Install build dependencies.
2. Clone `hzeller/rpi-rgb-led-matrix` to `/opt/rpi-rgb-led-matrix` pinned to a known commit.
3. Build the library with `make`, then `pip install` the source into the project's venv, which must use the system Pillow (see "Phase 2 as built"; the older `make install-python` flow no longer applies).
4. Blacklist `snd_bcm2835`; set `dtparam=audio=off`.
5. Append `isolcpus=3` to `cmdline.txt` (single line, idempotent).
6. Write the hardware profile to `config.toml`: `rows=64`, `cols=64` (hzeller counts the panel's full height; the 1/32 scan is handled by the library and the E line; Phase 2 verifies), `chain=1`, `parallel=1`, `hardware_mapping=adafruit-hat-pwm` (fallback `adafruit-hat`), `slowdown_gpio=2`, `pwm_lsb_nanoseconds`, `row_address_type`/`multiplexing` left default but overridable.
7. Both solder mods (E jumper, GPIO 4-18 PWM bridge) are already done, so the script defaults to `adafruit-hat-pwm`; it cannot verify the solder work, so the test in section 9 does.
8. Reboot required, again explicit.

The panel's exact scan quirks are unknown until tested, so the test tool exposes flags for `--led-row-addr-type`, `--led-multiplexing` and `--led-rgb-sequence` and the config carries them.

## 9. Hardware test (`tools/test-matrix.sh`)

Follows [Adafruit: Testing Install](https://learn.adafruit.com/adafruit-rgb-matrix-bonnet-for-raspberry-pi/testing-install): run the library's `demo` binaries (`-D0` rotating block, `-D1` color gradient, `-D4` etc.) and a Python `Hello` script via the bindings, with the project's flags. Pass criteria are a checklist (all 64 rows lit and not duplicated, correct colors, no tearing, no flicker, no visible dim half), recorded by the user. Also prints `vcgencmd get_throttled` to catch undervoltage.

## 10. Service and operations

- `matrix-display.service`: `Type=notify` with `WatchdogSec=30`, `Restart=always`, `RestartSec=5`, `OOMScoreAdjust=-500`, `EnvironmentFile=/etc/matrix/secrets.env`. No `CPUAffinity`: the library pins its own refresh thread to the isolated core (see "Phase 6 as built").
- Logs to journald (persistent via an explicit `Storage=persistent` override, capped at 30 MB), INFO by default.
- Config reload on `SIGHUP`. A `--once` mode renders a single frame to PNG for debugging.
- Failure display: on repeated data failure show the staleness dot; if the render loop dies, systemd restarts it; the panel is blanked on clean shutdown.

## 11. Testing strategy

- Unit tests: condition category mapping, next-change search, HA template parsing, and low/high formatting.
- Golden-image tests: render fixed states with the emulator and compare to checked-in PNGs with a tolerance.
- Fixture playback: recorded Open-Meteo and HA responses.
- On-device: the Phase 2 hardware test, then a 24 h soak test checking CPU, RSS, temperature and throttling.

## 12. Risks and open questions

Status as of 2026-10-24, after Phases 0-6 and the first live runs. Resolved items (hostname resolution, gate entities, panel geometry and colour order, ZIP lookup, debloat scope, token handling) have been removed; see the as-built notes in section 13.

| Risk / question | What happened | Status |
|---|---|---|
| CPU cannot hold flicker-free output while animating | The reserved core and PWM bridge work: the refresh thread has core 3 to itself and the loop holds 15 fps (render about 11 ms live, 3-5 ms in the emulator). The default refresh was only about 108 Hz (`pwm_bits = 11`), and minor flicker was seen. | **Open:** `pwm_bits = 9` (several times faster refresh) is applied; waiting for confirmation. Fallbacks: `limit_refresh_hz`, `gpio_slowdown = 3`, `pwm_bits = 10`. |
| About 460 MB RAM | The app needs about 35 MB, but Claude Code plus helper sessions filled RAM and swap. The Pi then thrashed: SSH drops, Wi-Fi chip errors (`brcmfmac`), and hardware-watchdog resets (several times, one with a hung SD-card worker). | **Mitigated:** `gpu_mem=16` (462 MB), single-job builds, `OOMScoreAdjust=-500`, persistent journal. Operating rule: keep large programs off the Pi (docs/operations.md). Confirm in the soak. |
| Unexplained resets or freezes | Early logs were lost because the Pi OS journal is volatile by default. Now persistent (30 MB cap); the watchdog was already on. | **Open until the soak is clean** (`tools/soak-report.sh`). |
| Power supply adequacy (4 A) | No throttling or undervoltage in any run; brightness capped at 50% (day), halved at night, ceiling 70%. Not measured with large white areas (snow, lightning) at higher brightness. | **Partly open:** watch `throttled` during bright scenes. |
| Bindings tied to Debian's Pillow | The panel bindings read Pillow's private image struct and were built against apt `python3-pil` 11.1.0. If apt upgrades `python3-pil`, the compiled bindings may crash or draw garbage. | **Known:** after any `python3-pil` upgrade rerun `./setup/02-matrix-setup.sh --apply` (rebuild), then restart and look at the panel. |
| Service runs as root | It did, because the code lived in the private home folder. | **Fixed by deploying to `/opt/matrix`** (see section 13); pending the first real deployment, which also exercises the privilege drop for the first time. |
| SD card wear and an unbranded card | Persistent journal is capped; swap is zram; `/` is `noatime`. Card has shown no I/O errors. | Watch; keep a backup of `/etc/matrix` (docs/operations.md). |
| Upstream library changes | Pinned to commit `14464b3ad8`; bindings build system changed in Feb 2026. | Bump deliberately and retest (`tools/test-matrix.sh`). |
| Behaviour not yet seen live | Gate open (blink), stale-data dots, Home Assistant 401 or unreachable, internet outage. These are covered by tests and fixtures only. | **Open:** exercise during the soak (briefly unplug the network, or stop Home Assistant). |

## 13. As-built notes by phase

Sections 7, 8, 10 and the risks above describe the plan as it stood; the notes below record what was actually built and where reality differed.

### Phase 1 as built (`setup/01-pi-setup.sh`)

Based on an inventory of this host (Debian 13, 633 packages, mostly default Raspberry Pi OS Lite). Steps `purge boot system update verify`; dry run by default; `--apply` needs root and typed confirmation; `--aggressive` adds a second tier.

- **Purge list** (about 125 packages with dependencies, before `--aggressive`): Bluetooth (`bluez`, `bluez-firmware`); the **Pi 5 kernel** and all kernel headers (this board runs the `rpi-v8` kernel, which is kept; apt never auto-removes kernels, so they are named explicitly); firmware for USB Wi-Fi dongles (`atheros`, `mediatek`, `realtek`, `libertas`; **`firmware-brcm80211` is kept**, it drives this board's Wi-Fi); `rpi-eeprom` (Pi 4/5 bootloader, not applicable to the Zero 2 W); camera and V4L tools; `mkvtoolnix`, `p7zip-full`, `ntfs-3g`, `cifs-utils`, `udisks2`, `libmtp-runtime`; `gdb`, `apt-listchanges`, `pciutils`, `usb-modeswitch`, `wireless-tools`, `manpages-dev`.
- **`--aggressive`** (about 153 packages): also `cloud-init` (first-boot provisioning, already done), `rpi-connect-lite` (Raspberry Pi Connect) and `rpi-usb-gadget`. Left off by default because they may be how you reach the Pi.
- **Kept on purpose:** `avahi-daemon` (`hostname.local` SSH access), NetworkManager, `wpa_supplicant`, SSH, `raspi-config`, `raspi-utils` (it depends on `kms++-utils`), build tools, `htop`, `nano`, `strace`.
- **Safety net:** before purging, `apt-get -s` simulates the full removal including dependents; if anything on the protect list would go, the script aborts with nothing changed.
- **`config.txt`:** conflicting lines are commented out with a `#matrix-setup#` prefix and a marked block in `[all]` sets `dtoverlay=disable-bt`, `dtparam=audio=off`, `camera_auto_detect=0`, `display_auto_detect=0`, `gpu_mem=16`. The KMS display overlay is turned off (headless). Idempotent; tested on a copy of this host's real file.
- **System:** disables `e2scrub`, `man-db.timer`, `NetworkManager-wait-online` (faster boot; the app tolerates starting offline) and Bluetooth units; `led-off.service` turns off the ACT LED; journald capped.
- **Dropped from the earlier draft:** `libnss-mdns` (`homeassistant` resolves via DNS), the SD swap file change (swap is already zram).
- **Correction (found 2026-10-23):** the journal was *not* persistent. Raspberry Pi OS ships `40-rpi-volatile-storage.conf` (`Storage=volatile`), so every reboot erased the logs, which made a hang undiagnosable. The drop-in now sets `Storage=persistent` (capped at 30 MB). The hardware watchdog needs no change: `raspberrypi-sys-mods` already enables it (`RuntimeWatchdogSec=1m`), and it is what reset the board after a freeze during the library build (reset status `0x20` = watchdog reset, filesystem needed orphan cleanup).

### Phase 2 as built (`setup/02-matrix-setup.sh`, `tools/test-matrix.sh`, `tools/matrix_hello.py`)

Checked against the Adafruit guides and the upstream README on 2026-10-23.

- **What the Adafruit "manual install" page actually requires:** build the library and demos, then resolve the sound conflict by blacklisting `snd_bcm2835` (`/etc/modprobe.d/blacklist-rgb-matrix.conf`) and rebooting. It makes no `config.txt` or `cmdline.txt` edits. The core reservation (`isolcpus`) is an extra from the upstream README, kept because it reduces flicker on a loaded Pi; `--no-isolcpus` skips it.
- **Python bindings changed in Feb 2026:** upstream now builds them with scikit-build-core and CMake, installed with `pip install <source dir>` into a virtualenv (the old `make install-python` flow in Adafruit's page is outdated). The script does exactly that, into `<repo>/.venv`, as the repo owner rather than root.
- **Pinned source:** commit `14464b3ad8` of `hzeller/rpi-rgb-led-matrix`, fetched shallow (a few MB). A full clone failed with TLS errors on this network, so the fetch uses HTTP/1.1 with retries. Source lives in `<repo>/vendor/` (gitignored).
- **Boot edits:** sound blacklist plus initramfs refresh; `isolcpus=domain,managed_irq,3 nohz_full=3 rcu_nocbs=3 irqaffinity=0,1,2` appended to `cmdline.txt` only if those keys are absent. Idempotent and tested on copies.
- **Verified facts:** the E pad bridges to the "8" pad for Adafruit 64x64 panels; `adafruit-hat-pwm` is the mapping for the GPIO 4-18 mod; `rows=64 cols=64` for this panel; a panel that shows half the rows may need `--led-row-addr-type=1`. The Zero 2 W has no official slowdown value, so the test starts at 2.
- **Test flow:** `tools/test-matrix.sh` runs the upstream `demo` (-D 0, 4, 5) with this panel's flags, asking what you see, then `tools/matrix_hello.py` exercises our own `HardwareDisplay` backend. Settings are overridable by environment (`SLOWDOWN`, `MAPPING`, `EXTRA`). Results are appended to `docs/hardware-test-results.txt`.
- **Pillow must come from apt (found 2026-10-23, first apply attempt):** the bindings' C shim (`shims/pillow.c`) includes Pillow's private `Imaging.h` and reads its in-memory image struct, so it must be compiled against, and run with, the same Pillow. pip wheels ship no such header (the build failed with `Imaging.h: No such file`), and the layout can differ between versions (the dev venv had Pillow 13.0.0; Debian's package is 11.1.0 and ships `/usr/include/python3.13/Imaging.h`). The script therefore installs `python3-pil`, makes the venv `--system-site-packages`, and removes any pip-installed Pillow from it. The C library and demos had already built successfully (about 4.5 minutes).

### Phase 5 as built (Home Assistant poller)

- `data/homeassistant.py`: `HomeAssistantPoller(config.homeassistant, cache_path=None)` with `poll_once()`, `start()/stop()`, `latest()` (last good `HomeSnapshot`, thread-safe), `age_seconds()`. stdlib `urllib` only.
- One `POST /api/template` per poll. The template returns non-`locked` locks as `[id, state]`, ids of lights that are `on` and have no `entity_id` attribute (this drops light groups), and `states()` of the configured gates. **Glob exclusions (`exclude_locks`, `exclude_lights`) are applied in Python with `fnmatch.fnmatchcase`**, so no user text enters the template; gate ids are the only interpolated values and must match `^[a-z0-9_]+\.[a-z0-9_]+$` (else `ValueError`). Payload is a few hundred bytes.
- Lock states `unlocked/jammed/unlocking/locking` -> `locks_unlocked`; `unavailable/unknown` -> `locks_unavailable`. Gates are open on `on/open/opening`; `unavailable/unknown` (which includes a misspelt entity id) -> `gates_unavailable`.
- Failures: timeout 5 s; delay doubles from `poll_seconds` up to 60 s; the last good snapshot is kept. 401/403 logs one error (mentions `MATRIX_HA_TOKEN`) and retries every 300 s; unreachable / bad JSON / bad shape log a warning on failure 1 and every 10th. Exceptions are raised `from None` with no URL or header, so the token cannot appear in messages, tracebacks or repr.
- Cache: the caller passes `cache_path`; written atomically after each good poll and loaded at construction (`latest()` is then available immediately, `age_seconds()` shows how stale). A corrupt cache is ignored with a warning.
- Integration (not done here): `app.py` should construct the poller with a cache path, call `start()`, treat `age_seconds()` > ~5 min as stale in the UI, and `stop()` on exit.

### Phase 3 as built (`data/weather.py`, `data/geocode.py`, `tools/geocode.py`, `scene/`)

- **Weather:** `fetch_weather(location)` makes the section 5 request (coordinates, units from `[location]`; wind stays in the API default, km/h) and `parse_open_meteo` builds a `WeatherSnapshot` with datetimes in the response's `utc_offset_seconds` zone. `WeatherFetcher` runs a daemon thread, refreshes every `[weather].refresh_seconds`, backs off 15/30/60/120/240/300 s (cap 5 min) after failures, keeps the last good snapshot, and writes it atomically to a JSON cache (default `weather-cache.json`, pass `cache_path`) that is loaded at start so a boot without network still has data. `latest()` is lock-protected. Tests parse `tests/fixtures/weather_open_meteo_recorded.json`, a real response for 40.748, -73.994 (recorded 2026-10-24).
- **Geocoding (verified live 2026-10-24):** Open-Meteo `geocoding-api` accepts `name=10001&countryCode=US` and lists the ZIP in `postcodes`; Nominatim `postalcode=10001&country=US&format=jsonv2&addressdetails=1` returns one centroid (40.748, -73.994). Results are filtered by country again client side, junk input is rejected before any request, Nominatim calls are throttled to 1/s with a descriptive User-Agent. `tools/geocode.py ZIP [--country US] [--units ...] [--yes]` lists matches, asks to confirm, then edits only the `[location]` table of `config/config.toml` (keys replaced in place, comments and other sections untouched; the result is re-parsed before writing). `--lat/--lon/--label` is the manual override. `country` is not stored because `LocationConfig` does not have that key.
- **Scene:** `draw_scene(frame, rect, state, t)` (also `SceneRenderer`, `render_state`) draws the 64x28 region from procedural art only. Static layers (sky gradient, ridges with snow caps and pines, cloud sprites, fog masks) are cached per (sun elevation in 1/40 steps, condition); each frame copies the cached sky and pastes sun/moon, stars, clouds, terrain, then particles, so a frame costs about 1 ms on a desktop and well under 20 ms on the Pi (to be measured in Phase 6). Sun and moon follow `daily.sunrise/sunset`; if `current.is_day` clearly disagrees with the clock (sample data) it wins. Moon phase is computed from the date. `is_day`/cloud cover/wind (quantised to 4 drift speeds so positions rarely jump on a refresh) and the WMO code (rain and snow intensity) all feed the picture. The temperature is drawn top right with built-in 3x5 digits and a degree dot.
- **Cross-fade:** the image is a pure function of (state, t), but `SceneRenderer` remembers the last drawn state so a condition change blends old into new over 2 s. A backwards or >5 s jump in `t` resets without fading. `draw_scene` uses one module-level renderer, so call it from the render thread only.
- Previews: `python -m matrix_display.scene.previews` writes `docs/previews/scene-<sample>.png` (scale 8) and `.gif`.

### Phase 4 as built (`ui/`, `app.py`, `tools/preview.py`)

- **Font** (`ui/fonts.py`): own 5-row proportional bitmap font defined in code (3 px wide glyphs, 4-5 px for M/N/W, 1 px spacing); digits, A-Z, space and `/ : . - + ?`, plus `^`/`v` arrows (5x5). No BDF loader was needed. `draw_text(frame, x, y, text, color)` clips to the frame; `text_width(text)`. Callers pass upper-case text.
- **Icons** (`ui/icons.py`): 9x9 weather icons per `Condition`, 7x9 lock (closed/open) and bulb, 9x9 gate (closed/open), as palette-character rows.
- **Widgets** (`ui/widgets.py`, `WIDGETS` dict keyed by `layout.REGIONS` names): `date` (centred, `FRI 10/23`), `temps` (arrow+value centred in each half, so `-12` and `105` fit), `next_change` (icon of the new condition + `SNOW 6P`; `STEADY 24H` with the current icon if none), `home` (lock red when unlocked, `?` marker for unavailable locks, bulb, gate green/red blinking at 1 s/amber). Counts clamp at 99. The staleness dot is drawn by `home` at pixel (63,63): red (weather > 3 h) wins over amber (HA > 5 min); a grey dot marks missing weather/home. Missing data draws dim `--`.
- **Compositor**: `app.build_default_compositor(include_scene=True)` registers the widgets plus `scene.draw_scene` when importable. `app.brightness_for(now, daily, cfg_display)` returns `brightness`, halved between sunset and sunrise, clamped to 1..`max_brightness`; the render loop (Phase 6) must call it and `Display.set_brightness`.
- **Previews/tests**: `tools/preview.py --all` writes `docs/previews/ui-<name>.png` (samples plus extra edge-case states from `ui/variants.py`). `tests/test_ui.py` has goldens in `tests/golden/` (scene excluded; regenerate with `UPDATE_GOLDEN=1`), region-overflow and brightness tests.
- Not verified on the real panel; judged legible only from emulator PNGs.

### Phase 6 as built (live loop and service)

- **`runner.py`:** background `WeatherFetcher` and `HomeAssistantPoller` threads publish snapshots; the main loop builds an `AppState` each frame, renders, and shows it at `scene_fps` (15). Slow frames are dropped, not queued. A widget exception shows the last good frame (and raises after about 10 s of continuous failure so systemd restarts the process). Brightness is recomputed every 30 s (`brightness_for`, never above `max_brightness`). A performance line (render avg/max, overruns, RSS, free memory, temperature, throttle flags) is logged every 60 s.
- **Measured in the emulator on this Pi:** a full frame renders in about 3-5 ms on average and 15 ms at worst against a 66 ms budget at 15 fps, so the CPU is not the constraint. Panel refresh behaviour under the real loop is measured on first run (log line plus `--led-show-refresh`-style behaviour via the demo's 108 Hz baseline).
- **systemd:** `Type=notify` with `WatchdogSec=30` (the loop pings about once a second), `Restart=always`, `OOMScoreAdjust=-500`, secrets only via `EnvironmentFile=/etc/matrix/secrets.env`, state cache in `/var/lib/matrix-display`. **No `CPUAffinity`:** the library pins its refresh thread to the isolated core 3 itself, which needs core 3 allowed (the earlier draft said to restrict the service to cores 0-2; that would have been wrong).
- **Privilege drop:** the panel library drops root after GPIO setup. The first install ran from the private home folder, so the unprivileged user could not import code afterwards and the installer ran the service as root (`MATRIX_DROP_PRIVILEGES=0`). **Superseded by the GitHub deployment below**, which keeps the code in world-readable `/opt/matrix` so the drop stays on (`--no-drop` opts out).
- **Start-up order:** the display is created first (privileges drop on hardware), then the network threads start, so a failed network never delays the first frame; cached weather/HA data from the last run is shown immediately.

### Ambient animation for clear skies (added after the first live look)
The original scene animated clouds, rain, snow, fog and thunder, but a clear sky was nearly frozen (measured: 0% of scene pixels changed over 10 s by day, 1.2% at night). Now: stars twinkle at individual rates (about 2-6 s periods, dipping to nearly dark); a shooting star crosses the sky every 23 s at night (position varies per cycle, derived from `t`); the sun has eight rays that alternate twice a second; and one slow wisp of cloud always drifts across a clear sky. Still a pure function of (state, t), still about 3-5 ms per frame. Tests in `tests/test_scene_ambient.py`.

### Deployment from GitHub (replaces running from the home folder)
- **Layout:** `/opt/matrix` is a `git clone` of the repository (root-owned, world-readable) with `.venv/` (system-site-packages, editable install of the app, the compiled `rgbmatrix`) and `vendor/` inside, both gitignored. The service runs `/opt/matrix/.venv/bin/python`. Config and token stay in `/etc/matrix`; caches in `/var/lib/matrix-display`. The development checkout in the home folder is no longer used at run time.
- **Installer (`setup/03-install-service.sh`):** clones or fast-forwards `/opt/matrix` to `--ref` (default `main`) from `--repo` (default: the checkout's `origin`), builds or reuses the panel library, installs the app, writes the unit and (re)starts the service. Re-running it is the update procedure. It is wrapped in a function so a `git pull` cannot rewrite the script while it runs.
- **Private repository access:** SSH URL plus a per-repository, read-only **deploy key** stored in `/etc/matrix/deploy_key` (0600); it can read this one repository and nothing else. The installer generates it and prints the public key to add on GitHub.
- **Reusing the build:** compiling the panel library takes 10+ minutes and nearly exhausts RAM, so when started from a checkout that already has a finished build the installer copies `vendor/` and the compiled `rgbmatrix` package instead (`--rebuild` forces a build).
- **Privilege drop is now exercised for the first time** by the full app (state-dir writes, config reload via environment variables, systemd notifications). If it misbehaves, `--no-drop` restores the root-only behaviour. After the drop the process can no longer read `vcgencmd` (`/dev/vchiq` is group `video`), so the throttle flags are read by `tools/soak-report.sh` instead of the service log.
