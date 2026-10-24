import importlib.util
import tomllib
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "set_config", Path(__file__).resolve().parent.parent / "tools" / "set-config.py")
sc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sc)

BROKEN = """# my comment
[location]
latitude = 40.748
longitude = -73.994

[homeassistant]
url = "http://homeassistant:8123"
gates = ["a", "b"]

[display]
pwm_bits = 10

[display]
pwm_bits = 9

[display]
pwm_bits = 10
"""


def test_repairs_duplicates_and_sets_value():
    out = sc.update(BROKEN, "display", "pwm_bits", "9")
    data = tomllib.loads(out)
    assert data["display"] == {"pwm_bits": 9}
    assert data["location"]["latitude"] == 40.748 and data["homeassistant"]["gates"] == ["a", "b"]
    assert out.count("[display]") == 1 and out.startswith("# my comment")


def test_idempotent_and_adds_new_keys_and_sections():
    once = sc.update(BROKEN, "display", "pwm_bits", "10")
    assert sc.update(once, "display", "pwm_bits", "10") == once
    more = sc.update(once, "display", "limit_refresh_hz", "150")
    assert tomllib.loads(more)["display"] == {"pwm_bits": 10, "limit_refresh_hz": 150}
    fresh = sc.update("[location]\nlatitude = 1.0\nlongitude = 2.0\n", "display", "pwm_bits", "9")
    assert tomllib.loads(fresh)["display"]["pwm_bits"] == 9


def test_strings_and_booleans_formatting():
    out = sc.update("", "display", "hardware_mapping", "adafruit-hat")
    assert tomllib.loads(out)["display"]["hardware_mapping"] == "adafruit-hat"
    assert tomllib.loads(sc.update("", "display", "drop_privileges", "false"))["display"]["drop_privileges"] is False


def test_cli_writes_backup_and_refuses_invalid(tmp_path):
    f = tmp_path / "c.toml"
    f.write_text(BROKEN)
    assert sc.main(["x", str(f), "display", "pwm_bits", "10"]) == 0
    assert tomllib.loads(f.read_text())["display"]["pwm_bits"] == 10
    assert (tmp_path / "c.toml.bak").read_text() == BROKEN
