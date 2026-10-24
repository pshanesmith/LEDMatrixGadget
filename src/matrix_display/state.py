"""Shared, immutable data contracts between data fetchers, scene and widgets.

All datetimes are timezone-aware, in the location's local time.
"""
from dataclasses import dataclass
from datetime import datetime
from enum import Enum


class Condition(str, Enum):
    CLEAR = "clear"
    PARTLY_CLOUDY = "partly_cloudy"
    OVERCAST = "overcast"
    FOG = "fog"
    RAIN = "rain"
    SNOW = "snow"
    THUNDER = "thunder"


def condition_from_wmo(code: int) -> Condition:
    """Map an Open-Meteo WMO weather code to a display category (DESIGN.md section 4).

    Unknown codes fall back to OVERCAST rather than raising, so a new code from the
    API can never crash the display.
    """
    if code in (0, 1):
        return Condition.CLEAR
    if code == 2:
        return Condition.PARTLY_CLOUDY
    if code == 3:
        return Condition.OVERCAST
    if code in (45, 48):
        return Condition.FOG
    if 51 <= code <= 67 or code in (80, 81, 82):
        return Condition.RAIN
    if 71 <= code <= 77 or code in (85, 86):
        return Condition.SNOW
    if code in (95, 96, 99):
        return Condition.THUNDER
    return Condition.OVERCAST


@dataclass(frozen=True)
class CurrentWeather:
    temperature: float          # in the configured unit
    code: int                   # WMO code
    cloud_cover: int            # percent
    wind_speed: float
    is_day: bool

    @property
    def condition(self) -> Condition:
        return condition_from_wmo(self.code)


@dataclass(frozen=True)
class HourlyPoint:
    time: datetime
    code: int
    temperature: float
    precipitation_probability: int

    @property
    def condition(self) -> Condition:
        return condition_from_wmo(self.code)


@dataclass(frozen=True)
class DailyWeather:
    low: float
    high: float
    sunrise: datetime
    sunset: datetime


@dataclass(frozen=True)
class WeatherSnapshot:
    fetched_at: datetime
    current: CurrentWeather
    hourly: tuple[HourlyPoint, ...]
    daily: DailyWeather


@dataclass(frozen=True)
class HomeSnapshot:
    fetched_at: datetime
    locks_unlocked: int                 # unlocked, jammed, or in motion: needs attention
    locks_unavailable: int              # state unknown; shown with a marker, not counted
    lights_on: int
    gates_open: tuple[str, ...]         # entity ids of gates that are open
    gates_unavailable: tuple[str, ...]  # gates whose state is unknown


@dataclass(frozen=True)
class AppState:
    now: datetime
    weather: WeatherSnapshot | None = None
    home: HomeSnapshot | None = None
