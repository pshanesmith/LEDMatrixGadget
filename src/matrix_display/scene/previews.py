"""Render scene previews for every sample state:  python -m matrix_display.scene.previews [outdir]

Writes scene-<name>.png (scale 8, LED look off so every pixel is readable) and a
scene-<name>.gif of two seconds of animation.
"""
import sys
from pathlib import Path

from PIL import Image

from ..samples import sample_names, sample_state
from .scene import render_state

SCALE = 8


def main(outdir: str = "docs/previews", gif_frames: int = 30, fps: int = 15) -> None:
    out = Path(outdir)
    out.mkdir(parents=True, exist_ok=True)
    for name in sample_names():
        state = sample_state(name)
        big = lambda im: im.resize((im.width * SCALE, im.height * SCALE), Image.NEAREST)  # noqa: E731
        big(render_state(state, 3.0)).save(out / f"scene-{name}.png")
        frames = [big(render_state(state, 3.0 + i / fps)) for i in range(gif_frames)]
        frames[0].save(out / f"scene-{name}.gif", save_all=True, append_images=frames[1:],
                       duration=1000 // fps, loop=0)
        print("wrote", out / f"scene-{name}.png")


if __name__ == "__main__":
    main(*sys.argv[1:2])
