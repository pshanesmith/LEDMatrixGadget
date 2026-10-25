import time
from dataclasses import replace
from datetime import timedelta

import pytest
from PIL import Image, ImageChops

from matrix_display import layout
from matrix_display.samples import NOON, sample_names, sample_state
from matrix_display.scene import SceneRenderer, draw_scene, render_state
from matrix_display.scene import mountains, particles
from matrix_display.scene.palette import sun_position
from matrix_display.state import AppState, Condition


def blank():
    return Image.new("RGB", (layout.WIDTH, layout.HEIGHT), (1, 2, 3))


def with_weather(state, **cur):
    return replace(state, weather=replace(state.weather, current=replace(state.weather.current, **cur)))


@pytest.mark.parametrize("name", sample_names())
def test_every_sample_renders_and_animates(name):
    state = sample_state(name)
    a, b = render_state(state, 0.0), render_state(state, 2.5)
    assert a.size == (64, 35) and a.mode == "RGB"
    if name not in ("clear_day", "overcast"):          # those drift only slightly or not visibly
        assert ImageChops.difference(a, b).getbbox() is not None
    assert len(set(a.getdata())) > 12                   # not a flat fill


def test_all_conditions_covered_by_samples():
    seen = {sample_state(n).weather.current.condition for n in sample_names()}
    assert seen == set(Condition)


def test_pure_function_of_state_and_t():
    state = sample_state("rain_then_snow")
    assert list(render_state(state, 4.2).getdata()) == list(render_state(state, 4.2).getdata())


def test_draw_scene_stays_inside_rect():
    frame = blank()
    draw_scene(frame, layout.SCENE, sample_state("clear_day"), 0.0)
    outside = Image.new("RGB", frame.size, (1, 2, 3))
    outside.paste(frame.crop((0, layout.SCENE.y, 64, layout.SCENE.y2)), (0, layout.SCENE.y))
    ref = blank()
    ref.paste(frame.crop((0, layout.SCENE.y, 64, layout.SCENE.y2)), (0, layout.SCENE.y))
    assert ImageChops.difference(frame, ref).getbbox() is None
    assert frame.crop((0, layout.SCENE.y2, 64, 64)).getcolors() == [(64 * (64 - layout.SCENE.y2), (1, 2, 3))]


def test_no_weather_draws_dim_neutral_scene():
    img = render_state(AppState(now=NOON), 0.0)
    assert max(max(px) for px in img.getdata()) <= 255
    lum = sum(sum(px) for px in img.crop((0, 0, 40, 12)).getdata()) / (40 * 12 * 3)
    assert lum < 80
    draw_scene(blank(), layout.SCENE, AppState(now=NOON), 0.0)


def test_day_is_brighter_than_night():
    day = render_state(sample_state("clear_day"), 0.0).crop((0, 8, 40, 14))
    night = render_state(sample_state("clear_night"), 0.0).crop((0, 8, 40, 14))
    avg = lambda im: sum(sum(p) for p in im.getdata())  # noqa: E731
    assert avg(day) > 3 * avg(night)


def test_temperature_changes_pixels_top_right():
    s = sample_state("clear_day")
    a = render_state(s, 0.0)
    b = render_state(with_weather(s, temperature=-8.0), 0.0)
    box = ImageChops.difference(a, b).getbbox()
    assert box is not None and box[0] >= 40 and box[3] <= 8


def test_wind_and_cloud_cover_extremes_do_not_crash():
    s = sample_state("partly_cloudy")
    for wind in (0, 15, 80):
        for cover in (0, 50, 100):
            render_state(with_weather(s, wind_speed=wind, cloud_cover=cover), 7.0)
    render_state(with_weather(s, temperature=123.4, code=1234), 1.0)   # unknown WMO code


def test_sunrise_and_sunset_follow_the_clock():
    s = sample_state("clear_day")
    d = s.weather.daily
    assert sun_position(NOON, True, d.sunrise, d.sunset).elev > 0.9
    assert sun_position(d.sunrise, True, d.sunrise, d.sunset).elev == pytest.approx(0, abs=0.01)
    late = sun_position(NOON.replace(hour=23), False, d.sunrise, d.sunset)
    assert late.elev < -0.5
    early = sun_position(NOON.replace(hour=2), False, d.sunrise, d.sunset)
    assert early.elev < -0.5
    # stale snapshot from yesterday still maps onto today's clock
    assert sun_position(NOON + timedelta(days=1), True, d.sunrise, d.sunset).elev > 0.9


def test_ridges_are_fixed_between_runs():
    a, _ = mountains.terrain(40, Condition.CLEAR, 70)
    assert list(a.getdata()) == list(mountains.terrain.__wrapped__(40, Condition.CLEAR, 70)[0].getdata())


def test_lightning_is_deterministic_and_occasional():
    levels = [particles.flash_level(i / 20) for i in range(20 * 60)]
    assert max(levels) > 0.9 and sum(1 for v in levels if v > 0.3) < len(levels) * 0.15
    assert particles.flash_level(12.34) == particles.flash_level(12.34)


def test_cross_fade_blends_for_two_seconds():
    r = SceneRenderer()
    clear, rain = sample_state("clear_day"), sample_state("rain_then_snow")
    r.render(clear, 10.0)
    start = r.render(rain, 10.5)                          # change observed here: starts as the old scene
    mid = r.render(rain, 11.5)
    end = r.render(rain, 12.6)
    assert list(start.getdata()) == list(render_state(clear, 10.5).getdata())   # alpha 0: still the old scene
    target = render_state(rain, 12.6)
    assert list(end.getdata()) == list(target.getdata())
    assert list(mid.getdata()) != list(render_state(rain, 11.5).getdata())


def test_time_jump_resets_instead_of_fading():
    r = SceneRenderer()
    r.render(sample_state("clear_day"), 1.0)
    rain = sample_state("rain_then_snow")
    assert list(r.render(rain, 100.0).getdata()) == list(render_state(rain, 100.0).getdata())


def test_frame_cost_is_low():
    state = sample_state("thunder")
    render_state(state, 0.0)                              # warm the caches
    t0 = time.perf_counter()
    for i in range(30):
        render_state(state, i / 15)
    per_frame = (time.perf_counter() - t0) / 30
    assert per_frame < 0.02


def test_date_overlay_is_top_left_and_readable_day_and_night():
    for name in ("clear_day", "clear_night"):
        img = render_state(sample_state(name), 0.0)
        box = img.crop((0, 0, 36, 8)).point(lambda v: 255 if v > 240 else 0).convert("L").getbbox()
        assert box is not None and box[0] >= 2 and box[1] >= 2, name     # white text, shadow included clear of the edge
        corner = img.crop((1, 3, 2, 4)).getpixel((0, 0))
        assert max(corner) < 200, name                                   # outline darkens the sky


def test_date_changes_with_the_day():
    s = sample_state("clear_day")
    other = s.__class__(now=s.now + timedelta(days=1), weather=s.weather, home=s.home)
    box = ImageChops.difference(render_state(s, 0.0), render_state(other, 0.0)).getbbox()
    assert box is not None and box[0] < 40 and box[3] <= 8


def test_ridge_colours_follow_the_season():
    def avg(doy):
        rgb, mask = mountains.terrain(40, Condition.CLEAR, mountains.season_key(doy))
        px = [p for p, m in zip(rgb.getdata(), mask.getdata()) if m]
        return [sum(c[i] for c in px) / len(px) for i in range(3)]
    summer, october, winter = avg(190), avg(293), avg(15)
    assert summer[1] > summer[0] * 1.15                     # green dominates in summer
    assert october[0] > summer[0] + 20                      # reds and oranges in October
    assert abs(winter[1] - winter[0]) < 25                  # bare browns and greys in winter
