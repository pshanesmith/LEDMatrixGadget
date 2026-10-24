import importlib.util
import json
import tomllib
from pathlib import Path

import pytest

from matrix_display.config import load_config
from matrix_display.data import geocode as g

FIX = Path(__file__).parent / "fixtures"


def load(name):
    return json.loads((FIX / name).read_text())


def fake_get(responses):
    """responses: substring of url -> payload or exception."""
    calls = []

    def get(url, *a, **kw):
        calls.append(url)
        for key, val in responses.items():
            if key in url:
                if isinstance(val, Exception):
                    raise val
                return val
        raise AssertionError(url)
    get.calls = calls
    return get


class NoThrottle:
    def wait(self):
        pass


def test_open_meteo_filters_other_countries():
    # the unfiltered recorded response also contains Spanish and French matches
    get = fake_get({"geocoding-api": load("geocode_openmeteo_10001.json")})
    places = g.search_open_meteo("10001", "US", get)
    assert [p.label for p in places] == ["New York, NY"]
    assert "countryCode=US" in get.calls[0]


def test_open_meteo_other_country():
    places = g.search_open_meteo("10001", "ES", fake_get({"geocoding-api": load("geocode_openmeteo_10001.json")}))
    assert places[0].country_code == "ES"


def test_nominatim_structured_query_and_label():
    get = fake_get({"nominatim": load("geocode_nominatim_10001_us.json")})
    places = g.search_nominatim("10001", "US", get, NoThrottle())
    assert places[0].label == "New York, NY"
    assert places[0].rounded() == (40.748, -73.994)
    assert "postalcode=10001" in get.calls[0] and "country=US" in get.calls[0]


def test_geocode_prefers_open_meteo():
    get = fake_get({"geocoding-api": load("geocode_openmeteo_10001_us.json")})
    assert g.geocode("10001", "US", get, NoThrottle())[0].provider == "open-meteo"
    assert len(get.calls) == 1


def test_geocode_falls_back_to_nominatim():
    for first in ({"results": []}, OSError("down"), {}):
        get = fake_get({"geocoding-api": first, "nominatim": load("geocode_nominatim_10001_us.json")})
        assert g.geocode("10001", "US", get, NoThrottle())[0].provider == "nominatim"


def test_offline_failure():
    get = fake_get({"geocoding-api": OSError("offline"), "nominatim": OSError("offline")})
    with pytest.raises(g.GeocodeError, match="offline"):
        g.geocode("10001", "US", get, NoThrottle())


def test_no_match_anywhere():
    get = fake_get({"geocoding-api": {}, "nominatim": []})
    with pytest.raises(g.GeocodeError, match="no match"):
        g.geocode("00000", "US", get, NoThrottle())


def test_rejects_junk_postal_code():
    with pytest.raises(g.GeocodeError):
        g.geocode("10001&evil=1", "US", fake_get({}), NoThrottle())


def test_nominatim_throttle_waits_one_second():
    now = [100.0]
    slept = []
    t = g._Throttle(1.0, clock=lambda: now[0], sleep=lambda s: (slept.append(s), now.__setitem__(0, now[0] + s)))
    t.wait()
    now[0] += 0.25
    t.wait()
    assert slept == [pytest.approx(0.75)]


CONFIG = """\
# Local config
[location]
postal_code = "00000"   # old
latitude = 1.0
longitude = 2.0
units = "celsius"

# Home Assistant
[homeassistant]
url = "http://homeassistant:8123"
"""

PLACE = g.Place("New York, NY", 40.74848, -73.99389, "US", "test")


def test_update_replaces_keys_and_keeps_everything_else(tmp_path):
    p = tmp_path / "config.toml"
    p.write_text(CONFIG)
    g.write_location(p, PLACE, "10001")
    text = p.read_text()
    assert "# Local config" in text and "# Home Assistant" in text
    assert 'url = "http://homeassistant:8123"' in text
    data = tomllib.loads(text)
    assert data["location"] == {"postal_code": "10001", "latitude": 40.748, "longitude": -73.994,
                                "label": "New York, NY", "units": "celsius"}
    assert "# old" in text


def test_update_adds_missing_table_and_keys(tmp_path):
    p = tmp_path / "config.toml"
    p.write_text('[homeassistant]\nurl = "x"\n')
    g.write_location(p, PLACE, "10001", units="fahrenheit")
    data = tomllib.loads(p.read_text())
    assert data["location"]["units"] == "fahrenheit" and data["homeassistant"]["url"] == "x"
    p2 = tmp_path / "new.toml"
    g.write_location(p2, PLACE, "10001")
    assert tomllib.loads(p2.read_text())["location"]["units"] == "fahrenheit"


def test_written_config_loads_with_project_loader(tmp_path):
    p = tmp_path / "config.toml"
    p.write_text(CONFIG)
    g.write_location(p, PLACE, "10001")
    loc = load_config(p, environ={}).require_location()
    assert (loc.latitude, loc.longitude, loc.postal_code) == (40.748, -73.994, "10001")


def test_label_with_quotes_is_escaped(tmp_path):
    p = tmp_path / "c.toml"
    g.write_location(p, g.Place('Say "hi", NC', 1.0, 2.0, "US", "t"), "12345")
    assert tomllib.loads(p.read_text())["location"]["label"] == 'Say "hi", NC'


def _tool():
    spec = importlib.util.spec_from_file_location("geocode_tool", Path(__file__).parent.parent / "tools" / "geocode.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def test_tool_confirm_and_cancel(tmp_path, monkeypatch, capsys):
    tool = _tool()
    monkeypatch.setattr(tool, "geocode", lambda zip_, country: [PLACE])
    cfg = tmp_path / "c.toml"
    cfg.write_text(CONFIG)
    assert tool.main(["10001", "--config", str(cfg)], ask=lambda prompt: "n") == 2
    assert 'postal_code = "00000"' in cfg.read_text()
    assert tool.main(["10001", "--config", str(cfg)], ask=lambda prompt: "") == 0
    assert tomllib.loads(cfg.read_text())["location"]["postal_code"] == "10001"


def test_tool_offline_writes_nothing(tmp_path, monkeypatch):
    tool = _tool()

    def offline(zip_, country):
        raise g.GeocodeError("offline")
    monkeypatch.setattr(tool, "geocode", offline)
    cfg = tmp_path / "c.toml"
    cfg.write_text(CONFIG)
    assert tool.main(["10001", "--config", str(cfg)]) == 1
    assert cfg.read_text() == CONFIG


def test_tool_manual_override(tmp_path):
    cfg = tmp_path / "c.toml"
    cfg.write_text(CONFIG)
    assert _tool().main(["--lat", "40.748", "--lon", "-73.994", "--label", "Home", "--config", str(cfg)]) == 0
    assert tomllib.loads(cfg.read_text())["location"]["label"] == "Home"
