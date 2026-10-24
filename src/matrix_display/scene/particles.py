"""Per-condition effects: rain, snow, fog bands and lightning. All pure functions of t."""
import math
import random
from functools import lru_cache

from PIL import Image, ImageChops, ImageDraw

from .palette import H, RGB, W, clamp, lerp

N_RAIN = 44
N_SNOW = 44

# WMO code -> intensity 0..1
_RAIN = {51: 0.35, 53: 0.45, 55: 0.55, 56: 0.4, 57: 0.55, 61: 0.55, 63: 0.8, 65: 1.0,
         66: 0.7, 67: 1.0, 80: 0.6, 81: 0.85, 82: 1.0, 95: 1.0, 96: 1.0, 99: 1.0}
_SNOW = {71: 0.5, 73: 0.8, 75: 1.0, 77: 0.45, 85: 0.7, 86: 1.0}


def rain_intensity(code: int) -> float:
    return _RAIN.get(code, 0.7)


def snow_intensity(code: int) -> float:
    return _SNOW.get(code, 0.7)


@lru_cache(maxsize=1)
def _rain_drops():
    rng = random.Random(902)
    return tuple((rng.uniform(0, W), rng.uniform(0, H + 4), rng.uniform(34, 52), rng.choice((2, 3, 3)))
                 for _ in range(N_RAIN))


@lru_cache(maxsize=1)
def _flakes():
    rng = random.Random(311)
    return tuple((rng.uniform(0, W), rng.uniform(0, H), rng.uniform(4, 9), rng.uniform(0.5, 2.0),
                  rng.uniform(0.6, 1.8), rng.uniform(0, 6.28), rng.random() < 0.55)
                 for _ in range(N_SNOW))


def draw_rain(img: Image.Image, intensity: float, wind: float, t: float, light: float) -> None:
    draw = ImageDraw.Draw(img)
    n = max(6, round(N_RAIN * intensity))
    slant = clamp(wind / 45, 0, 0.6)
    col = lerp((70, 90, 140), (150, 180, 235), light)
    for x0, y0, speed, length in _rain_drops()[:n]:
        y = (y0 + speed * t) % (H + length + 2) - length
        x = (x0 + slant * (speed * t)) % W
        draw.line([(x, y), (x - slant * length, y - length)], fill=col)


def draw_snow(img: Image.Image, intensity: float, wind: float, t: float) -> None:
    draw = ImageDraw.Draw(img)
    n = max(8, round(N_SNOW * intensity))
    drift = clamp(wind / 30, 0, 1.5) * 2.5
    for x0, y0, vy, amp, freq, phase, bright in _flakes()[:n]:
        y = (y0 + vy * t) % (H + 2) - 1
        x = (x0 + drift * t + amp * math.sin(freq * t + phase)) % W
        draw.point((round(x), round(y)), fill=(250, 252, 255) if bright else (160, 172, 195))


@lru_cache(maxsize=16)
def _fog_masks() -> tuple[Image.Image, Image.Image]:
    """Two horizontally periodic band masks (they wrap, so they can scroll forever)."""
    profile = (50, 105, 150, 105, 50)
    out = []
    for seed, rows in ((5, (6, 14, 22)), (9, (10, 18, 25))):
        rng = random.Random(seed)
        mask = Image.new("L", (W, H), 0)
        px = mask.load()
        waves = [(rng.randint(1, 3), rng.uniform(0, 6.28)) for _ in rows]
        for (k, ph), row in zip(waves, rows):
            for x in range(W):
                a = 0.65 + 0.35 * math.sin(2 * math.pi * k * x / W + ph)
                for i, v in enumerate(profile):
                    y = row - 2 + i
                    if 0 <= y < H:
                        px[x, y] = max(px[x, y], round(v * a))
        out.append(mask)
    return out[0], out[1]


def draw_fog(img: Image.Image, light: float, t: float) -> None:
    color = lerp((34, 38, 54), (206, 212, 222), light)
    for mask, speed in zip(_fog_masks(), (1.6, -2.6)):
        img.paste(color, (0, 0), ImageChops.offset(mask, round(speed * t) % W, 0))


# --- lightning -------------------------------------------------------------------------

PERIOD = 5.0


def _strike(slot: int):
    rng = random.Random(slot * 7919 + 13)
    if rng.random() > 0.8:
        return None
    start = rng.uniform(0.4, PERIOD - 1.0)
    x = rng.randint(10, 54)
    pts = [(x, 0)]
    y = 0
    while y < 20:
        y += rng.randint(2, 4)
        x += rng.randint(-3, 3)
        pts.append((x, y))
    return start, pts


def flash_level(t: float) -> float:
    """0..1 brightness of the flash at time t (a main strike plus a quick flicker)."""
    slot = int(t // PERIOD)
    strike = _strike(slot)
    if strike is None:
        return 0.0
    local = t - slot * PERIOD
    level = 0.0
    for delay, power in ((0.0, 1.0), (0.18, 0.6)):
        dt = local - strike[0] - delay
        if 0 <= dt < 0.5:
            level = max(level, power * math.exp(-dt / 0.07))
    return level


def draw_lightning(img: Image.Image, t: float) -> None:
    level = flash_level(t)
    if level < 0.05:
        return
    strike = _strike(int(t // PERIOD))
    if level > 0.3 and strike:
        ImageDraw.Draw(img).line(strike[1], fill=(255, 250, 200))
    img.paste((235, 238, 255), (0, 0), _flat_mask(round(level * 5)))


@lru_cache(maxsize=8)
def _flat_mask(step: int) -> Image.Image:
    return Image.new("L", (W, H), min(255, step * 22))
