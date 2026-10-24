from pathlib import Path

from PIL import Image, ImageDraw

from .base import Display


class EmulatorDisplay(Display):
    """Keeps frames in memory and writes previews; no hardware needed."""

    def __init__(self, width: int = 64, height: int = 64):
        self.width = width
        self.height = height
        self.brightness = 100
        self.last_frame: Image.Image | None = None

    def show(self, image: Image.Image) -> None:
        self.last_frame = self._check(image).copy()

    def set_brightness(self, percent: int) -> None:
        self.brightness = max(1, min(100, percent))

    def render(self, scale: int = 1, led_look: bool = False) -> Image.Image:
        """Return the last frame enlarged. `led_look` draws round dots on black."""
        frame = self.last_frame or Image.new("RGB", (self.width, self.height))
        if not led_look or scale < 4:
            return frame.resize((self.width * scale, self.height * scale), Image.NEAREST)
        out = Image.new("RGB", (self.width * scale, self.height * scale), (6, 6, 6))
        draw = ImageDraw.Draw(out)
        for y in range(self.height):
            for x in range(self.width):
                draw.ellipse(
                    [x * scale + 1, y * scale + 1, (x + 1) * scale - 2, (y + 1) * scale - 2],
                    fill=frame.getpixel((x, y)),
                )
        return out

    def save_png(self, path: str | Path, scale: int = 1, led_look: bool = False) -> None:
        self.render(scale, led_look).save(path)
