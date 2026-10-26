# Home Status Matrix

A 64x64 RGB LED matrix (Raspberry Pi Zero 2 W + Adafruit RGB Matrix Bonnet) showing the date,
an animated weather scene, and Home Assistant status. Written in Rust; the panel is driven by the
[`rpi-led-panel`](https://github.com/EmbersArc/rpi_led_panel) crate, a pure Rust port of hzeller's driver. See
[DESIGN.md](DESIGN.md) for the design and [docs/operations.md](docs/operations.md) for day-to-day operation.

## Development host requirements

Everything below is for the machine you develop on (a Mac, or Linux). The Pi itself needs none of it when you install
a release.

| Needed for | Tool | Install (macOS) |
|---|---|---|
| Building, tests, previews, the emulator | Rust 1.85 or newer (`cargo`, `rustc`; `clippy` for linting) | `brew install rustup && rustup default stable` (or `brew install rust`) |
| The shell-script tests and `tools/package-release.sh` | `bash`, `tar`, `curl`, and `sha256sum` or `shasum` | already on macOS and Linux |
| Cross-building for the Pi (`tools/cross-build.sh`) | `zig`, `cargo-zigbuild`, and the Rust target `aarch64-unknown-linux-gnu` (the glibc build for Raspberry Pi OS; add `aarch64-unknown-linux-musl` for a static one) | `brew install zig cargo-zigbuild`, then `rustup target add aarch64-unknown-linux-gnu` |
| Publishing a release by hand | GitHub CLI `gh`, logged in | `brew install gh && gh auth login` |
| Pi setup-script tests (`apt-config`, `systemd-analyze` checks) | Linux only; skipped elsewhere | none |

No Python, C++ toolchain, `make` or image library is needed. The first `cargo build` downloads the crates listed in
`Cargo.lock` from crates.io, so it needs network access. Releases made by GitHub Actions
(`.github/workflows/release.yml`) install their own toolchain (Rust, `ziglang` from pip, `cargo-zigbuild`), so none
of this is needed for them. If `cargo` is installed through `rustup` but not on your `PATH`, it is in
`~/.rustup/toolchains/stable-*/bin`.

## Development setup (no panel needed)

```bash
cargo test
cargo run --bin matrix-preview -- --sample rain_then_snow --debug-layout --out preview.png
cargo run -- --emulate --once --out frame.png
cargo run -- --emulate --offline --sample rain_then_snow --seconds 4 --out demo.gif
```

The default build has no panel driver, so it builds and runs anywhere. `--features hardware` adds the
panel backend, which is Linux-only (it builds for the Pi, not on a Mac; `tools/cross-build.sh` does that).
The panel driver is [our fork](https://github.com/pshanesmith/rpi_led_panel) of `rpi-led-panel` (a 64-bit alignment fix), pulled in by Cargo from GitHub at a pinned commit.
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
curl -fsSL https://github.com/pshanesmith/LEDMatrixGadget/releases/latest/download/install.sh | sudo bash
```

That installs into `/opt/matrix`, creates `/etc/matrix` and starts the service. Options go after `-s --`, for example
`... | sudo bash -s -- --version v0.2.1 --yes`. Because the script runs as root, you may prefer to read it first:
`curl -fsSLO <same url>`, `less install.sh`, `sudo bash install.sh`.

Then set your location and the Home Assistant token (see [docs/operations.md](docs/operations.md)).
Run the same command again to update. Options: `--version v0.2.0` pins a release, `--from-source [--ref main]`
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
