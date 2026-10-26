# LEDMatrixGadget: Design

A 64x64 RGB LED panel driven by a Raspberry Pi Zero 2 W that shows the date, an animated weather scene, and Home Assistant status at a glance. This document describes the system as built and the reasons behind its main choices. Day-to-day operation is in [docs/operations.md](docs/operations.md); wiring and panel checks are in [docs/hardware.md](docs/hardware.md).

## 1. Goals and non-goals

Goals
1. Glanceable: date, current weather, today's low and high, the next change in conditions, three Home Assistant indicators (unlocked locks, lights on, back-yard gates) and, optionally, the electricity rate tier.
2. Always on, self-recovering, low maintenance, low power.
3. Develop and test almost everything **without the panel attached** (emulator backend).

Non-goals: touch or button input, voice, multiple pages, cloud accounts for weather, anything that needs more than a read-only Home Assistant token.

## 2. Hardware and host

| Item | Detail |
|---|---|
| Host | Raspberry Pi Zero 2 W (4x Cortex-A53 at 1 GHz, about 462 MB usable RAM with `gpu_mem=16`) |
| OS | Debian 13 "trixie", aarch64, Raspberry Pi OS Lite (no desktop) |
| Driver board | Adafruit RGB Matrix Bonnet (#3211) |
| Panel | 64x64 HUB75, 2 mm pitch (#5362), 1/32 scan, uses the **E address line** |
| Language | Rust (2021 edition for this crate; 1.85 or newer because the panel crate needs it) |
| Panel library | [rpi_led_panel](https://github.com/EmbersArc/rpi_led_panel) (`rpi-led-panel` crate), a pure Rust rewrite of [hzeller's driver](https://github.com/hzeller/rpi-rgb-led-matrix); the version is pinned by `Cargo.lock` |

Hardware requirements for this panel and Bonnet:

- **E address line:** bridge the Bonnet's centre "E" pad to the "8" pad above it. Without it the panel shows only half the rows, repeated.
- **PWM bridge (flicker):** bridge GPIO 4 to GPIO 18 on the Bonnet and use `hardware_mapping = "adafruit-hat-pwm"`. `adafruit-hat` remains selectable as a fallback.
- **Onboard sound off:** it uses the same PWM hardware. `dtparam=audio=off` and a `snd_bcm2835` blacklist.
- **Reserved CPU core:** `isolcpus=domain,managed_irq,3 nohz_full=3 rcu_nocbs=3 irqaffinity=0,1,2` gives the panel refresh thread core 3 to itself. The library pins and prioritises that thread itself, so the service must not restrict CPU affinity.
- **Panel geometry:** `rows=64`, `cols=64` (the library counts the panel's full height). Default row-address type, multiplexing and RGB order are correct for this panel.
- **Power:** a 64x64 panel can draw about 4 A at full white. The supply is rated 4 A, feeds the Bonnet's terminal block, and powers the Pi through the Bonnet (never also connect USB power). Brightness is capped (default 50%, halved after sunset, hard ceiling 70%) and the scene avoids large white areas.

## 3. Screen layout (64x64)

The whole frame is composed in a 64x64 RGB framebuffer (`gfx.rs`) and pushed to the panel with a vsync swap. Regions are fixed in `layout.rs`.

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
     | [lock] 0    [bulb] 6    [gate] [bolt]           11 rows |
y=63 +----------------------------------------------------------+
```

- **Fonts and icons:** an own 5-row proportional bitmap font defined in code (upper case, digits, `/ : . - + ?` and arrows) and palette-character icons, so there are no font files or antialiasing. The temperature in the scene uses built-in 3x5 digits.
- **Date:** weekday and month/day (`SAT 10/24`) overlaid at the scene's top left with the same white-on-dark-outline treatment and 1 px margin as the temperature, so it reads over day and night skies; recomputed every frame from the forecast location's local time.
- **Temperature row:** low and high from the day's forecast, centred in each half so `-12` and `105` fit.
- **Next change:** the first forecast hour within 24 h whose condition category differs from the current one, shown as the new condition's 9x9 icon and the hour (`6P`, `11A`). If nothing changes, `STEADY 24H` with the current condition's icon. A clear sky is drawn as a moon between sunset and sunrise.
- **Home Assistant row:** three cells about 21 px wide. Lock: count of locks that are unlocked, jammed or in motion (red when nonzero, dim at zero, an amber `?` if a lock is offline). Bulb: room lights on. Gate: green when closed, slowly blinking red when any configured gate is open, amber when its state is unknown. Counts clamp at 99. **Rate bolt:** when `power_tier_entity` is set, a lightning bolt is added and the lock and bulb cells widen to 19 px, while the gate (9 px) and the bolt (7 px), which have no digits, sit 3 px apart in the remaining space on the right; icons and digits keep their size. The bolt's colour is the tier: discount green, off-peak blue, on-peak orange, critical peak red; a dim bolt means the sensor is unavailable or reports something unknown. Without `power_tier_entity` the row is the original three 21 px cells and no bolt is drawn. **Maintenance dot:** when any Home Assistant `update.*` entity is `on` (an update is waiting), the second-to-last pixel of the bottom row (62, 63) is yellow, just left of the staleness dot's corner pixel, so both can show; `exclude_updates` (fnmatch globs) ignores entities. The tier comes from the [ha-duke-tou](https://github.com/pshanesmith/ha-duke-tou) integration's `sensor.*_tier`, whose states are `discount`, `off_peak`, `on_peak` and `critical_peak`.
- **Staleness dot** (bottom-right pixel): amber when Home Assistant data is older than 5 minutes, red when weather is older than 3 hours (red wins), grey when a source has never produced data. Missing data draws a dim `--`.
- **Brightness:** `brightness_for()` returns the configured level by day, half of it between sunset and sunrise, never above `max_brightness`; the loop applies it every 30 s.

## 4. Weather scene

Procedural pixel art with no image assets. Layers, back to front:

1. **Sky:** vertical gradient from sun elevation (sunrise and sunset come from the forecast) and cloud cover; dark navy with stars at night.
2. **Sun or moon:** the sun moves along an arc between sunrise and sunset; the moon follows the date's phase. Both fade under heavy cloud.
3. **Clouds:** drifting sprites scaled by cloud cover and wind; overcast and precipitation use wrap-around cloud decks.
4. **Mountains and trees:** four parallax Blue Ridge-style ridges from seeded shapes, so the skyline never changes: wide, low, rounded hills that fade from vivid blue in the distance to a single smooth foliage colour nearby. That colour follows the day of the year (green in summer, fresh green in spring, russet in October, dull brown in winter) and is a plain blue silhouette at night, stepped so each ridge stays distinct from the sky. In front are three large deciduous trees built from leaf clumps with mixed colours, ragged edges and gaps showing branches, and three taller pines. The trees turn with the season: bright reds, oranges and yellows in the fall, fading to brown as leaves drop until winter shows bare branches; fresh green and blossom in spring. Snow only dusts the crests while it is snowing.
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
- **Cost:** a scene frame renders in well under a millisecond on a laptop (release build; the Python version needed 3-5 ms there); the Pi Zero 2 W has not been measured yet. The budget is 66 ms at 15 fps.
- **Same picture as the Python original:** the shapes use a Mersenne Twister that reproduces Python's `random.Random` (`pyrandom.rs`), rounding follows Python's round-half-to-even, and the drawing primitives follow Pillow's (ellipse outline tracing, mask blending). The tests compare more than 50 scene frames and the widget screens, made by the Python version, pixel for pixel (`tests/golden/`).
- Wind from the forecast biases rain, snow and cloud drift.

## 5. Data sources

### Weather: Open-Meteo
- No key, free for non-commercial use, one request returns everything:
  `GET https://api.open-meteo.com/v1/forecast?latitude=..&longitude=..&current=temperature_2m,weather_code,cloud_cover,wind_speed_10m,is_day&hourly=weather_code,temperature_2m,precipitation_probability&daily=temperature_2m_max,temperature_2m_min,sunrise,sunset&temperature_unit=fahrenheit&timezone=auto&forecast_days=2`
- `WeatherFetcher` refreshes on a background thread every `[weather].refresh_seconds` (900), backs off 15/30/60/120/240/300 s after failures, keeps the last good snapshot, and writes it atomically to a JSON cache so a boot without network still has data. Datetimes carry the location's UTC offset.

### Location: ZIP lookup, once
The user enters a ZIP or postal code; `matrix-geocode ZIP [--country US] [--units ...] [--yes]` resolves it **once, at setup time** and writes `[location]` (postal code, latitude and longitude rounded to 3 decimals, a label, units) into `config.toml`. The running service never geocodes. A ZIP centroid is accurate enough for a forecast and no street address is sent anywhere.
- **Providers (free, no key):** Open-Meteo geocoding first, then OpenStreetMap Nominatim (structured `postalcode` + `country`; at most 1 request per second, descriptive User-Agent; no personal contact is embedded, `MATRIX_CONTACT` optionally appends one).
- **Country filter:** a bare postal code is ambiguous across countries (for example `10001` also matches Spain and France), so results are filtered by `countryCode` and again client side, and the user confirms the match.
- **Manual override:** `--lat/--lon/--label`, or edit the file. Input is validated before any request.
- **Editing:** only the `[location]` table is rewritten (comments and other sections untouched) and the result is re-parsed before saving. Timezone is not configured: Open-Meteo's `timezone=auto` derives it.
- If both providers fail the tool says so; the service refuses to run weather without a location rather than guessing.

### Home Assistant
- **Token:** a read-only long-lived access token (profile > Security > Long-lived access tokens), supplied only as `MATRIX_HA_TOKEN` in the environment or an env file: `secrets.env` during development, `/etc/matrix/secrets.env` (mode 0600, root) on the Pi, loaded by systemd. It is never in code or git (`.gitignore` excludes `secrets.env`, `*.env` other than examples, `*.token` and the local `config.toml`; `.githooks/pre-commit` blocks a commit that contains a secrets file or a JWT-shaped string). Tools never accept it as an argument, and nothing prints it: errors carry no URL or headers, and the config's `Debug` output redacts it. If it leaks, delete it in Home Assistant and create a new one.
- **Base URL:** configurable, never hard-coded. Resolution order: `MATRIX_HA_URL`, then `[homeassistant].url`, then `http://homeassistant:8123`. The bare hostname resolves through local DNS (mDNS `homeassistant.local` did not). Scheme defaults to http, a trailing slash is stripped, and invalid values fail at startup.
- **Poll:** `HomeAssistantPoller` makes one `POST /api/template` every `poll_seconds` (15). The template returns locks that are not `locked` as `[id, state]`, ids of lights that are `on` and have no `entity_id` attribute (this drops light groups), ids of `update` entities that are `on`, the `states()` of the configured gates and, if set, of `power_tier_entity`. Glob exclusions (`exclude_locks`, `exclude_lights`, `fnmatch` semantics on entity ids) are applied in Rust, so no user text enters the template; gate ids and the tier entity id are the only interpolated values and must match `^[a-z0-9_]+\.[a-z0-9_]+$`. The payload is a few hundred bytes.
- **Counting rules:** lock states `unlocked`, `jammed`, `unlocking`, `locking` count as unlocked; `unavailable` and `unknown` count separately (`locks_unavailable`) because an offline lock is not known to be locked. Gates are open on `on`, `open`, `opening`; `unavailable` or `unknown` (including a misspelt id) means `gates_unavailable`. Unavailable lights are not counted as on.
- **Lights:** Home Assistant reports many `light` entities that are not room lights (for example switch status-LED bars and network-gear LEDs, plus groups). The default `exclude_lights` globs are `light.*_rgb_indicator`, `light.*_ap_led`, `light.*_led`; on the reference system they cut about 114 entities to about 56 real lights.
- **Discovery:** `matrix-ha-discover [--toml] [--all-lights] [--all-doors]` lists locks, room lights (noise hidden) and gate candidates (door-class `binary_sensor` and `cover` entities, preferring ones named "gate") and can print a config snippet.
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
                  |  scene + widgets, 15 fps   |      - hardware (rpi-led-panel)
                  +----------------------------+      - emulator (PNG/GIF)
```

| # | Decision | Rationale |
|---|---|---|
| D1 | Rust with the `rpi-led-panel` crate (a pure Rust port of hzeller's driver) and an own small framebuffer | One static binary, no interpreter or image library on the Pi (the Python version needed Pillow, whose private layout the old bindings read), a fraction of the CPU and memory. The framebuffer is about 400 lines because a 64x64 picture needs few operations. |
| D2 | `Display` abstraction with hardware and emulator backends | Everything except the final push can be developed and tested without the panel. |
| D3 | Fetchers run in threads and publish immutable snapshots; the render loop never waits on the network | A slow Wi-Fi call must not stutter the animation. |
| D4 | Last good data is cached to disk | A boot with no network still shows data. |
| D5 | One systemd service, `Type=notify`, watchdog, `Restart=always` | Self-healing: a hung loop or crash is restarted. |
| D6 | Non-secret config in `config.toml`, secrets only in the environment | Keeps the token out of the repository. |
| D7 | The service starts as root for GPIO timing; the library drops to `daemon` after setup | Least privilege for the network-facing code. Needs code readable by `daemon`, hence `/opt/matrix`. |
| D8 | Few dependencies: `chrono`, `serde_json`, `toml`, `ureq` (rustls), `clap`, `log`/`env_logger`, `signal-hook`, `anyhow`; `image` (PNG/GIF) is used by the emulator and previews only | Small update surface; everything is pinned by `Cargo.lock`. |
| D9 | Brightness capped and dimmed after sunset | Power budget, heat, night glare. |
| D10 | App memory budget under about 100 MB | The Python version measured about 35 MB of about 460 MB total; the Rust build is expected to be smaller but has not been measured on the Pi yet. |

**Runner (`runner.rs`).** Builds an `AppState` (clock in the forecast location's timezone, latest weather, latest home snapshot) every frame and draws it through the `Compositor`, which calls one drawer per layout region. Slow frames are dropped, never queued. A widget panic is caught: the last good frame stays up and the failure is logged; after about 10 s of continuous failure the loop returns an error so systemd restarts the process. The loop notifies systemd (`READY=1`, `WATCHDOG=1` about once a second, `STOPPING=1`), logs a performance line every minute (render average and maximum, overruns, RSS, free memory, temperature, throttle flags), and re-reads the config on `SIGHUP`. The display is created first (privileges drop on hardware) and the network threads start afterwards, so a down network never delays the first frame.

**Configuration** (`config.toml`; see `config/config.example.toml`): `[location]`, `[display]` (`rows`, `cols`, `chain_length`, `parallel`, `hardware_mapping`, `gpio_slowdown`, `pwm_bits`, `pwm_lsb_nanoseconds`, `limit_refresh_hz`, `row_address_type`, `multiplexing`, `rgb_sequence`, `brightness`, `max_brightness`, `scene_fps`, `drop_privileges`; the Python version's `safe_image_copy` is still accepted and ignored so existing files keep loading), `[weather]`, `[homeassistant]` (`url`, `poll_seconds`, `gates`, `exclude_locks`, `exclude_lights`, `exclude_updates`, `power_tier_entity`). Unknown keys are rejected. `MATRIX_HA_URL`, `MATRIX_HA_TOKEN` and `MATRIX_DROP_PRIVILEGES` come from the environment. `matrix-set-config` changes one value safely (idempotent, repairs duplicate sections, validates before saving).

**Repository layout**

```
README.md  DESIGN.md  Cargo.toml  Cargo.lock
setup/        01-pi-setup.sh  02-matrix-setup.sh  03-install-service.sh  lib/common.sh
tools/        test-matrix.sh  cross-build.sh  health-check.sh  soak-report.sh
src/
  lib.rs  main.rs (matrix-display)  runner.rs  compositor.rs  config.rs  state.rs  layout.rs
  forecast.rs  samples.rs  gfx.rs  pyrandom.rs  util.rs  setconfig.rs  hadiscover.rs
  display/{mod,emulator,hardware,pattern}.rs   data/{weather,geocode,homeassistant}.rs
  scene/{mod,sky,mountains,clouds,particles,palette,digits,cache}.rs
  ui/{fonts,icons,widgets,variants}.rs
  bin/          preview.rs  geocode.rs  set_config.rs  ha_discover.rs  hello.rs
config/       config.example.toml  secrets.env.example
systemd/      matrix-display.service  matrix-health.service  matrix-health.timer
docs/         operations.md  hardware.md  previews/
tests/        scene.rs  ui.rs  cli.rs  setup_scripts.rs  common/  fixtures/  golden/
.githooks/    pre-commit
```

Programs built from `src/bin/`: `matrix-preview` (render samples to PNG or GIF), `matrix-geocode`, `matrix-set-config`, `matrix-ha-discover`, and `matrix-hello` (panel smoke test; only with `--features hardware`).

## 7. Raspberry Pi setup (`setup/01-pi-setup.sh`)

Idempotent; **dry run by default** (`--apply` needs root and a typed confirmation); refuses to run on a non-Pi; snapshots the package and service state and timestamped copies of the boot files before changing anything; never reboots. Steps (`--only` / `--skip` select them): `purge boot system update autoupdate verify`. `--aggressive` adds a second tier of removals. Shared helpers (diff-then-write, marked blocks, backups, dry-run wrapper) are in `setup/lib/common.sh`.

- **Purge:** explicit package lists, never wildcards. Default tier (about 125 packages including dependents): Bluetooth (`bluez`, `bluez-firmware`); the Pi 5 kernel and all kernel headers (this board runs the `rpi-v8` kernel, which is kept; apt never auto-removes kernels, so they are named); firmware for USB Wi-Fi dongles (`firmware-brcm80211`, which drives this board's Wi-Fi, is kept); `rpi-eeprom` (Pi 4/5 bootloader); camera and V4L tools; unused desktop-style utilities; `gdb`, `apt-listchanges` and similar. `--aggressive`: also `cloud-init`, `rpi-connect-lite` and `rpi-usb-gadget`, left off by default because they may be how you reach the Pi.
- **Safety net:** `apt-get -s` simulates the removal including dependents first; if anything on the protect list (SSH, NetworkManager, wpa_supplicant, Wi-Fi firmware, the running kernel, Python, build tools, `git`, `sudo`, systemd, `avahi-daemon`, `raspi-config`, `raspi-utils` and similar) would go, the script aborts with nothing changed.
- **Kept on purpose:** `avahi-daemon` (`hostname.local` SSH), NetworkManager, SSH, `raspi-config`, build tools, `htop`, `nano`, `strace`.
- **`config.txt`:** conflicting lines are commented out with a `#matrix-setup#` prefix and a marked block in `[all]` sets `dtoverlay=disable-bt`, `dtparam=audio=off`, `camera_auto_detect=0`, `display_auto_detect=0`, `gpu_mem=16` (the KMS display overlay is dropped; the board is headless).
- **System:** disables `e2scrub`, `man-db.timer`, `NetworkManager-wait-online` (faster boot; the app tolerates starting offline) and Bluetooth units; `led-off.service` turns the ACT LED off; the journal is made **persistent** and capped at 30 MB. Raspberry Pi OS ships `Storage=volatile`, which erased logs on every reboot and made freezes undiagnosable; the drop-in overrides it. The hardware watchdog needs no change: `raspberrypi-sys-mods` already enables it (60 s).
- **Update:** `apt-get update && full-upgrade` (keeping existing config files), then `git` (to fetch the repository; the build tools are installed by `02-matrix-setup.sh`, and not needed at all when installing a cross-built binary).
- **Automatic updates (`autoupdate` step, `--no-auto-updates` skips it):** installs `unattended-upgrades` (11 small packages) and configures it through apt drop-ins. Allowed sources: Debian stable, its security and stable-updates suites, and the Raspberry Pi repository (kernel, firmware, raspi tools); never a new Debian release. The daily apt timer is moved to 03:00 (it was 06:00 plus up to an hour, and the periodic apt settings were unset, so nothing ran at all). A kernel or firmware update needs a reboot, which happens at **04:00 and only if nobody is logged in** (an open SSH session postpones it a night); the display is dark for about a minute. Config files are kept (`--force-confold`) and unused kernels and dependencies are removed. Nothing is held back. Application code is not updated automatically: it comes from GitHub when `setup/03-install-service.sh` is run.
- Swap is already a zram device and `/` is already `noatime`; nothing to change. No `libnss-mdns`: `homeassistant` resolves through DNS.

## 8. Building the display and boot settings (`setup/02-matrix-setup.sh`)

Automates the system side of Adafruit's "Install Manually" guide, adapted to Debian 13. Idempotent, dry run by default. Steps: `deps build boot verify`.

- **Panel library:** nothing is cloned or installed system-wide. The `rpi-led-panel` crate is a pure Rust port of hzeller's driver, so there is no C++ to build, no bindings and no `make`; it maps `/dev/mem` and runs its refresh loop on its own thread (the reserved core). `Cargo.lock` pins the version. `--features hardware` turns it on; the crate is Linux-only, so the default build (emulator only) is what builds and tests on a Mac.
- **Dependencies:** `gcc libc6-dev cargo rustc` from Debian (Rust 1.85 on Debian 13, which is the minimum the crate needs). `gcc` is the linker and compiles the TLS library's C parts; there is no C++ and no `make`. A Pi that only receives a cross-built binary needs none of them.
- **Build:** `cargo build --profile pi --features hardware --locked --bins -j1` at low priority. The `pi` profile is `release` without LTO and with 16 codegen units, because the Pi's 460 MB cannot link the optimised profile; the script warns when RAM plus swap is under 900 MB. The first compile takes a long time there. **Cross-compiling is much faster:** `tools/cross-build.sh` builds the aarch64 binaries on another machine with `zig` and `cargo-zigbuild`, and `setup/03-install-service.sh --binary` installs the result. Verified here: the whole program, including the panel backend and its tests, compiles and links for `aarch64-unknown-linux-musl` (a static binary). The glibc target is the default for the Pi (musl's resolver cannot use mDNS `.local` names) and needs `rustup target add aarch64-unknown-linux-gnu`.
- **Settings mapping:** `[display]` keys map onto the driver's: `gpio_slowdown` to `slowdown`, `rgb_sequence` to `led_sequence`, `hardware_mapping` (`adafruit-hat-pwm` becomes `AdafruitHatPwm`), `limit_refresh_hz` to `refresh_rate` (0, meaning no cap, uses the driver's default of 120 Hz), `drop_privileges` to `drop_privs`. `row_address_type` accepts 0-4 (direct, shift register, ABCD line, ABC shift register, SM5266) and `multiplexing` 0-4 (none, stripe, checkered, spiral, z-stripe); anything else is rejected at start-up.
- **Brightness:** the driver's canvas has a runtime brightness setter, which `HardwareDisplay` calls with the current percentage before drawing each frame; a change redraws the same frame. The driver applies it before its gamma table.
- **Boot edits:** `/etc/modprobe.d/blacklist-rgb-matrix.conf` (`snd_bcm2835`) plus an initramfs refresh; the `isolcpus` set from section 2 appended to `cmdline.txt` only for keys not already present (`--no-isolcpus` skips it). Tested on copies of the real files.
- `config.txt` audio settings come from the Pi setup script; the script checks for them.

## 9. Hardware test and flicker tuning

`tools/test-matrix.sh` (run with `sudo`) follows Adafruit's "Testing Install": it runs `matrix-hello` with three patterns (`square` rotating square, `pulse` pulsing colour, `gray` grayscale steps) using this panel's settings, asks what you see, then shows the `frame` pattern (red top half, blue bottom half, border, RGB bars, moving square). Everything goes through the project's own `HardwareDisplay`, so the test exercises the same code path as the service. Settings can be overridden by environment (`SLOWDOWN`, `MAPPING`, `EXTRA`, for example `EXTRA="--row-address-type=1"`); results append to `docs/hardware-test-results.txt`. `docs/hardware.md` has the symptom table.

The panel is refreshed by software timing, so flicker is a trade between colour depth and refresh rate. With `pwm_bits = 11` the measured refresh was about 108 Hz; each bit removed roughly doubles it. Tuning order: lower `pwm_bits`, set `limit_refresh_hz` to hold a steady rate, raise `gpio_slowdown` for shimmering lines.

## 10. Service and deployment (`setup/03-install-service.sh`)

The service runs a **copy of the repository deployed from GitHub**, not a working checkout.

- **Layout:** `/opt/matrix` is a `git clone` (root-owned, world-readable) with `target/` (the cargo build) and `bin/` (the installed `matrix-display` and helper programs the service runs) inside, both gitignored. Config and token are in `/etc/matrix` (`config.toml`, `secrets.env`, and `deploy_key` for a private repository). Caches are in `/var/lib/matrix-display` (owned by `daemon`).
- **Installer:** dry run by default. It clones or fast-forwards `/opt/matrix` to `--ref` (default `main`) from `--repo` (default: the checkout's `origin`), builds the program (or takes `--binary`), installs it into `bin/`, writes the unit and (re)starts the service. **Re-running it is the update procedure.** It is wrapped in a function so a `git pull` cannot rewrite the script while it runs. It never overwrites an existing `/etc/matrix` file. `--no-drop` keeps the service as root; `--uninstall` removes the unit.
- **Prebuilt binary:** compiling on the Pi takes 15+ minutes and nearly exhausts RAM, so `--binary PATH` installs a binary made by `tools/cross-build.sh` instead (helper programs next to it are installed too). Later updates reuse `target/`, so only changed crates rebuild; `--rebuild` starts from scratch.
- **Private repositories:** use an SSH URL; the installer creates a read-only per-repository deploy key and prints the public half to add on GitHub. A public repository needs nothing.
- **Unit (`systemd/matrix-display.service`):** `Type=notify` with `WatchdogSec=30`, `Restart=always`, `RestartSec=5`, `OOMScoreAdjust=-500`, `EnvironmentFile=/etc/matrix/secrets.env`, `NoNewPrivileges`, `PrivateTmp`, `ProtectHome=yes`, `ProtectSystem=full`, writable only in the state directory. No `CPUAffinity`.
- **Privilege drop:** the process starts as root and the panel library drops to `daemon` after GPIO setup, so the network-facing code is unprivileged; the files it still needs (the config for reloads, the cache directory) must stay readable to `daemon` afterwards, which is why the deployment is in `/opt` and not a private home folder; a `secrets.env` it can no longer read is skipped, because systemd already put the token in the environment. The installer checks the path and falls back to root if needed. After the drop the process can no longer read `vcgencmd` (`/dev/vchiq` is group `video`), so throttle flags are read by `tools/soak-report.sh` instead of the service log.
- **Logs:** journald (persistent, 30 MB cap), INFO. `SIGHUP` reloads configuration. On clean shutdown the panel is blanked.
- **Health check:** `tools/health-check.sh`, run by `matrix-health.timer` ten minutes after every boot (so after an automatic update and reboot) and daily at 08:00, verifies that the service is running and producing about 900 frames a minute, that the kernel logged no hung tasks, memory exhaustion, undervoltage, SD-card or Wi-Fi-chip errors since boot, that `isolcpus` and the sound blacklist survived the update, and that the installed binary is present and runs. It logs under the tag `matrix-health` (errors at error priority) and fails the unit so `systemctl --failed` shows it.
- **Health review:** `tools/soak-report.sh [since]` summarises boots, service restarts, warnings, kernel problems (hung tasks, SD card, Wi-Fi chip, memory, undervoltage), the worst performance values, throttle flags and free memory.

## 11. Testing

About 175 tests run in a few seconds with `cargo test`, none needing the panel or network:
- **Units** (in `src/`): the Python-compatible random generator (known sequences from CPython), drawing primitives (Pillow-exact ellipses and mask blending), condition mapping, next-change search, config parsing and validation, URL normalisation, the config setter, the geocoder (recorded Open-Meteo and Nominatim responses, offline failures), the weather parser and fetcher (a recorded forecast), the Home Assistant poller (a fake local HTTP server: locks, gates, 401, refused connection, bad JSON, backoff, token never in logs or `Debug` output), `fnmatch` globs, the runner (pacing, dropped frames, panic resilience, reload, systemd notifications over a real socket) and the hardware test patterns.
- **Rendering** (`tests/scene.rs`, `tests/ui.rs`): over 50 scene frames (every condition, four seasons, twilight, moon phases, cross-fades) and 16 widget screens are compared pixel for pixel with images produced by the original Python version (`tests/golden/`; `UPDATE_GOLDEN=1` rewrites the widget goldens, and `scene-cases.json` lists the scene cases); plus behaviour tests: text-overflow over value ranges, brightness, the rate bolt, per-frame cost, ambient motion.
- **Programs and scripts** (`tests/cli.rs`, `tests/setup_scripts.rs`): the binaries run as a user would, and the setup scripts' file-editing helpers and apt/systemd snippets are checked on copies (the shell tests run on Linux only).
- **Panel code without a panel:** the `rpi-led-panel` crate is Linux-only, so the hardware backend, its driver-settings tests and `matrix-hello` are compiled (not run) with `cargo zigbuild --lib --tests --features hardware --target aarch64-unknown-linux-musl`; `tools/cross-build.sh` links the programs.
- **On the device:** `tools/test-matrix.sh`, then a 24-48 hour soak reviewed with `tools/soak-report.sh`.

## 12. Risks and open questions

| Risk / question | Status |
|---|---|
| **Flicker.** The default refresh was only about 108 Hz and minor flicker was seen. | **Open.** `pwm_bits = 9` (several times faster refresh) is applied here; confirmation pending. Fallbacks: `limit_refresh_hz`, `gpio_slowdown = 3`, `pwm_bits = 10`. |
| **Memory.** About 460 MB total; the app needs about 35 MB, but large programs (Claude Code and helper sessions during development) filled RAM and swap, and the Pi thrashed: SSH drops, Wi-Fi chip errors, and hardware-watchdog resets. | **Mitigated:** `gpu_mem=16`, single-job builds, `OOMScoreAdjust=-500`, persistent journal. Keep large programs off the Pi. Confirm in the soak. |
| **Unexplained resets or freezes.** Early logs were lost to the volatile journal. | **Open until the soak is clean** (`tools/soak-report.sh`). |
| **Power (4 A supply).** No throttling or undervoltage seen; not measured with large white areas at higher brightness. | **Partly open:** watch `throttled` during snow and lightning. |
| **The Rust port has not run on the panel yet.** It was developed and tested on a Mac; the hardware backend compiles and links (cross-built) but was never executed against the real driver. | **Open:** run `tools/test-matrix.sh` first, then the service; watch the `render avg` log line and compare memory (`rss`) with the Python version's 35 MB. |
| **Building on the Pi.** Compiling needs about 1 GB with swap and 15+ minutes. | **Mitigated:** `tools/cross-build.sh` plus `03-install-service.sh --binary`; the `pi` profile and `-j1` keep an on-device build possible. |
| **Automatic upgrades and reboots.** Kernel and firmware updates from the Raspberry Pi repository arrive unattended and reboot the Pi at 04:00. A bad update could change panel timing or leave the display down. | **Open, untested:** the first automatic run has not happened yet. The health check runs after the reboot and logs a failure; `--no-auto-updates` or disabling `apt-daily-upgrade.timer` backs out. Review `journalctl -t matrix-health` and `/var/log/unattended-upgrades/` after the first runs. |
| **Panel library maturity.** `rpi-led-panel` 0.9 is a young rewrite (its README calls it "mostly untested"); timing, flicker and the privilege drop may differ from hzeller's C++ driver. Version pinned by `Cargo.lock`. | **Open:** judge flicker on the panel (`tools/test-matrix.sh`); the refresh rate is capped by `limit_refresh_hz` (120 Hz when 0). If it misbehaves, the C++ `rpi-led-matrix` crate was the alternative. |
| **SD card wear, unbranded card.** Persistent journal is capped, swap is zram, `/` is `noatime`; no I/O errors seen. | Watch; back up `/etc/matrix`. |
| **Behaviour not yet seen on the panel:** the open-gate blink, stale-data dots, Home Assistant 401 or unreachable, an internet outage. | **Open:** covered by tests and fixtures only; exercise during the soak. |
