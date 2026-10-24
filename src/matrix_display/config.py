"""Configuration: config.toml plus environment overrides (secrets never live in the file)."""
import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

DEFAULT_HA_URL = "http://homeassistant:8123"
CONFIG_SEARCH = ("config/config.toml", "/etc/matrix/config.toml")
SECRETS_SEARCH = ("secrets.env", "/etc/matrix/secrets.env")


class ConfigError(ValueError):
    pass


@dataclass(frozen=True)
class DisplayConfig:
    rows: int = 64
    cols: int = 64
    chain_length: int = 1
    parallel: int = 1
    hardware_mapping: str = "adafruit-hat-pwm"
    gpio_slowdown: int = 2
    pwm_bits: int = 11
    pwm_lsb_nanoseconds: int = 130
    limit_refresh_hz: int = 0       # 0 = unlimited; a steady cap can remove flicker caused by rate swings
    row_address_type: int = 0
    multiplexing: int = 0
    rgb_sequence: str = "RGB"
    brightness: int = 50
    max_brightness: int = 70
    scene_fps: int = 15
    drop_privileges: bool = True   # rgbmatrix drops root after GPIO setup; env MATRIX_DROP_PRIVILEGES=0 disables


@dataclass(frozen=True)
class LocationConfig:
    latitude: float
    longitude: float
    postal_code: str = ""
    label: str = ""
    units: str = "fahrenheit"


@dataclass(frozen=True)
class WeatherConfig:
    refresh_seconds: int = 900


@dataclass(frozen=True)
class HomeAssistantConfig:
    url: str = DEFAULT_HA_URL
    token: str = field(default="", repr=False)   # from the environment only; never repr'd
    poll_seconds: int = 15
    gates: tuple[str, ...] = ()
    exclude_locks: tuple[str, ...] = ()      # fnmatch globs on entity ids
    exclude_lights: tuple[str, ...] = ()


@dataclass(frozen=True)
class Config:
    display: DisplayConfig = DisplayConfig()
    location: LocationConfig | None = None
    weather: WeatherConfig = WeatherConfig()
    homeassistant: HomeAssistantConfig = HomeAssistantConfig()

    def require_location(self) -> LocationConfig:
        if self.location is None:
            raise ConfigError("[location] is not set; run tools/geocode.py <ZIP> "
                              "or add latitude/longitude to config.toml")
        return self.location


def normalize_url(url: str) -> str:
    """Accept 'host', 'host:port' or a full URL; default scheme http, no trailing slash."""
    url = url.strip()
    if not url:
        raise ConfigError("homeassistant url is empty")
    if "://" not in url:
        url = "http://" + url
    if not url.startswith(("http://", "https://")):
        raise ConfigError(f"homeassistant url must be http(s): {url!r}")
    return url.rstrip("/")


def parse_env_file(path: str | Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            key, _, value = line.partition("=")
            values[key.strip()] = value.strip().strip("'\"")
    return values


def _only(section: dict, cls, name: str) -> dict:
    allowed = set(cls.__dataclass_fields__)
    unknown = set(section) - allowed
    if unknown:
        raise ConfigError(f"unknown key(s) in [{name}]: {', '.join(sorted(unknown))}")
    return section


def build_config(data: dict, environ: dict[str, str] | None = None,
                 secrets: dict[str, str] | None = None) -> Config:
    """Build a Config from parsed TOML. Environment beats secrets file beats TOML."""
    environ = os.environ if environ is None else environ
    secrets = secrets or {}

    def pick(key: str) -> str | None:
        return environ.get(key) or secrets.get(key)

    display_raw = dict(_only(data.get("display", {}), DisplayConfig, "display"))
    drop_env = pick("MATRIX_DROP_PRIVILEGES")
    if drop_env is not None:
        display_raw["drop_privileges"] = drop_env.strip().lower() not in ("0", "false", "no", "off")
    d = DisplayConfig(**display_raw)
    if not 1 <= d.brightness <= 100 or not 1 <= d.max_brightness <= 100:
        raise ConfigError("brightness values must be 1-100")
    d = DisplayConfig(**{**d.__dict__, "brightness": min(d.brightness, d.max_brightness)})

    location = None
    if "location" in data:
        loc = _only(data["location"], LocationConfig, "location")
        if "latitude" not in loc or "longitude" not in loc:
            raise ConfigError("[location] needs latitude and longitude")
        location = LocationConfig(**loc)
        if not (-90 <= location.latitude <= 90 and -180 <= location.longitude <= 180):
            raise ConfigError("[location] latitude/longitude out of range")
        if location.units not in ("fahrenheit", "celsius"):
            raise ConfigError("[location] units must be 'fahrenheit' or 'celsius'")

    w = WeatherConfig(**_only(data.get("weather", {}), WeatherConfig, "weather"))

    ha_raw = dict(_only(data.get("homeassistant", {}), HomeAssistantConfig, "homeassistant"))
    ha_raw.pop("token", None)  # tokens are never read from config.toml
    for key in ("gates", "exclude_locks", "exclude_lights"):
        ha_raw[key] = tuple(ha_raw.get(key, ()))
    ha_raw["url"] = normalize_url(pick("MATRIX_HA_URL") or ha_raw.get("url", DEFAULT_HA_URL))
    ha = HomeAssistantConfig(**ha_raw, token=pick("MATRIX_HA_TOKEN") or "")

    return Config(display=d, location=location, weather=w, homeassistant=ha)


def load_config(path: str | Path | None = None, environ: dict[str, str] | None = None) -> Config:
    """Load config.toml (explicit path, else first existing search path) and secrets.env."""
    candidates = [path] if path else list(CONFIG_SEARCH)
    data: dict = {}
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            with open(candidate, "rb") as fh:
                data = tomllib.load(fh)
            break
    else:
        if path:
            raise ConfigError(f"config file not found: {path}")
    secrets: dict[str, str] = {}
    for candidate in SECRETS_SEARCH:
        if Path(candidate).is_file():
            secrets = parse_env_file(candidate)
            break
    return build_config(data, environ=environ, secrets=secrets)
