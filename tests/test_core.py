import dataclasses
import json
import subprocess
import sys
from pathlib import Path

import pytest
from PIL import Image

from matrix_display import layout
from matrix_display.app import Compositor
from matrix_display.config import ConfigError, build_config, normalize_url
from matrix_display.display.emulator import EmulatorDisplay
from matrix_display.samples import sample_names, sample_state
from matrix_display.state import Condition, condition_from_wmo

ROOT = Path(__file__).resolve().parent.parent


@pytest.mark.parametrize("code,expected", [
    (0, Condition.CLEAR), (1, Condition.CLEAR), (2, Condition.PARTLY_CLOUDY),
    (3, Condition.OVERCAST), (45, Condition.FOG), (48, Condition.FOG),
    (51, Condition.RAIN), (67, Condition.RAIN), (81, Condition.RAIN),
    (71, Condition.SNOW), (77, Condition.SNOW), (86, Condition.SNOW),
    (95, Condition.THUNDER), (99, Condition.THUNDER), (1234, Condition.OVERCAST)])
def test_condition_mapping(code, expected):
    assert condition_from_wmo(code) is expected


def test_regions_tile_the_screen_without_overlap():
    rects = sorted(layout.REGIONS.values(), key=lambda r: r.y)
    assert rects[0].y == 0 and rects[-1].y2 == layout.HEIGHT
    for a, b in zip(rects, rects[1:]):
        assert a.y2 == b.y
    assert all(r.x == 0 and r.w == layout.WIDTH for r in rects)


def test_normalize_url():
    assert normalize_url("homeassistant:8123/") == "http://homeassistant:8123"
    assert normalize_url("https://ha.example/") == "https://ha.example"
    with pytest.raises(ConfigError):
        normalize_url("ftp://x")


def test_config_defaults_and_ha_url_precedence():
    cfg = build_config({}, environ={})
    assert cfg.homeassistant.url == "http://homeassistant:8123"
    assert cfg.display.hardware_mapping == "adafruit-hat-pwm"
    toml_url = {"homeassistant": {"url": "10.0.0.5:8123"}}
    assert build_config(toml_url, environ={}).homeassistant.url == "http://10.0.0.5:8123"
    cfg = build_config(toml_url, environ={"MATRIX_HA_URL": "ha.lan"})
    assert cfg.homeassistant.url == "http://ha.lan"


def test_token_only_from_environment_and_not_repr():
    cfg = build_config({"homeassistant": {"token": "from-file"}}, environ={"MATRIX_HA_TOKEN": "secret-xyz"})
    assert cfg.homeassistant.token == "secret-xyz"
    assert "secret-xyz" not in repr(cfg)
    assert build_config({"homeassistant": {"token": "from-file"}}, environ={}).homeassistant.token == ""


def test_config_validation():
    with pytest.raises(ConfigError):
        build_config({"display": {"bogus": 1}}, environ={})
    with pytest.raises(ConfigError):
        build_config({"location": {"latitude": 95, "longitude": 0}}, environ={})
    with pytest.raises(ConfigError):
        build_config({}, environ={}).require_location()
    cfg = build_config({"display": {"brightness": 90, "max_brightness": 70}}, environ={})
    assert cfg.display.brightness == 70


@pytest.mark.parametrize("name", sample_names())
def test_samples_are_well_formed(name):
    s = sample_state(name)
    assert len(s.weather.hourly) == 48
    assert s.weather.daily.low < s.weather.daily.high
    assert s.now.tzinfo is not None
    dataclasses.replace(s.home, lights_on=3)  # frozen dataclass, replace works


def test_compositor_blank_and_debug_and_register():
    state = sample_state()
    blank = Compositor().render(state)
    assert blank.size == (64, 64) and blank.getbbox() is None
    assert Compositor(debug_layout=True).render(state).getbbox() is not None
    comp = Compositor()
    comp.register("temps", lambda f, r, s, t: f.putpixel((r.x, r.y), (255, 0, 0)))
    assert comp.render(state).getpixel((0, layout.TEMPS.y)) == (255, 0, 0)
    with pytest.raises(KeyError):
        comp.register("nope", lambda *a: None)


def test_emulator_rejects_wrong_size_and_scales(tmp_path):
    d = EmulatorDisplay()
    with pytest.raises(ValueError):
        d.show(Image.new("RGB", (32, 32)))
    d.show(Image.new("RGB", (64, 64), (1, 2, 3)))
    assert d.render(8, led_look=True).size == (512, 512)
    d.save_png(tmp_path / "a.png", 2)
    assert Image.open(tmp_path / "a.png").size == (128, 128)


def test_cli_emulate_once_writes_png(tmp_path):
    out = tmp_path / "frame.png"
    r = subprocess.run([sys.executable, "-m", "matrix_display", "--emulate", "--once",
                        "--out", str(out), "--scale", "1", "--config", str(ROOT / "config/config.example.toml")],
                       cwd=ROOT, env={"PYTHONPATH": str(ROOT / "src"), "PATH": "/usr/bin"},
                       capture_output=True, text=True)
    assert r.returncode == 0, r.stderr
    assert Image.open(out).size == (64, 64)


def test_fixtures_parse():
    for name in ("open_meteo_sample.json", "ha_template_response.json"):
        json.loads((ROOT / "tests/fixtures" / name).read_text())
