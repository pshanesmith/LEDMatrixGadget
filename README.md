# Home Status Matrix

A 64x64 RGB LED matrix (Raspberry Pi Zero 2 W + Adafruit RGB Matrix Bonnet) showing the date,
an animated weather scene, and Home Assistant status. See [DESIGN.md](DESIGN.md) for the design
and [PLAN.md](PLAN.md) for the phases.

## Development setup (no panel needed)

```bash
python3 -m venv .venv
.venv/bin/pip install -e '.[dev]'
.venv/bin/pytest
.venv/bin/python tools/preview.py --sample rain_then_snow --debug-layout --out preview.png
.venv/bin/python -m matrix_display --emulate --once --out frame.png
```

## Configuration and secrets

- `config/config.toml` (gitignored): copy from `config/config.example.toml`.
- `secrets.env` (gitignored, `chmod 600`): copy from `config/secrets.env.example` and set
  `MATRIX_HA_TOKEN`. On the Pi this lives at `/etc/matrix/secrets.env`.
- `MATRIX_HA_URL` overrides `[homeassistant].url` (default `http://homeassistant:8123`).
- Enable the commit guard after `git init`: `git config core.hooksPath .githooks`.

## Finding Home Assistant entities

```bash
python3 tools/ha-discover.py --toml      # locks, room lights, gate candidates + config snippet
python3 tools/ha-discover.py --all-lights --all-doors
```

## Layout

`src/matrix_display/`: `state.py` (data contracts), `layout.py` (screen regions), `config.py`,
`display/` (emulator and hardware backends), `app.py` (Compositor; phases register region
drawers), `samples.py` (named test states). `tools/`: `ha-discover.py`, `preview.py`.

## Running as a service (Phase 6)

The service runs a copy of this repository deployed from GitHub to `/opt/matrix`, not your working checkout.

```bash
sudo ./setup/03-install-service.sh --apply     # clone to /opt/matrix, install, enable, start
journalctl -u matrix-display -f                # live log (a performance line every 60 s)
sudo /opt/matrix/setup/03-install-service.sh --apply   # UPDATE: pull the latest main, reinstall, restart
sudo systemctl reload matrix-display           # re-read /etc/matrix/config.toml (SIGHUP)
sudo ./setup/03-install-service.sh --uninstall
```

A private repository needs an SSH URL (`git@github.com:USER/LEDMatrixGadget.git`): the installer creates a read-only deploy key and prints the public half to add under the repository's Settings > Deploy keys. See [docs/operations.md](docs/operations.md).

Try it without the panel first: `.venv/bin/python -m matrix_display --emulate --offline --sample rain_then_snow --seconds 4 --out demo.gif`.
**Memory:** the Pi has about 460 MB of RAM. Close Claude Code and other large programs before starting the display.
