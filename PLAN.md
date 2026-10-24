# Project Plan

Master-agent coordination plan for the Home Status Matrix. Design details live in [DESIGN.md](DESIGN.md); this file defines **phases**, **contracts between them**, and **agent briefs**.

## How the master agent works

- The master owns the plan, the shared interfaces (section "Contracts"), merging, and acceptance. Phase agents own implementation inside their listed paths only.
- Each phase is **individually deliverable**: it ends with something runnable and a documented acceptance check.
- Phases marked **hardware-gated** need the user at the Pi (soldering, power, observing the panel). The agent prepares scripts and a checklist; the user confirms the results.
- Code phases (3 to 5) are developed against the **emulator backend**, so they do not wait for hardware.
- Every agent: read DESIGN.md first, touch only owned paths, add tests, update README sections it owns, and report what was and was not verified.

## Dependency graph

```
 P0 Foundation (contracts, scaffold)
   |-- P1 Pi setup script --------+
   |-- P2 Matrix setup + HW test -+--> P6 Service & integration --> P7 Hardening & soak
   |-- P3 Weather data + scene ---+          ^
   |-- P4 Layout, date, display --+          |
   '-- P5 Home Assistant ---------+----------'
```

P1 and P2 are sequential on the device (P2 needs P1's boot config), but the scripts can be written in parallel. P3, P4, P5 are fully parallel after P0.

## Phase 0: Foundation (master agent, do first) - DONE

Deliverables
- Repo scaffold per DESIGN.md section 6, `README.md`, `pyproject.toml`/`requirements.txt`, lint/test config.
- **Contracts** as code (`state.py`, `display/base.py`, `config.py`) and a `config.example.toml`.
- Emulator backend and `tools/preview.py` (renders a state to PNG) so other phases can see their output.
- Sample fixtures: Open-Meteo JSON and HA template response.
- `config.example.toml` includes `[homeassistant].url` with the default `http://homeassistant:8123`.

Acceptance: `python -m matrix_display --emulate --once` writes a blank 64x64 PNG; `pytest` runs green.

Open inputs needed from the user: your ZIP code for the one-time location lookup; units; HA long-lived token (kept out of the repo), gate entity ids (or accept discovery in P5).

## Phase 1: Raspberry Pi setup script (`setup/01-pi-setup.sh`) - DONE (applied and verified after reboot)

Hardware-gated: no (runs on this host), but destructive, so review first.

Scope: DESIGN.md section 7. Debloat with explicit purge list and protect list, power savings, update and dependency install, SD-wear reductions, mDNS check.

Steps
1. Inventory the real host: `apt-mark showmanual`, enabled services, and boot config; propose the final purge list to the user.
2. Implement `setup/lib/common.sh` (dry-run, logging, backups, idempotent edit helpers for `config.txt` and `cmdline.txt`).
3. Implement the script; dry-run by default, `--apply` to act.
4. Verify idempotence by running twice; verify SSH, Wi-Fi, and mDNS still work.

Acceptance: after reboot, Bluetooth, audio and HDMI are off, `dpkg -l | grep -c ^ii` is meaningfully below 633, `apt full-upgrade` reports nothing pending, `http://homeassistant:8123` is reachable on port 8123, and rerunning the script changes nothing.

## Phase 2: Matrix setup and hardware test (`setup/02-matrix-setup.sh`, `tools/test-matrix.sh`) - DONE (applied and panel-tested 2026-10-24)

Hardware-gated: **yes** (wiring, power, observing results). The E-line jumper and PWM bridge are already done, so the default mapping is `adafruit-hat-pwm`; the checklist only asks the user to visually confirm the solder joints.

Scope: DESIGN.md sections 8 and 9. The agent first reads the three Adafruit guides and records any differences from the assumptions in DESIGN.md (updating it).

Steps
1. Write a wiring and mod checklist (`docs/hardware.md`): E jumper, GPIO4-18 PWM bridge, power, ribbon orientation.
2. Implement the install script for Trixie / Python 3.13 (venv, pinned library commit).
3. Implement the test script and a Python smoke test; expose geometry and timing flags.
4. User runs the checklist; agent records the flags that produced a clean image into `config.example.toml`.

Acceptance: demo `-D0`/`-D1` and the Python hello test display correctly across all 64 rows with correct colors and no duplicated halves; settings recorded.

## Phase 3: Weather data and animated scene

Owned paths: `src/matrix_display/data/weather.py`, `src/matrix_display/data/geocode.py`, `tools/geocode.py`, `src/matrix_display/scene/`, tests, fixtures.

Scope: DESIGN.md sections 4 and 5 (weather).

Deliverables
- ZIP-code geocoder (DESIGN.md section 5): Open-Meteo then Nominatim providers, confirmation prompt, coordinates written to `[location]`, manual lat/lon override; tested with recorded fixtures and offline-failure cases. Verify each provider's endpoint and response format first.
- Open-Meteo client with caching, backoff, and the WMO-to-category mapping.
- Pure functions: `daily_low_high(snapshot)`, `next_change(snapshot, now)`.
- Scene renderer `render_scene(state, t) -> Image` (64x28) with all layers and the seven condition effects, day/night, and the cross-fade.
- Preview GIFs for each condition produced by `tools/preview.py`.

Acceptance: unit tests for mapping and next-change edge cases (change tomorrow, no change, midnight); each condition renders and animates in the emulator; render cost under 20 ms per frame on the Pi (measured in P6).

## Phase 4: Layout, date and icons

Owned paths: `src/matrix_display/ui/`, `display/`, `app.py` render loop.

Scope: DESIGN.md sections 3 and 6.

Deliverables
- Bitmap fonts loader, date widget, temperature widget, next-change widget (with 9x9 condition icons), HA row widget (lock, bulb, gate icons; open, closed, and blinking states), staleness dot.
- Frame compositor that places the scene and widgets, plus brightness/dimming schedule.
- Hardware display backend wrapper around the rgbmatrix bindings (verified in P6).

Acceptance: golden-image tests for representative states (all-clear, rain with next-change snow, 0 and 12 counts, gate open); the emulator shows the full screen; no text overflows 64 px for any value in the supported ranges.

## Phase 5: Home Assistant integration

Owned paths: `src/matrix_display/data/homeassistant.py`, `tools/ha-discover.py`, tests.

Scope: DESIGN.md section 5 (HA).

Deliverables
- Poller using the `/api/template` call, with timeouts, auth-failure vs network-failure distinction, and last-good caching.
- Config for the HA base URL (`[homeassistant].url`, overridable by `MATRIX_HA_URL`; default `http://homeassistant:8123`), gate entity ids, and lock/light exclusions. Includes URL normalization and validation tests.
- Discovery tool `tools/ha-discover.py` (**built**, tested against a fake HA server) listing candidate gate entities, locks, and lights, with group lights flagged, plus a `--toml` config snippet.

Acceptance: tests against recorded fixtures cover unlocked/jammed/unknown locks, open/opening gates, and HA being unreachable; a live run prints the three values; the token is never logged.

## Phase 6: Service and integration (hardware-gated) - INSTALLED AND RUNNING (visual check pending)

Owned paths: `setup/03-install-service.sh`, `systemd/`, `app.py` wiring.

Deliverables: wire data threads, state store, and renderer; install to `/opt/matrix`, create `/etc/matrix/`, enable the systemd service, and add the first-boot cached-state behavior.

Acceptance: after reboot the panel shows the full display without intervention; pulling the network leaves the last data with the staleness dot; killing the process restarts it within 5 s; measured frame rate is stable and there is no visible flicker. Tune `slowdown_gpio`, `pwm_bits`, and scene fps here.

## Phase 7: Hardening and soak - IN PROGRESS (soak running)

Deliverables: 24-48 h soak with logged CPU, RSS, temperature and `get_throttled`; log and journal limits verified; brightness schedule tuned; README for operation and troubleshooting; backup of working config; list of follow-ups.

Acceptance: no crashes, no memory growth, no throttling flags, the user signs off on appearance.

## Contracts (defined in Phase 0)

```python
# state.py (immutable dataclasses)
WeatherSnapshot: fetched_at, current{temp, code, cloud, wind, is_day},
                 hourly[list of (time, code, temp, precip_prob)],
                 daily{low, high, sunrise, sunset}
HomeSnapshot:    fetched_at, locks_unlocked:int, lights_on:int, gates_open:list[str]
AppState:        weather: WeatherSnapshot|None, home: HomeSnapshot|None, now: datetime

# display/base.py
class Display: width, height; def show(self, image: PIL.Image) -> None; def set_brightness(self, pct: int) -> None
# scene:   render_scene(state: AppState, t: float) -> PIL.Image   (64x28)
# widgets: each ui widget exposes draw(canvas, state) with a fixed bounding box from layout.py
```

Changing a contract after Phase 0 is a master-agent decision; phase agents raise it rather than editing it.

## Suggested agent assignment

| Agent | Phases | Parallel with |
|---|---|---|
| master | 0, then integration and acceptance of all | n/a |
| ops-agent | 1, 2 (scripts and docs), later 6 install script | P3-P5 |
| scene-agent | 3 | P4, P5 |
| ui-agent | 4 | P3, P5 |
| ha-agent | 5 | P3, P4 |
| master + user | 2 hardware steps, 6, 7 | n/a |

## Decisions I need from you before Phase 0

1. Your ZIP code for the one-time location lookup. Units assumed °F.
2. Whether to remove packages only after reviewing the proposed list (assumed yes).
3. ~~Bonnet solder mods~~: done (E jumper and PWM bridge).
4. ~~Power supply~~: 4 A, so brightness is capped (see DESIGN.md).
5. ~~Gate names~~: use `tools/ha-discover.py` (built). Create the HA token and put it in `secrets.env` (see DESIGN.md section 5); it is gitignored.

## Phase 0 status (done)

Built and tested (39 tests pass, `pytest`): package scaffold, contracts (`state.py`, `layout.py`, `display/base.py`, `config.py`), emulator and (untested) hardware display backends, `Compositor` with pluggable region drawers, named sample states, `tools/preview.py`, `tools/ha-discover.py`, example config, README, fixtures. The Open-Meteo fixture is hand-built; Phase 3 should replace it with a recorded response. `HardwareDisplay` cannot be tested until Phase 2/6.

Findings that changed the design: the real HA has ~58 non-room "lights" (status LEDs) which are excluded by glob; `rows` for the panel is 64, not 32; an unavailable lock is shown with a marker rather than counted. The ZIP code is still needed for `[location]`.

## Phase 1 status

`setup/01-pi-setup.sh` and `setup/lib/common.sh` are written and dry-run on this host (see DESIGN.md, "Phase 1 as built"); the boot-file editing is tested for idempotence on a copy. **Not applied**: it needs `sudo`, which an agent session here cannot do (password required), so the user runs it. Remaining acceptance checks happen after the reboot: Bluetooth/audio/HDMI off, package count down, SSH and `http://homeassistant:8123` still reachable, rerun changes nothing.

Applied on 2026-10-23 with `--apply --aggressive`: 633 to 498 packages, no errors, `http://homeassistant:8123` still HTTP 200, Bluetooth gone, config.txt power block in place. Dry-run afterwards finds only one leftover orphan (`linux-base-6.18.50+rpt-rpi-2712`, now covered by the script). One package, `libpython3.13`, was purged and then reinstalled as a dependency of `python3-dev`; harmless. Still to check after reboot: SSH/Wi-Fi/HA up, Bluetooth/audio/HDMI off, ACT LED off, and a second run changes nothing.

Phase 1 verified after reboot (2026-10-23): kernel 6.18.50 rpi-v8 running; 498 packages; no failed units; Bluetooth gone; no sound card or `/dev/dri`; serial console moved to `ttyAMA0` (disable-bt took effect); ACT LED off; Wi-Fi up; `homeassistant` resolves to the Home Assistant host and answers HTTP 200; `throttled=0x0`; 45.6 C idle; total RAM 462 MB (was 415, from `gpu_mem=16`). Cosmetic leftovers for Phase 2 to tidy: `snd_bcm2835` is loaded with no card (Phase 2 blacklists it), and the legacy firmware camera modules (`bcm2835_v4l2`, `mmal_vchiq`, `vc_sm_cma`) load and log harmless "failed to open VCHI service" errors; an optional modprobe blacklist would silence them.

## Phase 2 status

Written and dry-run: `setup/02-matrix-setup.sh`, `tools/test-matrix.sh`, `tools/matrix_hello.py`, `docs/hardware.md`, and `cmdline_ensure` in `setup/lib/common.sh` (45 tests pass). Verified against the Adafruit guides and upstream README (see DESIGN.md, "Phase 2 as built"): pinned commit fetches shallowly, the C++ library and demos build in about 4.5 minutes on this Pi, and every option name used by `HardwareDisplay` exists in the bindings' source. A full pip build of the bindings was started as a final check. To do: user runs `sudo ./setup/02-matrix-setup.sh --apply`, reboots, then `sudo ./tools/test-matrix.sh`; the agent records flags that give a clean picture in `config/config.example.toml`.

## Phase 2 result (2026-10-24)

Applied (second attempt; the first failed on the missing Pillow header, now fixed by using apt's `python3-pil`) and rebooted. `snd_bcm2835` no longer loads, `isolcpus` is active, `throttled=0x0`. `tools/test-matrix.sh` passed all checks on the real panel with `--led-rows=64 --led-cols=64 --led-gpio-mapping=adafruit-hat-pwm --led-slowdown-gpio=2`: rotating square over all 64 rows (E jumper good), correct colours, no flicker, ~108 Hz refresh, and the Python smoke test through `HardwareDisplay` showed all expected elements. No extra flags were needed (`row_address_type`, `multiplexing`, `rgb_sequence` stay default). Phase 3 to 5 are unblocked; Phase 6 will measure CPU headroom while animating.

## Phase 5 status (2026-10-24)

Built `data/homeassistant.py` and `tests/test_homeassistant.py` (fixtures `tests/fixtures/ha_template_*.json`). Tests pass; live run against the real HA succeeded (see DESIGN.md, "Phase 5 as built"). Not yet wired into `app.py` (shared contract; the integrator calls `HomeAssistantPoller(cfg.homeassistant, cache_path).start()` and reads `latest()`).

## Phase 3 status

Done (49 phase 3 tests, whole suite 120 pass): `data/weather.py` (client, fetcher, cache, backoff), `data/geocode.py` + `tools/geocode.py` (Open-Meteo then Nominatim, country filter, config writer), `scene/` (all seven conditions, day/night, cross-fade) and `docs/previews/scene-*.png|gif`. Live-verified 2026-10-24: Open-Meteo forecast and both geocoders for ZIP 10001. Not yet done: registering `draw_scene` on `"scene"` and starting `WeatherFetcher` in `app.py`/`tools/preview.py` (other sessions own those); per-frame cost measured only on this Pi in a desktop-style run (about 1 ms warm), not under the panel's load (Phase 6). See DESIGN.md "Phase 3 as built".

## Phase 4 status (ui-agent)

Done: fonts, icons, date/temps/next-change/home widgets, staleness dot, `build_default_compositor`, `brightness_for`, `preview.py --all`; `tests/test_ui.py` passes (74 tests). Verified on emulator PNGs only; panel legibility and the brightness wiring in the render loop are for Phase 6. See DESIGN.md "Phase 4 as built".

## Phase 6 status

Built and tested without the panel: `runner.py` (loop, stats, watchdog, reload), new CLI modes (`--emulate --offline --sample --seconds`), `systemd/matrix-display.service` (passes `systemd-analyze verify`), `setup/03-install-service.sh` (dry-run default). Full suite: 206 tests pass. Not yet done (needs the user and the real panel): install the service, watch the first live run, confirm refresh rate and CPU while animating, then the 24-48 h soak (Phase 7).

Phase 6 live result (2026-10-24, 11 min after install): service active and enabled, running as root (home dir is private). 900 frames/min = exactly 15 fps; render avg 10.6 ms, max 15-29 ms, 1 overrun in the first minute (82 ms during start-up) and none since; RSS 34 MB; about 0.8 CPU-cores busy (mostly the panel refresh thread spinning on the isolated core); 43-46 C; `throttled=0x0`; memory available stable around 130-146 MB. Live data confirmed through the caches: weather fetched at start (45.5 F, clear), Home Assistant updating every 15 s (6 lights on, 0 unlocked, gates closed). Still to confirm: the user's visual check of the panel, then Phase 7 (24-48 h soak, hardening, README).

## Phase 7 status

Panel confirmed by the user (photo and in-person look): all regions correct, live data, night sky dark with mountains visible. Done: moon icon for clear nights (steady and next-change), `tools/soak-report.sh`, `docs/operations.md`, git repository initialised (no commit made) with `.gitignore` and the pre-commit guard verified (blocks a fake token and `secrets.env`; the real token appears in no tracked file, the journal or the caches). Remaining: a reboot test (service must come up by itself), the 24-48 h soak checked with `tools/soak-report.sh`, and the user's sign-off.

## Deployment from GitHub (done)

Request: stop running from the home folder; commit to the user's GitHub as `LEDMatrixGadget` and install from there. Done: installer reworked (`setup/03-install-service.sh` deploys a clone to `/opt/matrix`, deploy-key support for a private repo), docs updated, privacy scan of the files to be committed (no token or e-mail; the files originally contained the owner's ZIP/coordinates, entity names, home path and an e-mail address in a User-Agent; these were replaced by neutral examples (ZIP 10001, New York) because the repository is **public**). Waiting on: GitHub username and authentication from the Pi (no `gh`, no SSH key, no git identity yet), then the first commit and push, then the first deployment on the Pi.

Published 2026-10-24: https://github.com/pshanesmith/LEDMatrixGadget (public, branch `main`, one commit, authored with the GitHub noreply address; the public copy was checked for the e-mail and secrets). Next: the first deployment on the Pi with `sudo ./setup/03-install-service.sh --apply` (clones to `/opt/matrix`, reuses the existing panel build, restarts the service with the privilege drop on), then the 24-48 h soak.

First deployment done 2026-10-24 03:31: service active from `/opt/matrix` (clone of the public repository), running as `daemon` (privilege drop works), refresh thread real-time on core 3, weather and Home Assistant caches written by the unprivileged process, no warnings. Soak restarts from here; the earlier service ran about 1 h 2 min cleanly before the switch.
