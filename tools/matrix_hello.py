#!/usr/bin/env python3
"""Hardware smoke test through the project's own HardwareDisplay backend.

Run with root (GPIO access):  sudo .venv/bin/python tools/matrix_hello.py [options]

Draws an image that makes the usual wiring problems obvious:
  - top half red-ish, bottom half blue-ish  -> a duplicated/missing half means the E jumper
  - white 1px border                         -> clipped or shifted edges
  - R, G, B bars across the middle           -> wrong color order (try --rgb-sequence)
  - row ticks every 8 pixels on the left     -> missing or repeated rows
  - a small moving white square              -> flicker and tearing while animating
"""
import argparse
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from PIL import Image, ImageDraw  # noqa: E402

from matrix_display.config import ConfigError, DisplayConfig, load_config  # noqa: E402


def build_image(size: int, t: float) -> Image.Image:
    img = Image.new("RGB", (size, size))
    d = ImageDraw.Draw(img)
    half = size // 2
    d.rectangle([0, 0, size - 1, half - 1], fill=(40, 0, 0))
    d.rectangle([0, half, size - 1, size - 1], fill=(0, 0, 40))
    for i, color in enumerate(((255, 0, 0), (0, 255, 0), (0, 0, 255))):
        y = half - 6 + i * 4
        d.rectangle([8, y, size - 9, y + 3], fill=color)
    for y in range(0, size, 8):
        d.line([(1, y), (4 if y % 16 else 7, y)], fill=(255, 255, 0))
    d.rectangle([0, 0, size - 1, size - 1], outline=(255, 255, 255))
    x = 6 + int((size - 18) * (0.5 + 0.5 * __import__("math").sin(t * 2)))
    d.rectangle([x, 6, x + 5, 11], fill=(255, 255, 255))
    d.rectangle([x, size - 12, x + 5, size - 7], fill=(255, 255, 255))
    return img


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--config", help="config.toml (default: search)")
    ap.add_argument("--seconds", type=float, default=20)
    for name, typ in (("rows", int), ("cols", int), ("gpio-slowdown", int), ("pwm-bits", int),
                      ("pwm-lsb-nanoseconds", int), ("row-address-type", int),
                      ("multiplexing", int), ("brightness", int),
                      ("hardware-mapping", str), ("rgb-sequence", str)):
        ap.add_argument(f"--{name}", type=typ, help="override [display] setting")
    args = ap.parse_args()

    try:
        cfg = load_config(args.config).display
    except ConfigError as exc:
        print(f"config error: {exc}", file=sys.stderr)
        return 2
    overrides = {k: v for k, v in vars(args).items()
                 if k in DisplayConfig.__dataclass_fields__ and v is not None}
    cfg = DisplayConfig(**{**cfg.__dict__, **overrides})
    print(f"settings: {cfg}")

    from matrix_display.display.hardware import HardwareDisplay  # needs the bindings
    display = HardwareDisplay(cfg)       # drops root privileges after setup
    try:
        start = time.time()
        while time.time() - start < args.seconds:
            display.show(build_image(display.width, time.time() - start))
            time.sleep(1 / 30)
    except KeyboardInterrupt:
        pass
    finally:
        display.close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
