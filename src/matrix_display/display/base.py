from abc import ABC, abstractmethod

from PIL import Image


class Display(ABC):
    """A 64x64 RGB output. Backends: hardware (rgbmatrix) and emulator (PIL)."""

    width: int
    height: int

    @abstractmethod
    def show(self, image: Image.Image) -> None:
        """Present a frame. `image` must be RGB and exactly width x height."""

    @abstractmethod
    def set_brightness(self, percent: int) -> None:
        """Set brightness 1-100 (clamped to the configured max by the caller)."""

    def close(self) -> None:
        """Blank the display and release resources."""

    def _check(self, image: Image.Image) -> Image.Image:
        if image.size != (self.width, self.height):
            raise ValueError(f"frame is {image.size}, display is {(self.width, self.height)}")
        return image if image.mode == "RGB" else image.convert("RGB")
