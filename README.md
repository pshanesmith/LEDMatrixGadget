# Home Status Matrix

A 64x64 RGB LED matrix (Raspberry Pi Zero 2 W + Adafruit RGB Matrix Bonnet) showing the date,
an animated weather scene, and Home Assistant status. Written in Rust; the panel is driven by the
[`rpi-led-panel`](https://github.com/EmbersArc/rpi_led_panel) crate, a pure Rust port of hzeller's driver. See
[DESIGN.md](DESIGN.md) for the design and [docs/operations.md](docs/operations.md) for day-to-day operation.

## Development setup (no panel needed)

Install Rust (1.85 or newer), for example `brew install rustup && rustup default stable`.

```bash
cargo test
cargo run --bin matrix-preview -- --sample rain_then_snow --debug-layout --out preview.png
cargo run -- --emulate --once --out frame.png
cargo run -- --emulate --offline --sample rain_then_snow --seconds 4 --out demo.gif
```

The default build has no panel driver, so it builds and runs anywhere. `--features hardware` adds the
panel backend, which is Linux-only (it builds for the Pi, not on a Mac; `tools/cross-build.sh` does that).
Samples: `clear_day clear_night partly_cloudy overcast fog rain_then_snow snow thunder`.

## Configuration and secrets

- `config/config.toml` (gitignored): copy from `config/config.example.toml`.
- `secrets.env` (gitignored, `chmod 600`): copy from `config/secrets.env.example` and set
  `MATRIX_HA_TOKEN`. On the Pi this lives at `/etc/matrix/secrets.env`.
- `MATRIX_HA_URL` overrides `[homeassistant].url` (default `http://homeassistant:8123`).
- Enable the commit guard after `git init`: `git config core.hooksPath .githooks`.

Set your location once with a ZIP code (writes `[location]` into `config/config.toml`):

```bash
cargo run --bin matrix-geocode -- 10001
```

## Finding Home Assistant entities

```bash
cargo run --bin matrix-ha-discover -- --toml      # locks, room lights, gate candidates + config snippet
cargo run --bin matrix-ha-discover -- --all-lights --all-doors
```

## Layout

`src/`: `state.rs` (data contracts), `layout.rs` (screen regions), `config.rs`, `gfx.rs` (framebuffer),
`display/` (emulator and hardware backends), `compositor.rs` (region drawers), `scene/` (weather scene),
`ui/` (font, icons, widgets), `data/` (weather, Home Assistant, geocoding), `runner.rs` (the live loop),
`samples.rs` (named test states). Programs: `matrix-display` (`src/main.rs`) and the helpers in
`src/bin/`: `matrix-preview`, `matrix-geocode`, `matrix-ha-discover`, `matrix-set-config`, `matrix-hello`.

## Installing on the Pi

Download and run the installer from the latest GitHub release. It needs no git and no compiler on the Pi: the
program is built for 64-bit Raspberry Pi OS by GitHub Actions and downloaded as one checksummed archive.

```bash
curl -fsSL https://github.com/pshanesmith/LEDMatrixGadget/releases/latest/download/install.sh -o install.sh
less install.sh                  # it runs as root: read it first
sudo bash install.sh             # installs into /opt/matrix, creates /etc/matrix, starts the service
```

Then set your location and the Home Assistant token (see [docs/operations.md](docs/operations.md)).
Run the same two commands again to update. Options: `--version v0.2.0` pins a release, `--from-source [--ref main]`
clones the repository and compiles on the Pi instead (slow, needs about 1 GB of RAM with swap), `--no-start`,
`--no-drop`, `--yes`, `--dry-run`.

Other things you will want:

```bash
journalctl -u matrix-display -f                # live log (a performance line every 60 s)
sudo systemctl reload matrix-display           # re-read /etc/matrix/config.toml (SIGHUP)
sudo /opt/matrix/setup/03-install-service.sh --uninstall
```

**Making a release** (maintainers): bump `version` in `Cargo.toml`, commit, then `git tag v0.2.0 && git push origin v0.2.0`.
The release workflow runs the tests, cross-builds, packs and publishes `matrix-display-aarch64.tar.gz`, its checksum and
`install.sh`. To build one yourself on a Mac: `./tools/cross-build.sh && ./tools/package-release.sh`, then
`gh release create v0.2.0 dist/matrix-display-aarch64.tar.gz dist/matrix-display-aarch64.tar.gz.sha256 dist/install.sh`.

A private repository cannot use the plain download; use `--from-source` with an SSH URL (the installer creates a read-only
deploy key). See [docs/operations.md](docs/operations.md).

**Memory:** the Pi has about 460 MB of RAM. Close Claude Code and other large programs before starting the display.
