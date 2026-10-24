"""Open-Meteo client and background fetcher (DESIGN.md section 5).

`parse_open_meteo` turns the JSON response into a `state.WeatherSnapshot` with
timezone-aware datetimes. `WeatherFetcher` refreshes it on a thread, keeps the last
good snapshot (also on disk, so a boot without network still shows something) and
backs off exponentially, capped at 5 minutes, after failures.
"""
import json
import logging
import threading
import urllib.parse
import urllib.request
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from pathlib import Path

from ..config import LocationConfig, WeatherConfig
from ..state import CurrentWeather, DailyWeather, HourlyPoint, WeatherSnapshot

log = logging.getLogger(__name__)

API_URL = "https://api.open-meteo.com/v1/forecast"
USER_AGENT = "matrix-display/0.1 (home LED matrix; +https://open-meteo.com)"
TIMEOUT_SECONDS = 20
MAX_BACKOFF_SECONDS = 300
DEFAULT_CACHE = "weather-cache.json"


class WeatherError(RuntimeError):
    pass


def build_url(location: LocationConfig) -> str:
    query = {
        "latitude": f"{location.latitude:.3f}",
        "longitude": f"{location.longitude:.3f}",
        "current": "temperature_2m,weather_code,cloud_cover,wind_speed_10m,is_day",
        "hourly": "weather_code,temperature_2m,precipitation_probability",
        "daily": "temperature_2m_max,temperature_2m_min,sunrise,sunset",
        "temperature_unit": location.units,
        "timezone": "auto",
        "forecast_days": "2",
    }
    return API_URL + "?" + urllib.parse.urlencode(query, safe=",")


def fetch_json(url: str, timeout: float = TIMEOUT_SECONDS) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(req, timeout=timeout) as resp:  # noqa: S310 (fixed https URL)
        return json.loads(resp.read().decode("utf-8"))


def _local(text: str, tz: timezone) -> datetime:
    """Open-Meteo times are local wall-clock strings without an offset."""
    return datetime.fromisoformat(text).replace(tzinfo=tz)


def parse_open_meteo(data: dict, fetched_at: datetime | None = None) -> WeatherSnapshot:
    """Convert an Open-Meteo response to a snapshot. Raises WeatherError on a bad payload."""
    try:
        tz = timezone(timedelta(seconds=int(data["utc_offset_seconds"])))
        cur = data["current"]
        current = CurrentWeather(
            temperature=float(cur["temperature_2m"]),
            code=int(cur["weather_code"]),
            cloud_cover=int(cur["cloud_cover"]),
            wind_speed=float(cur["wind_speed_10m"]),
            is_day=bool(cur["is_day"]),
        )
        hourly = data["hourly"]
        points = tuple(
            HourlyPoint(
                time=_local(t, tz),
                code=int(code),
                temperature=float(temp),
                precipitation_probability=int(prob or 0),
            )
            for t, code, temp, prob in zip(
                hourly["time"], hourly["weather_code"], hourly["temperature_2m"],
                hourly["precipitation_probability"])
            if code is not None and temp is not None
        )
        daily = data["daily"]
        # Day 0 is "today" in the location's timezone.
        today = DailyWeather(
            low=float(daily["temperature_2m_min"][0]),
            high=float(daily["temperature_2m_max"][0]),
            sunrise=_local(daily["sunrise"][0], tz),
            sunset=_local(daily["sunset"][0], tz),
        )
    except (KeyError, IndexError, TypeError, ValueError) as exc:
        raise WeatherError(f"unexpected Open-Meteo response: {exc!r}") from exc
    return WeatherSnapshot(
        fetched_at=fetched_at or datetime.now(tz),
        current=current, hourly=points, daily=today)


def snapshot_to_json(snapshot: WeatherSnapshot) -> dict:
    return {
        "fetched_at": snapshot.fetched_at.isoformat(),
        "current": {
            "temperature": snapshot.current.temperature, "code": snapshot.current.code,
            "cloud_cover": snapshot.current.cloud_cover,
            "wind_speed": snapshot.current.wind_speed, "is_day": snapshot.current.is_day,
        },
        "hourly": [
            {"time": p.time.isoformat(), "code": p.code, "temperature": p.temperature,
             "precipitation_probability": p.precipitation_probability}
            for p in snapshot.hourly],
        "daily": {
            "low": snapshot.daily.low, "high": snapshot.daily.high,
            "sunrise": snapshot.daily.sunrise.isoformat(),
            "sunset": snapshot.daily.sunset.isoformat(),
        },
    }


def snapshot_from_json(data: dict) -> WeatherSnapshot:
    try:
        return WeatherSnapshot(
            fetched_at=datetime.fromisoformat(data["fetched_at"]),
            current=CurrentWeather(**data["current"]),
            hourly=tuple(
                HourlyPoint(datetime.fromisoformat(p["time"]), p["code"], p["temperature"],
                            p["precipitation_probability"])
                for p in data["hourly"]),
            daily=DailyWeather(
                data["daily"]["low"], data["daily"]["high"],
                datetime.fromisoformat(data["daily"]["sunrise"]),
                datetime.fromisoformat(data["daily"]["sunset"])),
        )
    except (KeyError, TypeError, ValueError) as exc:
        raise WeatherError(f"bad weather cache: {exc!r}") from exc


def fetch_weather(location: LocationConfig, fetcher: Callable[[str], dict] = fetch_json
                  ) -> WeatherSnapshot:
    try:
        data = fetcher(build_url(location))
    except WeatherError:
        raise
    except Exception as exc:  # network, HTTP, JSON: all mean "keep the old data"
        raise WeatherError(f"weather request failed: {exc!r}") from exc
    return parse_open_meteo(data)


def backoff_seconds(failures: int, base: float = 15.0, cap: float = MAX_BACKOFF_SECONDS) -> float:
    """Delay before retry number `failures` (1-based): 15, 30, 60, 120, 240, 300, 300..."""
    return min(cap, base * 2 ** max(0, failures - 1))


class WeatherFetcher:
    """Refreshes weather on a daemon thread; `latest()` is safe to call from any thread."""

    def __init__(self, location: LocationConfig, config: WeatherConfig | None = None,
                 cache_path: str | Path | None = DEFAULT_CACHE,
                 fetch: Callable[[LocationConfig], WeatherSnapshot] | None = None):
        self._location = location
        self._refresh = (config or WeatherConfig()).refresh_seconds
        self._cache_path = Path(cache_path) if cache_path else None
        self._fetch = fetch or fetch_weather
        self._lock = threading.Lock()
        self._latest: WeatherSnapshot | None = None
        self._failures = 0
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._load_cache()

    def latest(self) -> WeatherSnapshot | None:
        with self._lock:
            return self._latest

    @property
    def failures(self) -> int:
        with self._lock:
            return self._failures

    def refresh_once(self) -> bool:
        """Fetch now. Returns True on success; on failure the last good snapshot is kept."""
        try:
            snapshot = self._fetch(self._location)
        except Exception as exc:
            with self._lock:
                self._failures += 1
            log.warning("weather refresh failed (%d in a row): %s", self.failures, exc)
            return False
        with self._lock:
            self._latest = snapshot
            self._failures = 0
        self._save_cache(snapshot)
        return True

    def next_delay(self) -> float:
        failures = self.failures
        return self._refresh if failures == 0 else backoff_seconds(failures)

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, name="weather", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=5)

    def _run(self) -> None:
        while not self._stop.is_set():
            self.refresh_once()
            self._stop.wait(self.next_delay())

    def _load_cache(self) -> None:
        if not self._cache_path or not self._cache_path.is_file():
            return
        try:
            snapshot = snapshot_from_json(json.loads(self._cache_path.read_text("utf-8")))
        except (OSError, ValueError, WeatherError) as exc:
            log.warning("ignoring unreadable weather cache %s: %s", self._cache_path, exc)
            return
        self._latest = snapshot

    def _save_cache(self, snapshot: WeatherSnapshot) -> None:
        if not self._cache_path:
            return
        tmp = self._cache_path.with_suffix(".tmp")
        try:
            tmp.write_text(json.dumps(snapshot_to_json(snapshot)), encoding="utf-8")
            tmp.replace(self._cache_path)  # atomic: a crash never leaves half a file
        except OSError as exc:
            log.warning("could not write weather cache: %s", exc)
