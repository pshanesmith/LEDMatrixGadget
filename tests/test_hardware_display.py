"""HardwareDisplay with a fake rgbmatrix module: how frames are pushed, without the panel."""
import sys
import types

import pytest
from PIL import Image

from matrix_display.config import DisplayConfig


class FakeCanvas:
    def __init__(self):
        self.calls = []

    def SetImage(self, image, offset_x=0, offset_y=0, unsafe=True):
        self.calls.append(unsafe)


class FakeMatrix:
    def __init__(self, options=None):
        self.options, self.canvas, self.brightness = options, FakeCanvas(), 50

    def CreateFrameCanvas(self):
        return self.canvas

    def SwapOnVSync(self, canvas):
        return canvas

    def Clear(self):
        pass


@pytest.fixture
def fake_rgbmatrix(monkeypatch):
    mod = types.ModuleType("rgbmatrix")
    mod.RGBMatrix = FakeMatrix
    mod.RGBMatrixOptions = type("RGBMatrixOptions", (), {})
    monkeypatch.setitem(sys.modules, "rgbmatrix", mod)


def make(cfg):
    from matrix_display.display.hardware import HardwareDisplay
    return HardwareDisplay(cfg)


def test_default_uses_the_public_api_path(fake_rgbmatrix):
    d = make(DisplayConfig())
    d.show(Image.new("RGB", (64, 64)))
    assert d._canvas.calls == [False]            # unsafe=False: never reads Pillow's internals


def test_fast_path_is_opt_in(fake_rgbmatrix):
    d = make(DisplayConfig(safe_image_copy=False))
    d.show(Image.new("RGB", (64, 64)))
    assert d._canvas.calls == [True]


def test_rejects_wrong_size_frames(fake_rgbmatrix):
    with pytest.raises(ValueError):
        make(DisplayConfig()).show(Image.new("RGB", (32, 32)))


def test_options_passed_through(fake_rgbmatrix):
    d = make(DisplayConfig(pwm_bits=9, limit_refresh_hz=150, brightness=90, max_brightness=70))
    o = d._matrix.options
    assert (o.pwm_bits, o.limit_refresh_rate_hz, o.brightness, o.hardware_mapping) == (9, 150, 70, "adafruit-hat-pwm")
