"""The live loop: data threads feed a state snapshot, the compositor draws it, the display shows it.

Design points (DESIGN.md section 6): the render loop never waits on the network; a bug in a
widget must not blank the panel; systemd's watchdog restarts the process if the loop itself hangs.
"""
import logging
import os
import socket
import subprocess
import threading
import time
from collections.abc import Callable
from datetime import datetime
from pathlib import Path

from .app import Compositor, brightness_for, build_default_compositor
from .config import Config, ConfigError, load_config
from .data.homeassistant import HomeAssistantPoller
from .data.weather import WeatherFetcher
from .display.base import Display
from .state import AppState

log = logging.getLogger("matrix_display")

BRIGHTNESS_EVERY = 30.0       # seconds between brightness recalculations
STATS_EVERY = 60.0            # seconds between performance log lines
MAX_CONSECUTIVE_RENDER_FAILURES = 150   # about 10 s at 15 fps, then let systemd restart us


def state_dir() -> Path:
    """Where caches live: $MATRIX_STATE_DIR, else /var/lib/matrix-display when writable, else ~/.cache."""
    env = os.environ.get("MATRIX_STATE_DIR")
    if env:
        return Path(env)
    system = Path("/var/lib/matrix-display")
    if system.is_dir() and os.access(system, os.W_OK):
        return system
    return Path.home() / ".cache" / "matrix-display"


def sd_notify(message: str) -> None:
    """Tell systemd about readiness / liveness (no-op when not run by systemd)."""
    address = os.environ.get("NOTIFY_SOCKET")
    if not address:
        return
    if address.startswith("@"):
        address = "\0" + address[1:]
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as sock:
            sock.connect(address)
            sock.sendall(message.encode())
    except OSError:
        pass


class Sources:
    """The two background data feeds. Either may be absent (no location / no HA token)."""

    def __init__(self, cfg: Config, cache_dir: Path):
        self.weather: WeatherFetcher | None = None
        self.home: HomeAssistantPoller | None = None
        cache_dir.mkdir(parents=True, exist_ok=True)
        if cfg.location is not None:
            self.weather = WeatherFetcher(cfg.location, cfg.weather,
                                          cache_path=cache_dir / "weather.json")
        else:
            log.warning("no [location] in config: weather disabled (run tools/geocode.py <ZIP>)")
        if cfg.homeassistant.token:
            self.home = HomeAssistantPoller(cfg.homeassistant, cache_path=cache_dir / "home.json")
        else:
            log.warning("no MATRIX_HA_TOKEN: Home Assistant row disabled")

    def start(self) -> None:
        for source in (self.weather, self.home):
            if source is not None:
                source.start()

    def stop(self) -> None:
        for source in (self.weather, self.home):
            if source is not None:
                source.stop()

    def latest(self):
        return (self.weather.latest() if self.weather else None,
                self.home.latest() if self.home else None)


class FrameStats:
    def __init__(self):
        self.reset()

    def reset(self):
        self.count = 0
        self.total_ms = 0.0
        self.max_ms = 0.0
        self.overruns = 0

    def record(self, ms: float, overrun: bool) -> None:
        self.count += 1
        self.total_ms += ms
        self.max_ms = max(self.max_ms, ms)
        self.overruns += overrun

    def summary(self) -> str:
        avg = self.total_ms / self.count if self.count else 0.0
        return (f"frames={self.count} render avg={avg:.1f}ms max={self.max_ms:.1f}ms "
                f"overruns={self.overruns}")


_throttle_readable = True     # cleared once vcgencmd fails, so it is not re-run every minute


def system_health() -> str:
    """RSS, CPU temperature and throttle flags for the log (all best-effort)."""
    global _throttle_readable
    parts = []
    try:
        with open("/proc/self/status") as fh:
            for line in fh:
                if line.startswith("VmRSS:"):
                    parts.append(f"rss={int(line.split()[1]) // 1024}MB")
        with open("/proc/meminfo") as fh:
            for line in fh:
                if line.startswith("MemAvailable:"):
                    parts.append(f"mem_avail={int(line.split()[1]) // 1024}MB")
        with open("/sys/class/thermal/thermal_zone0/temp") as fh:
            parts.append(f"temp={int(fh.read()) / 1000:.0f}C")
    except (OSError, ValueError):
        pass
    if _throttle_readable:
        try:
            out = subprocess.run(["vcgencmd", "get_throttled"], capture_output=True, text=True,
                                 timeout=2).stdout.strip()
        except subprocess.TimeoutExpired:   # a busy moment: try again next time
            out = None
        except (OSError, subprocess.SubprocessError):
            out = ""
        if out is None:
            pass
        elif out.startswith("throttled="):   # unreadable (empty) once root privileges are dropped
            parts.append(out)
        else:
            _throttle_readable = False
    return " ".join(parts)


class Runner:
    def __init__(self, cfg: Config, display: Display, sources,
                 compositor: Compositor | None = None,
                 clock: Callable[[], datetime] | None = None,
                 monotonic: Callable[[], float] = time.monotonic,
                 sleep: Callable[[float], None] = time.sleep,
                 notify: Callable[[str], None] = sd_notify,
                 reload: Callable[[], tuple[Config, object]] | None = None):
        self.cfg = cfg
        self.display = display
        self.sources = sources
        self.compositor = compositor or build_default_compositor()
        self._clock = clock or (lambda: datetime.now().astimezone())
        self._monotonic = monotonic
        self._sleep = sleep
        self._notify = notify
        self._reload = reload
        self.stop_event = threading.Event()
        self.reload_event = threading.Event()
        self.stats = FrameStats()
        self._failures = 0

    # -- state ---------------------------------------------------------------
    def build_state(self) -> AppState:
        weather, home = self.sources.latest()
        now = self._clock()
        if weather is not None:           # show the date/time of the forecast location
            now = now.astimezone(weather.daily.sunrise.tzinfo)
        return AppState(now=now, weather=weather, home=home)

    # -- loop ----------------------------------------------------------------
    def run(self, max_frames: int | None = None) -> None:
        fps = max(1, self.cfg.display.scene_fps)
        period = 1.0 / fps
        start = next_frame = last_brightness = last_stats = last_ping = self._monotonic()
        last_brightness -= BRIGHTNESS_EVERY      # force an update on the first frame
        last_good = None
        frames = 0
        self._notify("READY=1")
        log.info("running at %d fps, brightness cap %d%%", fps, self.cfg.display.max_brightness)
        while not self.stop_event.is_set() and (max_frames is None or frames < max_frames):
            now_m = self._monotonic()
            if self.reload_event.is_set():
                self._do_reload()
            state = self.build_state()
            if now_m - last_brightness >= BRIGHTNESS_EVERY:
                daily = state.weather.daily if state.weather else None
                self.display.set_brightness(brightness_for(state.now, daily, self.cfg.display))
                last_brightness = now_m
            began = self._monotonic()
            try:
                frame = self.compositor.render(state, now_m - start)
                last_good, self._failures = frame, 0
            except Exception:
                self._failures += 1
                if self._failures == 1 or self._failures % 100 == 0:
                    log.exception("render failed (%d in a row); showing the last good frame",
                                  self._failures)
                if self._failures >= MAX_CONSECUTIVE_RENDER_FAILURES:
                    raise
                frame = last_good
            if frame is not None:
                self.display.show(frame)
            ms = (self._monotonic() - began) * 1000
            next_frame += period
            delay = next_frame - self._monotonic()
            self.stats.record(ms, overrun=delay < 0)
            frames += 1
            if now_m - last_ping >= 1.0:
                self._notify("WATCHDOG=1")
                last_ping = now_m
            if now_m - last_stats >= STATS_EVERY:
                log.info("%s %s", self.stats.summary(), system_health())
                self.stats.reset()
                last_stats = now_m
            if delay > 0:
                self._sleep(delay)
            else:
                next_frame = self._monotonic()     # fell behind: drop frames, do not spiral
        self._notify("STOPPING=1")

    def _do_reload(self) -> None:
        self.reload_event.clear()
        if self._reload is None:
            return
        try:
            cfg, sources = self._reload()
        except (ConfigError, OSError) as exc:
            log.error("reload failed, keeping the current configuration: %s", exc)
            return
        self.sources.stop()
        sources.start()
        self.cfg, self.sources = cfg, sources
        log.info("configuration reloaded")


class StaticSources:
    """Fixed data for demos and tests (`--sample`, `--offline`)."""

    def __init__(self, weather=None, home=None):
        self._weather, self._home = weather, home

    def start(self) -> None:
        pass

    def stop(self) -> None:
        pass

    def latest(self):
        return self._weather, self._home
