from matrix_display import layout
from matrix_display.app import Compositor
from matrix_display.samples import sample_state
from matrix_display.ui.icons import _PAL
from matrix_display.ui.widgets import draw_next_change


def _icon_colors(name):
    frame = Compositor().render(sample_state(name))      # blank canvas
    draw_next_change(frame, layout.NEXT_CHANGE, sample_state(name), 0.0)
    r = layout.NEXT_CHANGE
    return {frame.getpixel((x, y)) for x in range(r.x, r.x + 20) for y in range(r.y, r.y2)}


def test_clear_night_shows_moon_not_sun():
    colors = _icon_colors("clear_night")
    assert _PAL["M"] in colors and _PAL["Y"] not in colors


def test_clear_day_shows_sun_not_moon():
    colors = _icon_colors("clear_day")
    assert _PAL["Y"] in colors and _PAL["M"] not in colors
