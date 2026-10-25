"""Sky gradient, stars, sun and moon."""
import math
import random
from datetime import datetime, timezone
from functools import lru_cache

from PIL import Image, ImageDraw

from ..state import Condition
from .palette import GRAY_MIX, H, RGB, W, clamp, gray_of, lerp, light_level, scale, sky_colors

HORIZON = 28          # sun/moon centre sits here at sunrise/sunset
PEAK = 11             # and here at its highest


@lru_cache(maxsize=96)
def sky_image(eq: int, cond: Condition | None) -> Image.Image:
    elev = eq / 40
    top, bottom = sky_colors(elev)
    if cond is None:                      # no data yet: dim neutral sky
        top, bottom = (22, 26, 38), (44, 50, 66)
    else:
        light = light_level(elev)
        gray = lerp((16, 18, 28), (128, 136, 150), light)
        mix = GRAY_MIX[cond]
        dark = 0.82 if cond is Condition.THUNDER else 1.0
        top = scale(lerp(top, gray, mix), dark)
        bottom = scale(lerp(bottom, lerp(gray, (150, 156, 168), 0.3 * light), mix), dark)
    img = Image.new("RGB", (W, H))
    draw = ImageDraw.Draw(img)
    for y in range(H):
        draw.line([(0, y), (W - 1, y)], fill=lerp(top, bottom, (y / (HORIZON + 1)) ** 1.3))
    return img


@lru_cache(maxsize=1)
def _stars() -> tuple[tuple[int, int, float, float], ...]:
    rng = random.Random(1717)
    return tuple((rng.randrange(W), rng.randrange(0, 22), rng.uniform(0.5, 1.0),
                  rng.uniform(0, 6.28)) for _ in range(26))


def draw_stars(img: Image.Image, visibility: float, t: float) -> None:
    """Stars twinkle at their own rates (periods of about 2-6 s) and dip to nearly dark."""
    if visibility <= 0.02:
        return
    draw = ImageDraw.Draw(img)
    for x, y, base, phase in _stars():
        rate = 1.0 + (phase * 3.1) % 2.0
        wave = 0.5 + 0.5 * math.sin(t * rate + phase)
        b = visibility * base * (0.1 + 0.9 * wave ** 1.5)
        v = round(28 + 215 * clamp(b))
        draw.point((x, y), fill=(v, v, min(255, v + 25)))


SHOOTING_PERIOD = 23.0     # seconds between shooting stars
SHOOTING_LENGTH = 1.1      # seconds a streak is visible


def draw_shooting_star(img: Image.Image, visibility: float, t: float) -> None:
    """An occasional streak across the upper sky; position varies per cycle, all derived from t."""
    if visibility < 0.5:
        return
    cycle, local = divmod(t, SHOOTING_PERIOD)
    if local > SHOOTING_LENGTH:
        return
    rng = random.Random(int(cycle) * 7919 + 13)
    x0, y0 = rng.uniform(6, 40), rng.uniform(1, 12)
    dx, dy = 30.0 * rng.choice((1, 1, -1)), rng.uniform(7, 13)
    if dx < 0:
        x0 += 20
    draw = ImageDraw.Draw(img)
    for k in range(5):                              # head plus a fading tail
        tt = local - k * 0.035
        if tt < 0:
            continue
        fade = (1 - k / 5) * min(1.0, (SHOOTING_LENGTH - local) / 0.4)
        v = round(60 + 195 * clamp(fade * visibility))
        px, py = round(x0 + dx * tt), round(y0 + dy * tt)
        if 0 <= px < W and 0 <= py < HORIZON:
            draw.point((px, py), fill=(v, v, min(255, v + 20)))


def arc_position(progress: float) -> tuple[int, int]:
    return round(6 + progress * 52), round(HORIZON - math.sin(math.pi * progress) * (HORIZON - PEAK))


def draw_sun(img: Image.Image, progress: float, elev: float, hide: float, t: float = 0.0) -> None:
    if hide >= 0.97:
        return
    x, y = arc_position(progress)
    sky = img.getpixel((x, y))
    core = lerp((255, 120, 40), (255, 236, 130), elev / 0.3)
    draw = ImageDraw.Draw(img)
    glow = lerp(sky, core, 0.2 * (1 - hide))
    for dy in range(-4, 5):
        for dx in range(-4, 5):
            d = dx * dx + dy * dy
            if 6 < d <= 14:
                draw.point((x + dx, y + dy), fill=glow)
    # sparkle: eight rays that alternate on and off twice a second, fading with cloud cover
    ray = lerp(sky, core, 0.55 * (1 - hide))
    step = int(t * 2)
    for k in range(8):
        if (k + step) % 2 == 0:
            ang = k * math.pi / 4
            draw.point((x + round(5 * math.cos(ang)), y + round(5 * math.sin(ang))), fill=ray)
    body = lerp(core, sky, hide)
    for dy in range(-2, 3):
        half = 1 if abs(dy) == 2 else 2
        draw.line([(x - half, y + dy), (x + half, y + dy)], fill=body)


def moon_phase(now: datetime) -> float:
    """0 new, 0.5 full (mean synodic month from the 2000-01-06 new moon)."""
    days = (now.astimezone(timezone.utc) - datetime(2000, 1, 6, 18, 14, tzinfo=timezone.utc)
            ).total_seconds() / 86400
    return (days % 29.530588) / 29.530588


@lru_cache(maxsize=32)
def _moon_sprite(phase_q: int) -> tuple[Image.Image, Image.Image]:
    ph = 2 * math.pi * phase_q / 32
    k = math.cos(ph)
    waxing = phase_q < 16
    rgb = Image.new("RGB", (7, 7))
    mask = Image.new("L", (7, 7), 0)
    for py in range(7):
        for px in range(7):
            dx, dy = px - 3, py - 3
            if dx * dx + dy * dy > 10:
                continue
            xn, yn = dx / 3.2, dy / 3.2
            w = math.sqrt(max(0.0, 1 - yn * yn))
            lit = xn > k * w if waxing else xn < -k * w
            if lit:
                rgb.putpixel((px, py), (232, 232, 205))
                mask.putpixel((px, py), 255)
            else:
                rgb.putpixel((px, py), (36, 42, 66))
                mask.putpixel((px, py), 110)
    return rgb, mask


def draw_moon(img: Image.Image, progress: float, phase: float, hide: float) -> None:
    if hide >= 0.97:
        return
    x, y = arc_position(progress)
    rgb, mask = _moon_sprite(round(phase * 32) % 32)
    img.paste(rgb, (x - 3, y - 3), mask.point(lambda v: round(v * (1 - hide))))
