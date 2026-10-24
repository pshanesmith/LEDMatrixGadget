"""UI widget tests: golden images (scene excluded, so they are deterministic), text
overflow, icons and brightness. Regenerate goldens with UPDATE_GOLDEN=1."""
import os
from dataclasses import replace
from datetime import timedelta
from pathlib import Path
from types import SimpleNamespace

import pytest
from PIL import Image, ImageChops

from matrix_display import layout
from matrix_display.app import brightness_for, build_default_compositor
from matrix_display.samples import NOON, sample_state
from matrix_display.state import Condition
from matrix_display.ui import icons
from matrix_display.ui.fonts import GLYPHS, text_width
from matrix_display.ui.variants import variants

GOLDEN = Path(__file__).parent / "golden"
GOLDEN_CASES = ["all_clear", "rain_then_snow", "counts_zero", "counts_twelve",
                "gate_open", "weather_none", "stale_home", "stale_weather"]


def _render(state, t=0.0):
    return build_default_compositor(include_scene=False).render(state, t)


@pytest.mark.parametrize("name", GOLDEN_CASES)
def test_golden(name):
    img = _render(variants()[name])
    path = GOLDEN / f"ui-{name}.png"
    if os.environ.get("UPDATE_GOLDEN") or not path.exists():
        GOLDEN.mkdir(exist_ok=True)
        img.save(path)
    assert ImageChops.difference(img, Image.open(path).convert("RGB")).getbbox() is None


def test_gate_blinks():
    state = variants()["gate_open"]
    a, b = _render(state, 0.0), _render(state, 1.0)
    assert ImageChops.difference(a, b).getbbox() is not None


def test_none_state_does_not_raise():
    from matrix_display.state import AppState
    _render(AppState(now=NOON))


def test_all_widgets_registered():
    comp = build_default_compositor(include_scene=False)
    assert {"date", "temps", "next_change", "home"} <= set(comp._drawers)


def test_icon_sizes():
    for cond in Condition:
        rows = icons.WEATHER[cond]
        assert len(rows) == 9 and all(len(r) == 9 for r in rows), cond
    for rows in (*icons.LOCK.values(), icons.BULB):
        assert len(rows) == 9 and all(len(r) == 7 for r in rows)
    for rows in icons.GATE.values():
        assert len(rows) == 9 and all(len(r) == 9 for r in rows)


def test_font_covers_required_chars():
    for ch in "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ /:.-+?":
        assert ch in GLYPHS
    for g in GLYPHS.values():
        assert len(g) == 5


@pytest.mark.parametrize("low,high", [(-40, 130), (-9, 9), (0, 100), (-100, 999), (5, 15)])
@pytest.mark.parametrize("count", [0, 7, 12, 99, 250])
@pytest.mark.parametrize("t", [0.0, 1.0])
def test_widgets_stay_inside_their_region(low, high, count, t):
    from matrix_display.ui.widgets import WIDGETS
    base = variants()["rain_then_snow"]
    w = replace(base.weather, daily=replace(base.weather.daily, low=low, high=high))
    h = replace(base.home, locks_unlocked=count, lights_on=count, locks_unavailable=1,
                gates_open=("g",))
    state = replace(base, weather=w, home=h)
    for name, drawer in WIDGETS.items():
        rect = layout.REGIONS[name]
        frame = Image.new("RGB", (64, 64))
        drawer(frame, rect, state, t)
        bbox = frame.getbbox()
        assert bbox is not None, name
        assert bbox[0] >= rect.x and bbox[1] >= rect.y, name
        assert bbox[2] <= rect.x2 and bbox[3] <= rect.y2, name


@pytest.mark.parametrize("text", ["FRI 10/23", "WED 12/31", "THUNDER 11A", "STEADY 24H",
                                  "PARTLY 12P", "v-100", "^999", "99?", "STORM 12A"])
def test_text_widths_fit(text):
    assert text_width(text) <= 64


def test_longest_next_change_row_fits():
    assert 9 + 3 + text_width("PARTLY 12P") <= 64


def _cfg(**kw):
    return SimpleNamespace(**{"brightness": 50, "max_brightness": 70, **kw})


def test_brightness():
    daily = sample_state().weather.daily          # sunrise 07:40, sunset 18:20
    assert brightness_for(NOON, daily, _cfg()) == 50
    assert brightness_for(NOON.replace(hour=22), daily, _cfg()) == 25
    assert brightness_for(NOON.replace(hour=5), daily, _cfg()) == 25
    assert brightness_for(NOON, None, _cfg()) == 50
    assert brightness_for(NOON, daily, _cfg(brightness=90)) == 70
    assert brightness_for(NOON.replace(hour=22), daily, _cfg(brightness=1)) >= 1
