"""Colour helpers, time-of-day maths and sky palette for the weather scene."""
import math
from dataclasses import dataclass
from datetime import datetime, timedelta

from ..state import Condition

W, H = 64, 35
RGB = tuple[int, int, int]

# How much each condition greys the sky and terrain (0 = untouched).
GRAY_MIX = {
    Condition.CLEAR: 0.0, Condition.PARTLY_CLOUDY: 0.12, Condition.OVERCAST: 0.7,
    Condition.FOG: 0.6, Condition.RAIN: 0.75, Condition.SNOW: 0.65, Condition.THUNDER: 0.85,
    None: 0.6,
}


def clamp(x: float, lo: float = 0.0, hi: float = 1.0) -> float:
    return lo if x < lo else hi if x > hi else x


def lerp(a: RGB, b: RGB, t: float) -> RGB:
    t = clamp(t)
    return (round(a[0] + (b[0] - a[0]) * t), round(a[1] + (b[1] - a[1]) * t),
            round(a[2] + (b[2] - a[2]) * t))


def scale(c: RGB, f: float) -> RGB:
    return (min(255, round(c[0] * f)), min(255, round(c[1] * f)), min(255, round(c[2] * f)))


def gray_of(c: RGB) -> RGB:
    g = round(0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2])
    return (g, g, g)


@dataclass(frozen=True)
class Sun:
    elev: float            # -1..1; 0 on the horizon, +1 noon, -1 midnight
    progress: float        # 0..1 along the sun's arc (day) or the moon's arc (night)


def sun_position(now: datetime, is_day: bool, sunrise: datetime, sunset: datetime) -> Sun:
    """Where the sun (day) or moon (night) is. `is_day` from the API wins when it clearly
    disagrees with the clock (sample data, stale cache), except within 30 min of a boundary."""
    now = now.astimezone(sunrise.tzinfo) if now.tzinfo else now.replace(tzinfo=sunrise.tzinfo)
    shift = timedelta(days=(now.date() - sunrise.date()).days)
    rise, sset = sunrise + shift, sunset + shift
    day = timedelta(days=1)
    if rise <= now <= sset:
        p = (now - rise) / (sset - rise)
        clock = Sun(math.sin(math.pi * p), p)
        clock_day = True
    else:
        if now < rise:
            start, end = sset - day, rise
        else:
            start, end = sset, rise + day
        q = (now - start) / (end - start)
        clock = Sun(-math.sin(math.pi * clamp(q)), clamp(q))
        clock_day = False
    near = min(abs(now - rise), abs(now - sset)) < timedelta(minutes=30)
    if clock_day != is_day and not near:
        return Sun(1.0, 0.5) if is_day else Sun(-1.0, 0.5)
    return clock


def light_level(elev: float) -> float:
    """0 in full night, 1 in full day, smooth through twilight."""
    return clamp((elev + 0.08) / 0.45)


def twilight(elev: float) -> float:
    """1 right at sunrise/sunset, fading to 0 about 0.2 away in elevation."""
    return clamp(1.0 - abs(elev - 0.02) / 0.22)


# (elevation, sky top, sky horizon)
_SKY = (
    (-0.40, (4, 6, 24), (10, 14, 40)),
    (-0.12, (14, 14, 50), (62, 36, 84)),
    (0.00, (40, 50, 110), (255, 140, 70)),
    (0.12, (70, 110, 190), (255, 190, 130)),
    (0.35, (56, 124, 214), (150, 200, 245)),
)


def sky_colors(elev: float) -> tuple[RGB, RGB]:
    if elev <= _SKY[0][0]:
        return _SKY[0][1], _SKY[0][2]
    for (e0, t0, b0), (e1, t1, b1) in zip(_SKY, _SKY[1:]):
        if elev <= e1:
            f = (elev - e0) / (e1 - e0)
            return lerp(t0, t1, f), lerp(b0, b1, f)
    return _SKY[-1][1], _SKY[-1][2]
