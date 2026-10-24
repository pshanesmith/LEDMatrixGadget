"""One-time ZIP / postal code geocoding (DESIGN.md section 5). Never used at runtime.

Open-Meteo geocoding first, OpenStreetMap Nominatim as fallback. Results are always
filtered by country: a bare "10001" also matches places in Spain and France.
"""
import json
import re
import time
import os
import urllib.parse
import urllib.request
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

# Identifies the application (Nominatim policy) without naming a person; optionally set
# MATRIX_CONTACT to append your own contact if you want one.
USER_AGENT = "LEDMatrixGadget/0.1 (home LED matrix; one-time ZIP lookup)" + (
    " " + os.environ["MATRIX_CONTACT"] if os.environ.get("MATRIX_CONTACT") else "")
OPEN_METEO_URL = "https://geocoding-api.open-meteo.com/v1/search"
NOMINATIM_URL = "https://nominatim.openstreetmap.org/search"
NOMINATIM_MIN_INTERVAL = 1.0   # usage policy: at most one request per second

US_STATES = {
    "Alabama": "AL", "Alaska": "AK", "Arizona": "AZ", "Arkansas": "AR", "California": "CA",
    "Colorado": "CO", "Connecticut": "CT", "Delaware": "DE", "District of Columbia": "DC",
    "Florida": "FL", "Georgia": "GA", "Hawaii": "HI", "Idaho": "ID", "Illinois": "IL",
    "Indiana": "IN", "Iowa": "IA", "Kansas": "KS", "Kentucky": "KY", "Louisiana": "LA",
    "Maine": "ME", "Maryland": "MD", "Massachusetts": "MA", "Michigan": "MI",
    "Minnesota": "MN", "Mississippi": "MS", "Missouri": "MO", "Montana": "MT",
    "Nebraska": "NE", "Nevada": "NV", "New Hampshire": "NH", "New Jersey": "NJ",
    "New Mexico": "NM", "New York": "NY", "North Carolina": "NC", "North Dakota": "ND",
    "Ohio": "OH", "Oklahoma": "OK", "Oregon": "OR", "Pennsylvania": "PA",
    "Rhode Island": "RI", "South Carolina": "SC", "South Dakota": "SD", "Tennessee": "TN",
    "Texas": "TX", "Utah": "UT", "Vermont": "VT", "Virginia": "VA", "Washington": "WA",
    "West Virginia": "WV", "Wisconsin": "WI", "Wyoming": "WY", "Puerto Rico": "PR",
}


class GeocodeError(RuntimeError):
    pass


@dataclass(frozen=True)
class Place:
    label: str
    latitude: float
    longitude: float
    country_code: str
    provider: str

    def rounded(self) -> tuple[float, float]:
        return round(self.latitude, 3), round(self.longitude, 3)


Fetcher = Callable[[str, dict[str, str]], object]


def http_get_json(url: str, headers: dict[str, str] | None = None, timeout: float = 20):
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT, **(headers or {})})
    with urllib.request.urlopen(req, timeout=timeout) as resp:  # noqa: S310 (fixed https URLs)
        return json.loads(resp.read().decode("utf-8"))


def _state_label(state: str, country_code: str) -> str:
    if country_code.upper() == "US":
        return US_STATES.get(state, state)
    return state


def _join(*parts: str) -> str:
    return ", ".join(p for p in parts if p)


def search_open_meteo(postal_code: str, country: str, get: Callable = http_get_json) -> list[Place]:
    params = {"name": postal_code, "count": "10", "language": "en", "format": "json",
              "countryCode": country.upper()}
    data = get(OPEN_METEO_URL + "?" + urllib.parse.urlencode(params))
    places = []
    for r in (data or {}).get("results") or []:
        code = (r.get("country_code") or "").upper()
        if code != country.upper():     # belt and braces: the API filter is not trusted alone
            continue
        # Prefer matches that actually list this postal code.
        listed = postal_code in (r.get("postcodes") or [])
        label = _join(r.get("name", ""), _state_label(r.get("admin1", ""), code))
        places.append((not listed, Place(label, float(r["latitude"]), float(r["longitude"]),
                                         code, "open-meteo")))
    places.sort(key=lambda p: p[0])
    return [p for _, p in places]


class _Throttle:
    def __init__(self, interval: float, clock=time.monotonic, sleep=time.sleep):
        self.interval, self._clock, self._sleep, self._last = interval, clock, sleep, None

    def wait(self) -> None:
        if self._last is not None:
            remaining = self.interval - (self._clock() - self._last)
            if remaining > 0:
                self._sleep(remaining)
        self._last = self._clock()


_nominatim_throttle = _Throttle(NOMINATIM_MIN_INTERVAL)


def search_nominatim(postal_code: str, country: str, get: Callable = http_get_json,
                     throttle: _Throttle = _nominatim_throttle) -> list[Place]:
    params = {"postalcode": postal_code, "country": country.upper(), "format": "jsonv2",
              "addressdetails": "1", "limit": "5"}
    throttle.wait()
    data = get(NOMINATIM_URL + "?" + urllib.parse.urlencode(params))
    places = []
    for r in data or []:
        addr = r.get("address", {})
        code = (addr.get("country_code") or country).upper()
        if code != country.upper():
            continue
        town = (addr.get("city") or addr.get("town") or addr.get("village")
                or addr.get("municipality") or addr.get("county") or r.get("name", ""))
        label = _join(town, _state_label(addr.get("state", ""), code))
        places.append(Place(label, float(r["lat"]), float(r["lon"]), code, "nominatim"))
    return places


def geocode(postal_code: str, country: str = "US", get: Callable = http_get_json,
            throttle: _Throttle = _nominatim_throttle) -> list[Place]:
    """Matches for a postal code, country-filtered. Raises GeocodeError if nothing is found
    or every provider failed (for example offline)."""
    postal_code = postal_code.strip()
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9 -]{1,9}", postal_code):
        raise GeocodeError(f"{postal_code!r} does not look like a postal code")
    errors = []
    for name, search in (("open-meteo", lambda: search_open_meteo(postal_code, country, get)),
                         ("nominatim", lambda: search_nominatim(postal_code, country, get, throttle))):
        try:
            places = search()
        except Exception as exc:    # network, HTTP, JSON, odd payload: try the next provider
            errors.append(f"{name}: {exc!r}")
            continue
        if places:
            return places
        errors.append(f"{name}: no match")
    if all(e.endswith("no match") for e in errors):
        raise GeocodeError(f"no match for {postal_code} in {country.upper()}")
    raise GeocodeError("geocoding failed (offline?): " + "; ".join(errors)
                       + ". Retry later or set latitude/longitude in config.toml by hand.")


# --- config.toml update -------------------------------------------------------------

_SECTION = re.compile(r"^\s*\[([^\]]+)\]\s*(#.*)?$")


def _toml_value(value) -> str:
    if isinstance(value, str):
        return json.dumps(value)        # JSON string escaping is valid TOML basic-string syntax
    return repr(value)


def update_location(text: str, values: dict[str, object]) -> str:
    """Set keys in the [location] table, leaving every other line (and comment) untouched.

    Existing keys keep their position; new keys are added at the end of the table; a
    missing table is appended at the end of the file.
    """
    lines = text.splitlines()
    start = next((i for i, ln in enumerate(lines)
                  if (m := _SECTION.match(ln)) and m.group(1).strip() == "location"), None)
    if start is None:
        body = [f"{k} = {_toml_value(v)}" for k, v in values.items()]
        sep = [""] if lines and lines[-1].strip() else []
        return "\n".join(lines + sep + ["[location]"] + body) + "\n"
    end = next((i for i in range(start + 1, len(lines)) if _SECTION.match(lines[i])), len(lines))
    pending = dict(values)
    for i in range(start + 1, end):
        m = re.match(r"^(\s*)([A-Za-z0-9_-]+)(\s*=\s*)", lines[i])
        if m and m.group(2) in pending:
            comment = re.search(r"\s+#.*$", lines[i][m.end():])
            lines[i] = (f"{m.group(1)}{m.group(2)}{m.group(3)}{_toml_value(pending.pop(m.group(2)))}"
                        + (comment.group(0) if comment else ""))
    insert_at = end
    while insert_at > start + 1 and not lines[insert_at - 1].strip():
        insert_at -= 1                  # keep blank lines/comments for the next section below
    while insert_at > start + 1 and lines[insert_at - 1].lstrip().startswith("#") \
            and insert_at == end:
        insert_at -= 1                  # a trailing comment belongs to the following section
    lines[insert_at:insert_at] = [f"{k} = {_toml_value(v)}" for k, v in pending.items()]
    return "\n".join(lines) + "\n"


def write_location(path: str | Path, place: Place, postal_code: str, units: str | None = None) -> None:
    path = Path(path)
    text = path.read_text(encoding="utf-8") if path.exists() else ""
    lat, lon = place.rounded()
    values: dict[str, object] = {"postal_code": postal_code, "latitude": lat,
                                 "longitude": lon, "label": place.label}
    if units:
        values["units"] = units
    elif not re.search(r"^\s*units\s*=", text, re.M):
        values["units"] = "fahrenheit"
    new = update_location(text, values)
    import tomllib
    tomllib.loads(new)                  # never write a file we cannot parse back
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(new, encoding="utf-8")
    tmp.replace(path)
