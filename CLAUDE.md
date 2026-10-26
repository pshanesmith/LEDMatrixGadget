# LEDMatrixGadget

64x64 RGB LED matrix status display (Raspberry Pi Zero 2 W), written in Rust. See README.md, DESIGN.md, docs/operations.md.

## Workflow: develop on the Mac, test on the Pi
- Develop locally with Rust 1.85 or newer (`brew install rustup` and `rustup default stable`, or `brew install rust`). With a rustup toolchain that is not linked into the PATH, use `~/.rustup/toolchains/stable-*/bin`.
- Verify with `cargo test` and the emulator: `cargo run --bin matrix-preview -- --sample <name> --out x.png` or `cargo run -- --emulate --offline --sample <name> --seconds 4 --out x.png|gif`. No panel is needed; the default build has no panel driver.
- The panel backend (`--features hardware`, crate `rpi-led-panel`) is Linux-only and does not compile on a Mac. Build it with `tools/cross-build.sh --target aarch64-unknown-linux-musl` (needs `zig`, `cargo-zigbuild`; the musl target is a static binary, the default glibc one needs `rustup target add aarch64-unknown-linux-gnu`), and compile its tests with `cargo zigbuild --lib --tests --features hardware --target aarch64-unknown-linux-musl`. They cannot run off the Pi.
- The scene and widgets match images made by the original Python version pixel for pixel (`tests/golden/`). A rendering change must update them deliberately: `UPDATE_GOLDEN=1 cargo test --test ui` for the widget screens; `tests/golden/scene-cases.json` lists the scene cases.
- Tests in `tests/setup_scripts.rs` exercise the Pi setup scripts and the boot-file shell helpers; the shell ones run on Linux only.
- Deploy: tag a release (`git tag vX.Y.Z && git push origin vX.Y.Z`; the tag must match `Cargo.toml`) and GitHub Actions builds and publishes it; on the Pi run `sudo bash install.sh` (downloaded from the latest release; `--version TAG` pins one; `--from-source --ref <branch>` compiles a branch on the Pi). `/opt/matrix` is a root-owned deploy copy; never edit it.
- Do not run Claude Code or other large programs on the Pi (about 460 MB RAM; it causes watchdog resets), and avoid compiling there if a cross-build will do.
- `config/config.toml` and secrets are gitignored; on the Pi they live in `/etc/matrix/`.
