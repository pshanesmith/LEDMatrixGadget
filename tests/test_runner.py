import os
import socket
import sys
import threading

import pytest
from PIL import Image

from matrix_display.app import Compositor, build_default_compositor
from matrix_display.config import build_config
from matrix_display.display.emulator import EmulatorDisplay
from matrix_display.runner import (MAX_CONSECUTIVE_RENDER_FAILURES, Runner, StaticSources,
                                   FrameStats, sd_notify, state_dir)
from matrix_display.samples import NOON, sample_state


class FakeClock:
    """Deterministic time: sleep() advances monotonic()."""
    def __init__(self):
        self.t = 100.0
    def monotonic(self):
        return self.t
    def sleep(self, s):
        self.t += s


def make_runner(compositor=None, sample="rain_then_snow", reload=None, fps=15, notes=None):
    cfg = build_config({"display": {"scene_fps": fps, "brightness": 50, "max_brightness": 70}}, environ={})
    s = sample_state(sample)
    clock = FakeClock()
    display = EmulatorDisplay()
    brightness = []
    display.set_brightness = brightness.append
    runner = Runner(cfg, display, StaticSources(s.weather, s.home), compositor or build_default_compositor(),
                    clock=lambda: s.now, monotonic=clock.monotonic, sleep=clock.sleep,
                    notify=(notes.append if notes is not None else (lambda m: None)), reload=reload)
    return runner, display, clock, brightness


def test_runs_requested_frames_at_the_configured_rate():
    runner, display, clock, _ = make_runner(fps=15)
    start = clock.t
    runner.run(max_frames=30)
    assert display.last_frame.size == (64, 64) and display.last_frame.getbbox() is not None
    assert clock.t - start == pytest.approx(2.0, abs=0.01)     # 30 frames at 15 fps
    assert runner.stats.count == 30


def test_brightness_set_on_first_frame_and_clamped():
    runner, _, _, brightness = make_runner()
    runner.run(max_frames=3)
    assert brightness and 1 <= brightness[0] <= 70


def test_systemd_notifications():
    notes = []
    runner, *_ = make_runner(notes=notes)
    runner.run(max_frames=45)                                   # 3 s => several watchdog pings
    assert notes[0] == "READY=1" and notes[-1] == "STOPPING=1"
    assert notes.count("WATCHDOG=1") >= 2


def test_render_error_keeps_last_good_frame_and_does_not_crash():
    comp = Compositor()
    calls = {"n": 0}
    def flaky(frame, rect, state, t):
        calls["n"] += 1
        if calls["n"] > 3:
            raise RuntimeError("widget bug")
        frame.putpixel((rect.x, rect.y), (255, 0, 0))
    comp.register("temps", flaky)
    runner, display, _, _ = make_runner(compositor=comp)
    runner.run(max_frames=10)
    assert display.last_frame.getpixel((0, 35)) == (255, 0, 0)     # stale but present


def test_persistent_render_failure_raises_so_systemd_can_restart():
    comp = Compositor()
    comp.register("temps", lambda *a: 1 / 0)
    runner, *_ = make_runner(compositor=comp)
    with pytest.raises(ZeroDivisionError):
        runner.run(max_frames=MAX_CONSECUTIVE_RENDER_FAILURES + 5)


def test_slow_frames_are_dropped_not_queued():
    comp = Compositor()
    clock_holder = {}
    def slow(frame, rect, state, t):
        clock_holder["c"].t += 0.5                               # each frame takes 500 ms
    comp.register("temps", slow)
    runner, display, clock, _ = make_runner(compositor=comp)
    clock_holder["c"] = clock
    runner.run(max_frames=5)
    assert runner.stats.overruns == 5
    assert clock.t - 100.0 < 5 * 0.5 + 0.1                       # never sleeps extra to catch up


def test_stop_event_ends_loop():
    runner, *_ = make_runner()
    runner.stop_event.set()
    runner.run()                                                 # returns immediately
    assert runner.stats.count == 0


def test_reload_swaps_config_and_survives_errors():
    new_cfg = build_config({"display": {"scene_fps": 10}}, environ={})
    s = sample_state()
    seen = []
    def reload():
        seen.append(1)
        if len(seen) == 1:
            raise OSError("disk gone")
        return new_cfg, StaticSources(s.weather, s.home)
    runner, *_ = make_runner(reload=reload)
    runner.reload_event.set(); runner.run(max_frames=2)
    assert runner.cfg.display.scene_fps == 15                    # failed reload keeps old config
    runner.reload_event.set(); runner.run(max_frames=2)
    assert runner.cfg.display.scene_fps == 10


@pytest.mark.skipif(sys.platform != "linux", reason="systemd notify socket is Pi/Linux only")
def test_sd_notify_noop_without_socket_and_works_with_one(tmp_path, monkeypatch):
    monkeypatch.delenv("NOTIFY_SOCKET", raising=False)
    sd_notify("READY=1")                                         # must not raise
    path = str(tmp_path / "notify")
    server = socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM)
    server.bind(path)
    monkeypatch.setenv("NOTIFY_SOCKET", path)
    sd_notify("WATCHDOG=1")
    assert server.recv(100) == b"WATCHDOG=1"
    server.close()


def test_state_dir_precedence(tmp_path, monkeypatch):
    monkeypatch.setenv("MATRIX_STATE_DIR", str(tmp_path))
    assert state_dir() == tmp_path
    monkeypatch.delenv("MATRIX_STATE_DIR")
    assert state_dir().name == "matrix-display"


def test_frame_stats_summary():
    st = FrameStats(); st.record(10, False); st.record(30, True)
    assert "frames=2" in st.summary() and "avg=20.0ms" in st.summary() and "overruns=1" in st.summary()


def test_system_health_stops_calling_vcgencmd_once_unreadable(monkeypatch):
    import subprocess
    from matrix_display import runner
    calls = []

    def fake_run(*args, **kwargs):
        calls.append(args)
        return subprocess.CompletedProcess(args, 0, stdout="", stderr="")

    monkeypatch.setattr(runner, "_throttle_readable", True)
    monkeypatch.setattr(runner.subprocess, "run", fake_run)
    runner.system_health()
    runner.system_health()
    assert len(calls) == 1


def test_system_health_retries_vcgencmd_after_a_timeout(monkeypatch):
    import subprocess
    from matrix_display import runner
    calls = []

    def fake_run(*args, **kwargs):
        calls.append(args)
        if len(calls) == 1:
            raise subprocess.TimeoutExpired("vcgencmd", 2)
        return subprocess.CompletedProcess(args, 0, stdout="throttled=0x0", stderr="")

    monkeypatch.setattr(runner, "_throttle_readable", True)
    monkeypatch.setattr(runner.subprocess, "run", fake_run)
    runner.system_health()
    assert "throttled=0x0" in runner.system_health()
    assert len(calls) == 2
