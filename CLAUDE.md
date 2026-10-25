# LEDMatrixGadget

64x64 RGB LED matrix status display (Raspberry Pi Zero 2 W). See README.md, DESIGN.md, docs/operations.md.

## Workflow: develop on the Mac, test on the Pi
- Develop locally in `.venv` (Python >= 3.11; create with `uv venv --python 3.12 .venv && uv pip install --python .venv/bin/python -e '.[dev]'`).
- Verify with `.venv/bin/pytest` and the emulator: `tools/preview.py` or `python -m matrix_display --emulate --offline --sample <name> --out x.png|gif`. No panel is needed.
- Tests in `tests/test_setup_boot.py` exercise the Pi setup scripts and are skipped on non-Linux.
- Deploy: push to GitHub, then on the Pi run `sudo /opt/matrix/setup/03-install-service.sh --apply` (`--ref <branch|tag>` to try a branch). `/opt/matrix` is a root-owned deploy copy; never edit it.
- Do not run Claude Code or other large programs on the Pi (about 460 MB RAM; it causes watchdog resets).
- `config/config.toml` and secrets are gitignored; on the Pi they live in `/etc/matrix/`.
