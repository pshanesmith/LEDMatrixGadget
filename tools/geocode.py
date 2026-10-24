#!/usr/bin/env python3
"""Look up a ZIP / postal code once and write it to [location] in config/config.toml.

  tools/geocode.py 10001                 # US by default, asks you to confirm
  tools/geocode.py 10001 --country US --units fahrenheit
  tools/geocode.py --lat 40.748 --lon -73.994 --label "New York, NY"   # manual override
"""
import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "src"))

from matrix_display.data.geocode import GeocodeError, Place, geocode, write_location  # noqa: E402


def choose(places: list[Place], assume_yes: bool, ask=input) -> Place | None:
    for i, p in enumerate(places, 1):
        lat, lon = p.rounded()
        print(f"  {i}. {p.label}  ({lat}, {lon})  [{p.provider}]")
    if assume_yes:
        return places[0]
    answer = ask(f"Use which match? [1-{len(places)}, Enter = 1, n = cancel] ").strip().lower()
    if answer in ("n", "no", "q"):
        return None
    try:
        return places[int(answer or "1") - 1]
    except (ValueError, IndexError):
        print("not a valid choice", file=sys.stderr)
        return None


def main(argv=None, ask=input) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("postal_code", nargs="?")
    ap.add_argument("--country", default="US", help="ISO country code (default US)")
    ap.add_argument("--config", default="config/config.toml")
    ap.add_argument("--units", choices=("fahrenheit", "celsius"))
    ap.add_argument("--yes", action="store_true", help="accept the first match without asking")
    ap.add_argument("--lat", type=float)
    ap.add_argument("--lon", type=float)
    ap.add_argument("--label", default="")
    a = ap.parse_args(argv)

    if a.lat is not None or a.lon is not None:
        if a.lat is None or a.lon is None:
            ap.error("--lat and --lon go together")
        if not (-90 <= a.lat <= 90 and -180 <= a.lon <= 180):
            ap.error("coordinates out of range")
        place = Place(a.label or f"{a.lat:.3f}, {a.lon:.3f}", a.lat, a.lon, a.country.upper(), "manual")
    elif a.postal_code:
        try:
            places = geocode(a.postal_code, a.country)
        except GeocodeError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 1
        place = choose(places, a.yes, ask)
        if place is None:
            print("nothing written")
            return 2
    else:
        ap.error("give a postal code, or --lat/--lon")

    write_location(a.config, place, a.postal_code or "", a.units)
    lat, lon = place.rounded()
    print(f"wrote [location] to {a.config}: {place.label} ({lat}, {lon})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
