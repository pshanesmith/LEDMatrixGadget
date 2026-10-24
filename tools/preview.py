#!/usr/bin/env python3
"""Render a sample state to PNG, or an animated GIF, using the emulator.

  tools/preview.py --sample rain_then_snow --out rain.png
  tools/preview.py --sample thunder --gif --frames 45 --fps 15 --out storm.gif
--all writes every sample (and extra UI variants) to docs/previews/ui-<name>.png.
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from matrix_display.app import build_default_compositor  # noqa: E402
from matrix_display.display.emulator import EmulatorDisplay  # noqa: E402
from matrix_display.samples import sample_names, sample_state  # noqa: E402


def build_compositor(debug_layout=False):
    return build_default_compositor(debug_layout=debug_layout)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--sample", choices=sample_names(), default="clear_day")
    ap.add_argument("--out", default="preview.png")
    ap.add_argument("--scale", type=int, default=8)
    ap.add_argument("--gif", action="store_true")
    ap.add_argument("--frames", type=int, default=30)
    ap.add_argument("--fps", type=int, default=15)
    ap.add_argument("--debug-layout", action="store_true")
    ap.add_argument("--all", action="store_true",
                    help="write every sample and UI variant to docs/previews/ui-<name>.png")
    a = ap.parse_args(argv)

    if a.all:
        from matrix_display.ui.variants import variants
        out_dir = Path(__file__).resolve().parent.parent / "docs" / "previews"
        out_dir.mkdir(parents=True, exist_ok=True)
        comp, disp = build_compositor(a.debug_layout), EmulatorDisplay()
        states = {n: sample_state(n) for n in sample_names()}
        states.update({f"var-{n}": s for n, s in variants().items()})
        for name, st in states.items():
            disp.show(comp.render(st, 0.0))
            disp.save_png(out_dir / f"ui-{name}.png", a.scale, led_look=a.scale >= 4)
            print(f"wrote {out_dir / f'ui-{name}.png'}")
        return
    comp, state, disp = build_compositor(a.debug_layout), sample_state(a.sample), EmulatorDisplay()
    if not a.gif:
        disp.show(comp.render(state, 0.0))
        disp.save_png(a.out, a.scale, led_look=a.scale >= 4)
    else:
        frames = []
        for i in range(a.frames):
            disp.show(comp.render(state, i / a.fps))
            frames.append(disp.render(a.scale, led_look=a.scale >= 4))
        frames[0].save(a.out, save_all=True, append_images=frames[1:],
                       duration=int(1000 / a.fps), loop=0)
    print(f"wrote {a.out}")


if __name__ == "__main__":
    main()
