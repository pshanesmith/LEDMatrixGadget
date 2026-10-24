from PIL import Image

from ..config import DisplayConfig
from .base import Display


class HardwareDisplay(Display):
    """hzeller rgbmatrix backend. Needs the bindings from setup/02-matrix-setup.sh.

    Verified on the real panel with tools/test-matrix.sh and the running service.
    """

    def __init__(self, cfg: DisplayConfig):
        try:
            from rgbmatrix import RGBMatrix, RGBMatrixOptions
        except ImportError as exc:
            raise RuntimeError(
                "rgbmatrix bindings not installed; run setup/02-matrix-setup.sh "
                "or use --emulate") from exc
        o = RGBMatrixOptions()
        o.rows = cfg.rows
        o.cols = cfg.cols
        o.chain_length = cfg.chain_length
        o.parallel = cfg.parallel
        o.hardware_mapping = cfg.hardware_mapping
        o.gpio_slowdown = cfg.gpio_slowdown
        o.pwm_bits = cfg.pwm_bits
        o.pwm_lsb_nanoseconds = cfg.pwm_lsb_nanoseconds
        o.limit_refresh_rate_hz = cfg.limit_refresh_hz
        o.row_address_type = cfg.row_address_type
        o.multiplexing = cfg.multiplexing
        o.led_rgb_sequence = cfg.rgb_sequence
        o.brightness = min(cfg.brightness, cfg.max_brightness)
        o.drop_privileges = cfg.drop_privileges
        self.width = cfg.cols * cfg.chain_length
        self.height = cfg.rows * cfg.parallel
        # The bindings' fast path reads Pillow's private image struct, which can change between Pillow versions
        # (and an automatic upgrade would then draw garbage). The safe path uses only Pillow's public API.
        self._unsafe = not cfg.safe_image_copy
        self._matrix = RGBMatrix(options=o)
        self._canvas = self._matrix.CreateFrameCanvas()

    def show(self, image: Image.Image) -> None:
        self._canvas.SetImage(self._check(image), 0, 0, unsafe=self._unsafe)
        self._canvas = self._matrix.SwapOnVSync(self._canvas)

    def set_brightness(self, percent: int) -> None:
        self._matrix.brightness = max(1, min(100, percent))

    def close(self) -> None:
        self._matrix.Clear()
