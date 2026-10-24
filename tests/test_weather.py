import json
import time
from datetime import timedelta
from pathlib import Path

import pytest

from matrix_display.config import LocationConfig, WeatherConfig
from matrix_display.data import weather as w
from matrix_display.forecast import daily_low_high, next_change

FIXTURE = Path(__file__).parent / "fixtures" / "weather_open_meteo_recorded.json"
LOC = LocationConfig(latitude=40.748, longitude=-73.994, postal_code="10001", units="fahrenheit")


@pytest.fixture
def data():
    return json.loads(FIXTURE.read_text())


def test_build_url_uses_location_and_units():
    url = w.build_url(LOC)
    assert "latitude=40.748" in url and "longitude=-73.994" in url
    assert "temperature_unit=fahrenheit" in url and "timezone=auto" in url
    assert "forecast_days=2" in url and "is_day" in url
    assert "celsius" in w.build_url(LocationConfig(1, 2, units="celsius"))


def test_parse_recorded_response(data):
    snap = w.parse_open_meteo(data)
    assert snap.current.temperature == data["current"]["temperature_2m"]
    assert snap.current.is_day is bool(data["current"]["is_day"])
    assert len(snap.hourly) == 48
    assert snap.hourly[0].time.utcoffset() == timedelta(seconds=data["utc_offset_seconds"])
    assert snap.daily.sunrise < snap.daily.sunset
    assert snap.daily.sunrise.tzinfo is not None
    low, high = daily_low_high(snap)
    assert low == round(data["daily"]["temperature_2m_min"][0])
    assert high == round(data["daily"]["temperature_2m_max"][0])


def test_next_change_runs_on_parsed_snapshot(data):
    snap = w.parse_open_meteo(data)
    result = next_change(snap, snap.hourly[0].time)
    assert result is None or result.time > snap.hourly[0].time


def test_parse_rejects_bad_payload(data):
    del data["current"]
    with pytest.raises(w.WeatherError):
        w.parse_open_meteo(data)
    with pytest.raises(w.WeatherError):
        w.parse_open_meteo({})


def test_parse_skips_null_hours(data):
    data["hourly"]["weather_code"][3] = None
    assert len(w.parse_open_meteo(data).hourly) == 47


def test_json_roundtrip(data):
    snap = w.parse_open_meteo(data)
    assert w.snapshot_from_json(json.loads(json.dumps(w.snapshot_to_json(snap)))) == snap


def test_backoff_doubles_and_caps():
    assert [w.backoff_seconds(n) for n in (1, 2, 3, 4, 5, 6, 9)] == [15, 30, 60, 120, 240, 300, 300]


def test_fetch_weather_wraps_network_errors():
    def boom(url):
        raise OSError("offline")
    with pytest.raises(w.WeatherError):
        w.fetch_weather(LOC, fetcher=boom)


def test_fetch_weather_with_fake_transport(data):
    snap = w.fetch_weather(LOC, fetcher=lambda url: data)
    assert snap.current.code == data["current"]["weather_code"]


def test_fetcher_keeps_last_good_and_backs_off(tmp_path, data):
    cache = tmp_path / "cache.json"
    good = w.parse_open_meteo(data)
    results = [good, OSError("offline"), OSError("offline")]

    def fake(loc):
        r = results.pop(0)
        if isinstance(r, Exception):
            raise r
        return r

    f = w.WeatherFetcher(LOC, WeatherConfig(refresh_seconds=900), cache, fetch=fake)
    assert f.latest() is None
    assert f.refresh_once() and f.latest() == good and f.next_delay() == 900
    assert not f.refresh_once()
    assert f.latest() == good and f.failures == 1 and f.next_delay() == 15
    assert not f.refresh_once() and f.next_delay() == 30

    # a restart while offline loads the cached snapshot
    offline = w.WeatherFetcher(LOC, cache_path=cache, fetch=lambda loc: (_ for _ in ()).throw(OSError()))
    assert offline.latest() == good


def test_corrupt_cache_is_ignored(tmp_path):
    cache = tmp_path / "cache.json"
    cache.write_text("{not json")
    assert w.WeatherFetcher(LOC, cache_path=cache, fetch=lambda loc: None).latest() is None


def test_background_thread_publishes(tmp_path, data):
    good = w.parse_open_meteo(data)
    f = w.WeatherFetcher(LOC, WeatherConfig(refresh_seconds=3600), tmp_path / "c.json",
                         fetch=lambda loc: good)
    f.start()
    try:
        deadline = time.time() + 5
        while f.latest() is None and time.time() < deadline:
            time.sleep(0.01)
        assert f.latest() == good
    finally:
        f.stop()
