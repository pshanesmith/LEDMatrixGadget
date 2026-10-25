"""Extra AppStates (beyond samples.py) that exercise the widgets' edge cases."""
from dataclasses import replace
from datetime import timedelta

from ..samples import NOON, sample_state
from ..state import AppState, HomeSnapshot


def _home(**kw) -> HomeSnapshot:
    base = dict(fetched_at=NOON, locks_unlocked=0, locks_unavailable=0, lights_on=0,
                gates_open=(), gates_unavailable=())
    base.update(kw)
    return HomeSnapshot(**base)


def variants() -> dict[str, AppState]:
    clear = sample_state("clear_day")
    rain = sample_state("rain_then_snow")
    cold = replace(clear.weather, daily=replace(clear.weather.daily, low=-12.4, high=104.6))
    return {
        "all_clear": replace(clear, home=_home()),
        "rain_then_snow": rain,
        "counts_zero": replace(rain, home=_home()),
        "counts_twelve": replace(rain, home=_home(locks_unlocked=12, lights_on=12)),
        "gate_open": replace(rain, home=_home(locks_unlocked=1, lights_on=3, gates_open=("gate",))),
        "gate_unavailable": replace(rain, home=_home(locks_unavailable=1, gates_unavailable=("gate",))),
        "tier_discount": replace(rain, home=_home(lights_on=3, power_tier="discount")),
        "tier_off_peak": replace(rain, home=_home(lights_on=3, power_tier="off_peak")),
        "tier_on_peak": replace(rain, home=_home(locks_unlocked=1, lights_on=3, power_tier="on_peak")),
        "tier_critical": replace(rain, home=_home(lights_on=3, power_tier="critical_peak")),
        "tier_unavailable": replace(rain, home=_home(lights_on=3, power_tier="unavailable")),
        "tier_counts_twelve": replace(rain, home=_home(locks_unlocked=12, lights_on=12,
                                                       power_tier="on_peak")),
        "tier_gate_open": replace(rain, home=_home(locks_unlocked=1, lights_on=3,
                                                   gates_open=("gate",), power_tier="critical_peak")),
        "weather_none": AppState(now=NOON, weather=None, home=_home(lights_on=2)),
        "home_none": replace(rain, home=None),
        "extreme_temps": replace(clear, weather=cold, home=_home(lights_on=1)),
        "stale_home": replace(rain, home=_home(fetched_at=NOON - timedelta(minutes=10), lights_on=4)),
        "stale_weather": replace(rain, weather=replace(rain.weather, fetched_at=NOON - timedelta(hours=4))),
    }
