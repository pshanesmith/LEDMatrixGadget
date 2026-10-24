#!/usr/bin/env python3
"""List Home Assistant locks, lights and gate candidates so they can be chosen
for config.toml. Read-only; standard library only (works before the venv exists).

The token is never accepted on the command line (shell history) and never
printed. It is read from, in order:
  1. the MATRIX_HA_TOKEN environment variable
  2. an env file: --env-file, else ./secrets.env, else /etc/matrix/secrets.env
  3. an interactive hidden prompt (only when stdin is a terminal)

The base URL comes from MATRIX_HA_URL, --url, or the default below.
"""
import argparse
import getpass
import json
import os
import re
import stat
import sys
import urllib.error
import urllib.request

DEFAULT_URL = "http://homeassistant:8123"
ENV_FILES = ("secrets.env", "/etc/matrix/secrets.env")
GATE_WORDS = re.compile(r"gate", re.I)
GATE_DEVICE_CLASSES = {"door", "garage_door", "gate", "opening"}
# Lights that are not room lighting: switch status-LED bars (Inovelli "RGB
# Indicator"), access-point / switch LEDs. Counting them would inflate "lights on".
NOISE_LIGHT = re.compile(r"(rgb_indicator|_ap_led|_led)$")
NOISE_GLOBS = ["light.*_rgb_indicator", "light.*_ap_led", "light.*_led"]


def normalize_url(url):
    url = url.strip()
    if "://" not in url:
        url = "http://" + url
    return url.rstrip("/")


def parse_env_file(path):
    values = {}
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, value = line.partition("=")
            values[key.strip()] = value.strip().strip("'\"")
    return values


def load_settings(env_file=None, environ=None, url_arg=None, interactive=None):
    """Return (url, token). Raises SystemExit with a message when no token."""
    environ = os.environ if environ is None else environ
    file_values = {}
    candidates = [env_file] if env_file else list(ENV_FILES)
    for path in candidates:
        if path and os.path.isfile(path):
            mode = os.stat(path).st_mode
            if mode & (stat.S_IRWXG | stat.S_IRWXO):
                print(f"warning: {path} is readable by other users; run: chmod 600 {path}",
                      file=sys.stderr)
            file_values = parse_env_file(path)
            break
        if env_file:
            sys.exit(f"error: env file not found: {env_file}")

    token = environ.get("MATRIX_HA_TOKEN") or file_values.get("MATRIX_HA_TOKEN")
    if not token:
        if interactive is None:
            interactive = sys.stdin.isatty()
        if interactive:
            token = getpass.getpass("Home Assistant long-lived token (hidden): ").strip()
    if not token:
        sys.exit("error: no token. Put MATRIX_HA_TOKEN=... in ./secrets.env "
                 "(see config/secrets.env.example) or export MATRIX_HA_TOKEN.")
    url = url_arg or environ.get("MATRIX_HA_URL") or file_values.get("MATRIX_HA_URL") or DEFAULT_URL
    return normalize_url(url), token


def fetch_states(url, token, timeout=15):
    req = urllib.request.Request(
        url + "/api/states",
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as exc:
        if exc.code == 401:
            sys.exit("error: Home Assistant rejected the token (401). Create a new "
                     "long-lived token and update secrets.env.")
        sys.exit(f"error: Home Assistant returned HTTP {exc.code} for {url}/api/states")
    except (urllib.error.URLError, TimeoutError, OSError) as exc:
        sys.exit(f"error: cannot reach {url}: {getattr(exc, 'reason', exc)}")


def _entry(state):
    attrs = state.get("attributes", {})
    domain = state["entity_id"].split(".", 1)[0]
    return {
        "entity_id": state["entity_id"],
        "name": attrs.get("friendly_name", ""),
        "state": state.get("state", ""),
        "device_class": attrs.get("device_class", ""),
        # Light groups and similar aggregates list their members; counting both
        # the group and the members would double count.
        "is_group": isinstance(attrs.get("entity_id"), list),
        "is_noise": bool(domain == "light" and NOISE_LIGHT.search(state["entity_id"])),
    }


def classify(states):
    """Split raw /api/states output into locks, lights, gates and other cover/sensors."""
    locks, lights, gates, other = [], [], [], []
    for state in states:
        domain = state["entity_id"].split(".", 1)[0]
        e = _entry(state)
        if domain == "lock":
            locks.append(e)
        elif domain == "light":
            lights.append(e)
        elif domain in ("cover", "binary_sensor"):
            text = f"{e['entity_id']} {e['name']}"
            if GATE_WORDS.search(text):
                gates.append(e)
            elif e["device_class"] in GATE_DEVICE_CLASSES:
                other.append(e)
    key = lambda e: e["entity_id"]
    return sorted(locks, key=key), sorted(lights, key=key), sorted(gates, key=key), sorted(other, key=key)


def _table(title, rows, note="", hidden=0):
    print(f"\n{title} ({len(rows)}){note}")
    if hidden:
        print(f"  ({hidden} more hidden; use --all-lights to show them)")
    if not rows:
        print("  (none found)")
        return
    width = max(len(r["entity_id"]) for r in rows)
    for r in rows:
        flags = " [group]" if r["is_group"] else ""
        cls = f" ({r['device_class']})" if r["device_class"] else ""
        print(f"  {r['entity_id']:<{width}}  {r['state']:<12} {r['name']}{cls}{flags}")


def toml_snippet(locks, lights, gates):
    def lst(items):
        return "[" + ", ".join(f'"{i}"' for i in items) + "]"
    groups = [l["entity_id"] for l in lights if l["is_group"]]
    noise = NOISE_GLOBS if any(l["is_noise"] for l in lights) else []
    out = ["[homeassistant]",
           "# Review each list; delete entries you do not want counted.",
           f"gates = {lst([g['entity_id'] for g in gates])}",
           f"exclude_locks = []",
           "# Globs (fnmatch) matched against entity ids; light groups and status LEDs are excluded.",
           f"exclude_lights = {lst(groups + noise)}"]
    return "\n".join(out)


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--url", help=f"Home Assistant base URL (default {DEFAULT_URL})")
    ap.add_argument("--env-file", help="env file holding MATRIX_HA_TOKEN")
    ap.add_argument("--all-doors", action="store_true",
                    help="also list covers/binary sensors with a door-like device class")
    ap.add_argument("--all-lights", action="store_true",
                    help="also list status-LED / access-point lights normally hidden")
    ap.add_argument("--toml", action="store_true", help="print a config.toml snippet")
    args = ap.parse_args(argv)

    url, token = load_settings(env_file=args.env_file, url_arg=args.url)
    print(f"Querying {url} ...", file=sys.stderr)
    locks, lights, gates, other = classify(fetch_states(url, token))

    _table("Locks", locks)
    real = [l for l in lights if not l["is_noise"]]
    shown = lights if args.all_lights else real
    on = sum(1 for l in real if l["state"] == "on")
    _table("Lights", shown,
           f" - {len(real)} room lights, {on} on now; groups flagged [group]",
           hidden=0 if args.all_lights else len(lights) - len(real))
    _table("Gate candidates", gates, " - entities whose id or name contains 'gate'")
    if args.all_doors or not gates:
        _table("Other door/garage/opening entities", other,
               " - pick from here if no gate matched by name")
    if args.toml:
        print("\n# --- paste into config/config.toml ---")
        print(toml_snippet(locks, lights, gates))
    return 0


if __name__ == "__main__":
    sys.exit(main())
