"""The electricity rate tier: Home Assistant sensor -> HomeSnapshot -> coloured bolt."""
import json
from dataclasses import replace

import pytest

from matrix_display import layout
from matrix_display.app import build_default_compositor
from matrix_display.config import HomeAssistantConfig, build_config
from matrix_display.data import homeassistant as ha
from matrix_display.samples import NOON
from matrix_display.ui import icons, widgets
from matrix_display.ui.variants import variants

TIER = "sensor.duke_energy_r_tou_cpp_tier"


def config(**kw):
    return HomeAssistantConfig(url="http://ha.local", token="t", gates=("cover.gate",), **kw)


def payload(**kw):
    return {"locks": [], "lights": ["light.a"], "gates": ["off"], **kw}


# --- data ------------------------------------------------------------------


@pytest.mark.parametrize("state", ["discount", "off_peak", "on_peak", "critical_peak", "unavailable"])
def test_tier_is_read_from_the_sensor(state):
    snap = ha.parse_template_result(payload(tier=state), config(power_tier_entity=TIER))
    assert snap.power_tier == state


def test_not_configured_means_no_bolt_even_if_the_key_is_present():
    assert ha.parse_template_result(payload(tier="on_peak"), config()).power_tier is None


@pytest.mark.parametrize("extra", [{}, {"tier": None}, {"tier": ""}])
def test_configured_but_missing_tier_is_unknown(extra):
    snap = ha.parse_template_result(payload(**extra), config(power_tier_entity=TIER))
    assert snap.power_tier == "unknown"


def test_template_names_the_entity_and_rejects_bad_ids():
    template = ha.build_template(("cover.gate",), TIER)
    assert json.dumps(TIER) in template and "states(tier_entity)" in template
    assert 'set tier_entity = ""' in ha.build_template(("cover.gate",))
    with pytest.raises(ValueError):
        ha.build_template((), "sensor.x') }}{{ evil")


def test_cache_round_trip_keeps_the_tier():
    snap = ha.parse_template_result(payload(tier="critical_peak"), config(power_tier_entity=TIER))
    assert ha.snapshot_from_json(ha.snapshot_to_json(snap)).power_tier == "critical_peak"
    old = ha.snapshot_to_json(snap)
    del old["power_tier"]  # a cache written before the bolt existed
    assert ha.snapshot_from_json(old).power_tier is None


def test_config_key():
    cfg = build_config({"homeassistant": {"power_tier_entity": TIER}}, environ={})
    assert cfg.homeassistant.power_tier_entity == TIER
    assert build_config({}, environ={}).homeassistant.power_tier_entity == ""


# --- drawing ---------------------------------------------------------------


def _bolt_pixels(state):
    img = build_default_compositor(include_scene=False).render(state, 0.0)
    x0, y0 = layout.HOME.x + widgets.BOLT_X, layout.HOME.y + 1
    return {img.getpixel((x0 + dx, y0 + dy))
            for dy, row in enumerate(icons.BOLT) for dx, ch in enumerate(row) if ch != "."}


@pytest.mark.parametrize("name,color", [
    ("tier_discount", widgets.GREEN),
    ("tier_off_peak", widgets.BLUE),
    ("tier_on_peak", widgets.ORANGE),
    ("tier_critical", widgets.RED),
    ("tier_unavailable", widgets.DIM),
])
def test_bolt_colour_follows_the_tier(name, color):
    assert _bolt_pixels(variants()[name]) == {color}


def test_tier_colours_are_the_requested_ones():
    assert widgets.TIER_COLORS["discount"][1] > 200          # green
    assert widgets.TIER_COLORS["off_peak"][2] > 200          # blue
    r, g, b = widgets.TIER_COLORS["on_peak"]
    assert r > 200 and 100 < g < 180 and b < 80              # orange
    r, g, b = widgets.TIER_COLORS["critical_peak"]
    assert r > 200 and g < 80 and b < 80                     # red


def test_no_bolt_without_a_tier_and_layout_is_the_old_three_cells():
    base = variants()["counts_zero"]
    assert base.home.power_tier is None
    with_tier = replace(base, home=replace(base.home, power_tier="on_peak"))
    a = build_default_compositor(include_scene=False).render(base, 0.0)
    b = build_default_compositor(include_scene=False).render(with_tier, 0.0)
    assert a != b
    # gate icon sits in the middle of the third 21 px cell without the bolt
    assert a.getpixel((layout.HOME.x + 42 + 6, layout.HOME.y + 1)) == widgets.GREEN


def test_gate_and_bolt_are_close_together_but_clear_of_the_counts():
    state = variants()["tier_counts_twelve"]
    img = build_default_compositor(include_scene=False).render(state, 0.0)
    y = range(layout.HOME.y + 1, layout.HOME.y + 10)

    def lit(x):
        return any(img.getpixel((x, yy)) != (0, 0, 0) for yy in y)

    gate_right = widgets.BOLT_GATE_X + 9
    assert widgets.BOLT_X - gate_right == 3                       # tight, as the gate has no digits
    assert not any(lit(x) for x in range(gate_right, widgets.BOLT_X))
    assert lit(widgets.BOLT_GATE_X) and lit(widgets.BOLT_X + 3)
    # two-digit lock and bulb counts never reach the gate
    assert not any(lit(x) for x in range(widgets.BOLT_GATE_X - 2, widgets.BOLT_GATE_X))


@pytest.mark.parametrize("count", [0, 9, 12, 99, 250])
@pytest.mark.parametrize("t", [0.0, 1.0])
def test_bolt_row_stays_inside_the_region(count, t):
    state = variants()["tier_counts_twelve"]
    state = replace(state, home=replace(state.home, locks_unlocked=count, lights_on=count))
    img = build_default_compositor(include_scene=False).render(state, t)
    empty = replace(state, home=None)
    ref = build_default_compositor(include_scene=False).render(empty, t)
    above = (0, 0, 64, layout.HOME.y)
    assert img.crop(above).tobytes() == ref.crop(above).tobytes()
