import argparse
import logging
import signal
import sys
from datetime import datetime

from .app import build_default_compositor
from .config import ConfigError, load_config
from .display.emulator import EmulatorDisplay
from .runner import Runner, Sources, StaticSources, state_dir
from .samples import sample_names, sample_state
from .state import AppState

log = logging.getLogger("matrix_display")


class RecordingDisplay(EmulatorDisplay):
    """Emulator that keeps every frame so a run can be saved as an animated GIF."""

    def __init__(self, *args, scale=4, **kwargs):
        super().__init__(*args, **kwargs)
        self.scale = scale
        self.frames = []

    def show(self, image):
        super().show(image)
        self.frames.append(self.render(self.scale, led_look=self.scale >= 4))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="matrix-display")
    ap.add_argument("--config", help="path to config.toml")
    ap.add_argument("--emulate", action="store_true", help="use the emulator instead of the panel")
    ap.add_argument("--once", action="store_true", help="render a single frame and exit (implies --emulate)")
    ap.add_argument("--seconds", type=float, help="with --emulate: run this long and save --out")
    ap.add_argument("--out", default="frame.png", help="PNG (or .gif with --seconds) for emulator output")
    ap.add_argument("--scale", type=int, default=8, help="PNG enlargement factor")
    ap.add_argument("--sample", choices=sample_names(), help="use a built-in sample state")
    ap.add_argument("--offline", action="store_true", help="no network: use --sample data (default clear_day)")
    ap.add_argument("--debug-layout", action="store_true", help="outline the screen regions")
    ap.add_argument("-v", "--verbose", action="store_true")
    args = ap.parse_args(argv)

    logging.basicConfig(level=logging.DEBUG if args.verbose else logging.INFO,
                        format="%(levelname)s %(name)s: %(message)s")
    try:
        cfg = load_config(args.config)
    except ConfigError as exc:
        print(f"config error: {exc}", file=sys.stderr)
        return 2

    static = args.sample or ("clear_day" if args.offline else None)
    compositor = build_default_compositor(debug_layout=args.debug_layout)

    if args.once:
        state = sample_state(static) if static else AppState(now=datetime.now().astimezone())
        display = EmulatorDisplay(cfg.display.cols * cfg.display.chain_length,
                                  cfg.display.rows * cfg.display.parallel)
        display.show(compositor.render(state))
        display.save_png(args.out, scale=args.scale, led_look=args.scale >= 4)
        print(f"wrote {args.out}")
        return 0

    # Create the display first: on hardware it drops root privileges, and everything started
    # afterwards (network threads, cache writes) then runs unprivileged.
    if args.emulate:
        display = RecordingDisplay(cfg.display.cols * cfg.display.chain_length,
                                   cfg.display.rows * cfg.display.parallel, scale=min(args.scale, 4))
    else:
        from .display.hardware import HardwareDisplay
        display = HardwareDisplay(cfg.display)

    if static:
        sample = sample_state(static)
        sources = StaticSources(sample.weather, sample.home)
    else:
        sources = Sources(cfg, state_dir())

    def reload():
        new_cfg = load_config(args.config)
        return new_cfg, (StaticSources(*sources.latest()) if static else Sources(new_cfg, state_dir()))

    runner = Runner(cfg, display, sources, compositor, reload=reload)
    signal.signal(signal.SIGTERM, lambda *_: runner.stop_event.set())
    signal.signal(signal.SIGINT, lambda *_: runner.stop_event.set())
    signal.signal(signal.SIGHUP, lambda *_: runner.reload_event.set())

    sources.start()
    try:
        max_frames = int(args.seconds * cfg.display.scene_fps) if args.seconds else None
        runner.run(max_frames=max_frames)
    finally:
        runner.sources.stop()
        display.close()
    if args.emulate and display.frames:
        if args.out.endswith(".gif"):
            display.frames[0].save(args.out, save_all=True, append_images=display.frames[1:],
                                   duration=int(1000 / cfg.display.scene_fps), loop=0)
        else:
            display.save_png(args.out, scale=args.scale, led_look=args.scale >= 4)
        print(f"wrote {args.out} ({len(display.frames)} frames)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
