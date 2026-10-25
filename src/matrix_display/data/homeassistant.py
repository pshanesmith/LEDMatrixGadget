"""Home Assistant poller (DESIGN.md section 5, "Home Assistant").

One `POST {url}/api/template` per poll. The template runs on the HA side and returns
only compact JSON: locks that are not `locked`, lights that are `on` (group lights
already dropped via their `entity_id` attribute) and the states of the configured gates.
Glob exclusions (fnmatch) are applied here in Python, which keeps the template free of
user-supplied patterns. `parse_template_result` turns that JSON into a
`state.HomeSnapshot`; `HomeAssistantPoller` runs it on a thread, keeps the last good
snapshot (also on disk) and backs off exponentially after failures.

The token is only ever used to build the Authorization header; it is never logged,
repr'd, or put in an exception message.
"""
import json
import logging
import re
import threading
import urllib.error
import urllib.request
from collections.abc import Callable
from datetime import datetime, timezone
from fnmatch import fnmatchcase
from pathlib import Path

from ..config import HomeAssistantConfig, normalize_url
from ..state import HomeSnapshot

log = logging.getLogger(__name__)

TIMEOUT_SECONDS = 5
MAX_BACKOFF_SECONDS = 60
AUTH_RETRY_SECONDS = 300
LOCK_ATTENTION = frozenset({"unlocked", "jammed", "unlocking", "locking"})
UNAVAILABLE = frozenset({"unavailable", "unknown"})
GATE_OPEN = frozenset({"on", "open", "opening"})
ENTITY_ID = re.compile(r"^[a-z0-9_]+\.[a-z0-9_]+$")

# Gate ids are validated against ENTITY_ID before being rendered with json.dumps, so
# nothing from the config can break out of the Jinja string literals.
TEMPLATE = """\
{%- set tier_entity = __TIER__ -%}
{%- set ns = namespace(locks=[], lights=[]) -%}
{%- set updates = states.update | selectattr('state', 'eq', 'on') | map(attribute='entity_id') | list -%}
{%- for s in states.lock if s.state != 'locked' -%}
{%- set ns.locks = ns.locks + [[s.entity_id, s.state]] -%}
{%- endfor -%}
{%- for s in states.light if s.state == 'on' and s.attributes.get('entity_id') is none -%}
{%- set ns.lights = ns.lights + [s.entity_id] -%}
{%- endfor -%}
{{ {"locks": ns.locks, "lights": ns.lights, "updates": updates,
    "gates": __GATES__ | map('states') | list,
    "tier": states(tier_entity) if tier_entity else none} | tojson }}"""


class HomeAssistantError(RuntimeError):
    """Base class; messages never contain the token."""


class AuthError(HomeAssistantError):
    pass


class UnreachableError(HomeAssistantError):
    pass


class BadResponseError(HomeAssistantError):
    pass


def build_template(gates: tuple[str, ...], tier_entity: str = "") -> str:
    for entity in (*gates, *([tier_entity] if tier_entity else [])):
        if not ENTITY_ID.match(entity):
            raise ValueError(f"invalid entity id: {entity!r}")
    return (TEMPLATE.replace("__GATES__", json.dumps(list(gates)))
            .replace("__TIER__", json.dumps(tier_entity)))


def _excluded(entity_id: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatchcase(entity_id, p) for p in patterns)


def parse_template_result(data: object, config: HomeAssistantConfig,
                          now: datetime | None = None) -> HomeSnapshot:
    """Turn the rendered template JSON into a HomeSnapshot, applying the glob exclusions."""
    try:
        locks = data["locks"]            # type: ignore[index]
        lights = data["lights"]          # type: ignore[index]
        states = data["gates"]           # type: ignore[index]
        if len(states) != len(config.gates):
            raise ValueError("gate count mismatch")
        locks_unlocked = locks_unavailable = 0
        for entity_id, state in locks:
            if _excluded(entity_id, config.exclude_locks):
                continue
            if state in LOCK_ATTENTION:
                locks_unlocked += 1
            elif state in UNAVAILABLE:
                locks_unavailable += 1
        lights_on = sum(1 for e in lights if not _excluded(e, config.exclude_lights))
        gates_open = tuple(g for g, s in zip(config.gates, states) if s in GATE_OPEN)
        gates_unavailable = tuple(g for g, s in zip(config.gates, states) if s in UNAVAILABLE)
        updates = data.get("updates") or []  # type: ignore[attr-defined]
        updates_pending = sum(1 for e in updates if not _excluded(e, config.exclude_updates))
        power_tier = str(data.get("tier") or "unknown") if config.power_tier_entity else None  # type: ignore[attr-defined]
    except (KeyError, TypeError, ValueError) as exc:
        raise BadResponseError(f"unexpected template result ({type(exc).__name__})") from None
    return HomeSnapshot(
        fetched_at=now or datetime.now(timezone.utc),
        locks_unlocked=locks_unlocked,
        locks_unavailable=locks_unavailable,
        lights_on=lights_on,
        gates_open=gates_open,
        gates_unavailable=gates_unavailable,
        power_tier=power_tier,
        updates_pending=updates_pending,
    )


def snapshot_to_json(snapshot: HomeSnapshot) -> dict:
    return {
        "fetched_at": snapshot.fetched_at.isoformat(),
        "locks_unlocked": snapshot.locks_unlocked,
        "locks_unavailable": snapshot.locks_unavailable,
        "lights_on": snapshot.lights_on,
        "gates_open": list(snapshot.gates_open),
        "gates_unavailable": list(snapshot.gates_unavailable),
        "power_tier": snapshot.power_tier,
        "updates_pending": snapshot.updates_pending,
    }


def snapshot_from_json(data: dict) -> HomeSnapshot:
    return HomeSnapshot(
        fetched_at=datetime.fromisoformat(data["fetched_at"]),
        locks_unlocked=int(data["locks_unlocked"]),
        locks_unavailable=int(data["locks_unavailable"]),
        lights_on=int(data["lights_on"]),
        gates_open=tuple(data["gates_open"]),
        gates_unavailable=tuple(data["gates_unavailable"]),
        power_tier=data.get("power_tier"),
        updates_pending=int(data.get("updates_pending", 0)),
    )


def render_template(url: str, token: str, template: str,
                    timeout: float = TIMEOUT_SECONDS) -> object:
    """POST the template and return the parsed JSON it renders. Raises HomeAssistantError."""
    request = urllib.request.Request(
        url + "/api/template",
        data=json.dumps({"template": template}).encode(),
        headers={"Authorization": f"Bearer {token}", "Content-Type": "application/json",
                 "User-Agent": "matrix-display/0.1"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read()
    except urllib.error.HTTPError as exc:
        if exc.code in (401, 403):
            raise AuthError(f"Home Assistant rejected the token (HTTP {exc.code})") from None
        raise BadResponseError(f"Home Assistant returned HTTP {exc.code}") from None
    except (urllib.error.URLError, OSError) as exc:
        reason = getattr(exc, "reason", exc)
        raise UnreachableError(f"Home Assistant unreachable ({type(reason).__name__})") from None
    try:
        return json.loads(body)
    except ValueError:
        raise BadResponseError("Home Assistant returned invalid JSON") from None


class HomeAssistantPoller:
    def __init__(self, config: HomeAssistantConfig, cache_path: str | Path | None = None,
                 clock: Callable[[], datetime] | None = None,
                 render: Callable[[str, str, str], object] | None = None,
                 timeout: float = TIMEOUT_SECONDS):
        self._config = config
        self._url = normalize_url(config.url)
        self._template = build_template(config.gates, config.power_tier_entity)
        self._cache_path = Path(cache_path) if cache_path else None
        self._clock = clock or (lambda: datetime.now(timezone.utc))
        self._render = render or (lambda u, t, tpl: render_template(u, t, tpl, timeout))
        self._lock = threading.Lock()
        self._snapshot: HomeSnapshot | None = None
        self._failures = 0
        self._auth_logged = False
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None
        self._load_cache()

    def __repr__(self) -> str:
        return f"HomeAssistantPoller(url={self._url!r})"

    # -- public state -------------------------------------------------------
    def latest(self) -> HomeSnapshot | None:
        with self._lock:
            return self._snapshot

    def age_seconds(self) -> float | None:
        snap = self.latest()
        if snap is None:
            return None
        return (self._clock() - snap.fetched_at).total_seconds()

    @property
    def failures(self) -> int:
        return self._failures

    # -- polling ------------------------------------------------------------
    def poll_once(self) -> HomeSnapshot | None:
        """One poll. Returns the new snapshot, or None on failure (last good is kept)."""
        try:
            data = self._render(self._url, self._config.token, self._template)
            snap = parse_template_result(data, self._config, self._clock())
        except AuthError as exc:
            self._failures += 1
            if not self._auth_logged:
                self._auth_logged = True
                log.error("%s; check MATRIX_HA_TOKEN. Retrying every %ds.", exc, AUTH_RETRY_SECONDS)
            return None
        except HomeAssistantError as exc:
            self._failures += 1
            # First failure and then every 10th, so a long outage does not spam the log.
            if self._failures == 1 or self._failures % 10 == 0:
                log.warning("%s (failure %d); keeping last data", exc, self._failures)
            return None
        if self._failures:
            log.info("Home Assistant poll recovered after %d failure(s)", self._failures)
        self._failures = 0
        self._auth_logged = False
        with self._lock:
            self._snapshot = snap
        self._save_cache(snap)
        return snap

    def next_delay(self, auth_failed: bool = False) -> float:
        if auth_failed:
            return AUTH_RETRY_SECONDS
        if not self._failures:
            return self._config.poll_seconds
        return min(self._config.poll_seconds * 2 ** (self._failures - 1), MAX_BACKOFF_SECONDS)

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(target=self._run, name="ha-poller", daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=TIMEOUT_SECONDS + 2)
            self._thread = None

    def _run(self) -> None:
        while not self._stop.is_set():
            self.poll_once()
            self._stop.wait(self.next_delay(auth_failed=self._auth_logged))

    # -- cache --------------------------------------------------------------
    def _load_cache(self) -> None:
        if not self._cache_path or not self._cache_path.is_file():
            return
        try:
            self._snapshot = snapshot_from_json(json.loads(self._cache_path.read_text()))
        except (OSError, ValueError, KeyError, TypeError):
            log.warning("ignoring unreadable Home Assistant cache %s", self._cache_path)

    def _save_cache(self, snap: HomeSnapshot) -> None:
        if not self._cache_path:
            return
        tmp = self._cache_path.with_suffix(self._cache_path.suffix + ".tmp")
        try:
            tmp.write_text(json.dumps(snapshot_to_json(snap)))
            tmp.replace(self._cache_path)
        except OSError as exc:
            log.warning("could not write Home Assistant cache: %s", type(exc).__name__)
