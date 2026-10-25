"""Text and icon widgets. Each is a RegionDrawer: (frame, rect, state, t) -> None."""
from datetime import timedelta

from .. import forecast
from ..layout import Rect
from ..state import AppState, Condition
from . import icons
from .fonts import HEIGHT, draw_text, text_width

LOW_COLOR = (80, 150, 255)
HIGH_COLOR = (255, 140, 40)
LABEL_COLOR = (200, 200, 210)
DIM = (60, 60, 70)
RED = (255, 40, 30)
AMBER = (255, 170, 0)
GREEN = (30, 220, 70)
BULB_ON = (255, 215, 90)
LOCK_OK = (110, 130, 150)

HOME_STALE = timedelta(minutes=5)
WEATHER_STALE = timedelta(hours=3)

_NAMES = {
    Condition.CLEAR: "CLEAR",
    Condition.PARTLY_CLOUDY: "PARTLY",
    Condition.OVERCAST: "CLOUDY",
    Condition.FOG: "FOG",
    Condition.RAIN: "RAIN",
    Condition.SNOW: "SNOW",
    Condition.THUNDER: "STORM",
}


def _text_y(rect: Rect) -> int:
    return rect.y + (rect.h - HEIGHT) // 2


def _center_x(rect: Rect, width: int, cx: int | None = None) -> int:
    cx = rect.x + rect.w // 2 if cx is None else cx
    return max(rect.x, cx - width // 2)


def hour_label(dt) -> str:
    h = dt.hour % 12 or 12
    return f"{h}{'A' if dt.hour < 12 else 'P'}"


def draw_temps(frame, rect, state: AppState, t: float) -> None:
    y = _text_y(rect)
    if state.weather is None:
        for cx in (rect.x + 16, rect.x + 48):
            draw_text(frame, _center_x(rect, text_width("--"), cx), y, "--", DIM)
        return
    low, high = forecast.daily_low_high(state.weather)
    for cx, arrow, val, color in ((rect.x + 16, "v", low, LOW_COLOR),
                                  (rect.x + 48, "^", high, HIGH_COLOR)):
        text = f"{arrow}{val}"
        draw_text(frame, _center_x(rect, text_width(text), cx), y, text, color)


def _is_night(weather, when) -> bool:
    """True when `when` falls before today's sunrise or after today's sunset (local time of day)."""
    tod = when.astimezone(weather.daily.sunrise.tzinfo).time()
    return tod < weather.daily.sunrise.time() or tod >= weather.daily.sunset.time()


def draw_next_change(frame, rect, state: AppState, t: float) -> None:
    y = _text_y(rect)
    if state.weather is None:
        draw_text(frame, _center_x(rect, text_width("--")), y, "--", DIM)
        return
    change = forecast.next_change(state.weather, state.now)
    if change is None:
        cond, text, color = state.weather.current.condition, "STEADY 24H", LABEL_COLOR
        night = not state.weather.current.is_day
    else:
        cond = change.condition
        text = f"{_NAMES[cond]} {hour_label(change.time)}"
        color = LABEL_COLOR
        night = _is_night(state.weather, change.time)
    width = 9 + 3 + text_width(text)
    x = _center_x(rect, width)
    icons.draw_weather_icon(frame, x, rect.y + (rect.h - 9) // 2, cond, night)
    draw_text(frame, x + 12, y, text, color)


def _count(n: int) -> str:
    return str(min(n, 99))


def _icon_count(frame, cell_x, rect, icon, icon_w, icon_color, text, text_color):
    width = icon_w + 2 + text_width(text)
    x = cell_x + max(0, (21 - width) // 2)
    icons.blit(frame, x, rect.y + 1, icon, icon_color)
    draw_text(frame, x + icon_w + 2, rect.y + 3, text, text_color)


def draw_home(frame, rect, state: AppState, t: float) -> None:
    home = state.home
    c0, c1, c2 = rect.x, rect.x + 21, rect.x + 42
    if home is None:
        for cx in (c0, c1, c2):
            draw_text(frame, cx + 9, rect.y + 3, "--", DIM)
    else:
        # locks
        n = home.locks_unlocked
        color = AMBER if n else DIM
        if n:
            color = RED
        text = _count(n)
        if home.locks_unavailable:
            text += "?"
        icon_color = color if n else LOCK_OK
        _icon_count(frame, c0, rect, icons.LOCK[n > 0], 7, icon_color, text,
                    AMBER if (not n and home.locks_unavailable) else color)
        # lights
        n = home.lights_on
        _icon_count(frame, c1, rect, icons.BULB, 7, BULB_ON if n else DIM,
                    _count(n), BULB_ON if n else DIM)
        # gate
        gx = c2 + (21 - 9) // 2
        if home.gates_open:
            on = int(t) % 2 == 0
            icons.blit(frame, gx, rect.y + 1, icons.GATE[True], RED if on else (70, 12, 10))
        elif home.gates_unavailable:
            icons.blit(frame, gx, rect.y + 1, icons.GATE[False], AMBER)
        else:
            icons.blit(frame, gx, rect.y + 1, icons.GATE[False], GREEN)
    _staleness_dot(frame, rect, state)


def _staleness_dot(frame, rect, state: AppState) -> None:
    now = state.now
    if state.weather is not None and now - state.weather.fetched_at > WEATHER_STALE:
        color = RED
    elif state.home is not None and now - state.home.fetched_at > HOME_STALE:
        color = AMBER
    elif state.weather is None or state.home is None:
        color = (90, 90, 90)
    else:
        return
    frame.putpixel((rect.x2 - 1, rect.y2 - 1), color)


WIDGETS = {
    "temps": draw_temps,
    "next_change": draw_next_change,
    "home": draw_home,
}
