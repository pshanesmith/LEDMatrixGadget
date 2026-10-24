import time

from PIL import ImageChops

from matrix_display.app import build_default_compositor
from matrix_display.samples import sample_state


def changed_fraction(name, seconds, step=0.25):
    comp = build_default_compositor()
    st = sample_state(name)
    frames = [comp.render(st, i * step).crop((0, 7, 64, 35)) for i in range(int(seconds / step))]
    changed = set()
    for a, b in zip(frames, frames[1:]):
        px = ImageChops.difference(a, b).convert("L").load()
        changed |= {(x, y) for y in range(28) for x in range(64) if px[x, y]}
    return len(changed) / (64 * 28)


def test_clear_day_is_not_frozen():
    assert changed_fraction("clear_day", 10) > 0.03


def test_clear_night_twinkles_and_has_shooting_star():
    assert changed_fraction("clear_night", 25) > 0.03


def test_deterministic_for_same_time():
    st = sample_state("clear_night")
    a = build_default_compositor().render(st, 7.3)
    b = build_default_compositor().render(st, 7.3)
    assert a.tobytes() == b.tobytes()


def test_frame_cost_stays_small():
    comp = build_default_compositor()
    st = sample_state("clear_day")
    comp.render(st, 0.0)
    t0 = time.perf_counter()
    for i in range(30):
        comp.render(st, i / 15)
    assert (time.perf_counter() - t0) / 30 < 0.03       # budget is 66 ms at 15 fps
