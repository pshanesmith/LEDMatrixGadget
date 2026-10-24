"""Named sample AppStates for previews, golden images and tests (no network needed)."""
from datetime import datetime, timedelta, timezone

from .state import (AppState, CurrentWeather, DailyWeather, HomeSnapshot, HourlyPoint,
                    WeatherSnapshot)

TZ = timezone(timedelta(hours=-4))
NOON = datetime(2026, 10, 23, 12, 0, tzinfo=TZ)

# name -> (current code, is_day, cloud %, wind, hours-from-now, code after the change)
_SCENARIOS = {
    "clear_day": (0, True, 5, 4, None, None),
    "clear_night": (0, False, 0, 3, None, None),
    "partly_cloudy": (2, True, 40, 8, 5, 61),
    "overcast": (3, True, 100, 6, 7, 3),
    "fog": (45, True, 90, 1, 4, 0),
    "rain_then_snow": (63, True, 100, 12, 6, 73),
    "snow": (73, True, 100, 10, 9, 3),
    "thunder": (95, False, 100, 20, 3, 61),
}


def sample_state(name: str = "clear_day", now: datetime = NOON) -> AppState:
    code, is_day, cloud, wind, change_h, change_code = _SCENARIOS[name]
    hourly = []
    for h in range(48):
        c = change_code if change_h is not None and h >= change_h else code
        hourly.append(HourlyPoint(now + timedelta(hours=h), c, 60.0 - h * 0.2, 20))
    weather = WeatherSnapshot(
        fetched_at=now,
        current=CurrentWeather(61.0, code, cloud, wind, is_day),
        hourly=tuple(hourly),
        daily=DailyWeather(48.0, 71.0, now.replace(hour=7, minute=40), now.replace(hour=18, minute=20)),
    )
    home = HomeSnapshot(now, locks_unlocked=1, locks_unavailable=0, lights_on=8,
                        gates_open=(), gates_unavailable=())
    return AppState(now=now, weather=weather, home=home)


def sample_names() -> list[str]:
    return list(_SCENARIOS)
