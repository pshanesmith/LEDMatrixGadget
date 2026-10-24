from datetime import timedelta

from matrix_display.forecast import daily_low_high, next_change
from matrix_display.samples import NOON, sample_state
from matrix_display.state import Condition


def test_low_high_rounded():
    w = sample_state("clear_day").weather
    assert daily_low_high(w) == (48, 71)


def test_no_change_returns_none():
    assert next_change(sample_state("clear_day").weather, NOON) is None


def test_finds_first_different_hour():
    s = sample_state("rain_then_snow")          # rain now, snow from hour 6
    nc = next_change(s.weather, s.now)
    assert nc.condition is Condition.SNOW
    assert nc.time == NOON + timedelta(hours=6)


def test_change_beyond_24h_ignored():
    s = sample_state("snow")                    # change at hour 9 (within 24h) -> found
    assert next_change(s.weather, s.now).condition is Condition.OVERCAST
    far = sample_state("clear_day").weather
    assert next_change(far, NOON - timedelta(hours=30)) is None


def test_hours_at_or_before_now_are_skipped():
    s = sample_state("rain_then_snow")          # snow hour starts at NOON+6h
    nc = next_change(s.weather, NOON + timedelta(hours=6))
    assert nc.time == NOON + timedelta(hours=7)
