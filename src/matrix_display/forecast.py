"""Pure forecast helpers shared by the scene (Phase 3) and the widgets (Phase 4)."""
from dataclasses import dataclass
from datetime import datetime, timedelta

from .state import Condition, WeatherSnapshot

LOOKAHEAD = timedelta(hours=24)


@dataclass(frozen=True)
class NextChange:
    time: datetime            # start of the first forecast hour with a different condition
    condition: Condition      # the condition it changes to


def daily_low_high(weather: WeatherSnapshot) -> tuple[int, int]:
    """Today's (low, high), rounded to whole degrees in the configured unit."""
    return round(weather.daily.low), round(weather.daily.high)


def next_change(weather: WeatherSnapshot, now: datetime) -> NextChange | None:
    """First forecast hour after `now`, within 24 h, whose condition differs from the current one."""
    current = weather.current.condition
    for point in weather.hourly:
        if point.time <= now:
            continue
        if point.time - now > LOOKAHEAD:
            break
        if point.condition is not current:
            return NextChange(point.time, point.condition)
    return None
