"""Drifting clouds (clear / partly cloudy) and the continuous overcast layers."""
import random
from functools import lru_cache

from PIL import Image, ImageDraw

from ..state import Condition
from .palette import H, RGB, W, clamp, lerp, light_level, scale, twilight

# (light, shade) by condition in full daylight
_DAY = {
    Condition.CLEAR: ((240, 244, 252), (186, 198, 216)),
    Condition.PARTLY_CLOUDY: ((240, 244, 252), (186, 198, 216)),
    Condition.OVERCAST: ((168, 174, 186), (128, 134, 148)),
    Condition.FOG: ((168, 174, 186), (128, 134, 148)),
    Condition.RAIN: ((150, 156, 170), (108, 114, 130)),
    Condition.SNOW: ((158, 164, 178), (116, 122, 138)),
    Condition.THUNDER: ((92, 94, 112), (58, 60, 76)),
}

# (width, height, [(cx, cy, rx, ry)])
_SHAPES = (
    (20, 7, [(5, 5, 4, 3), (10, 4, 5, 4), (15, 5, 4, 3), (10, 6, 8, 3)]),
    (14, 6, [(4, 4, 3, 3), (8, 3, 4, 3), (11, 4, 3, 3), (7, 5, 6, 2)]),
    (24, 8, [(5, 6, 4, 3), (10, 4, 5, 4), (16, 5, 5, 4), (20, 6, 3, 3), (12, 7, 10, 3)]),
)
CLOUD_DROP = 4        # the sky is taller than it was; keep the clouds off the date row
# (shape, y, x0, speed px/s)
_DRIFTERS = ((0, 3, 4, 2.0), (2, 9, 40, 1.4), (1, 6, 22, 2.6), (0, 12, 58, 1.7),
             (1, 2, 50, 2.2), (2, 5, 8, 1.2), (1, 10, 30, 2.9))


def palette(eq: int, cond: Condition) -> tuple[RGB, RGB]:
    elev = eq / 40
    light, shade = _DAY[cond]
    lv, tw = light_level(elev), twilight(elev)
    out = []
    for col in (light, shade):
        night = lerp(scale(col, 0.2), (30, 40, 70), 0.25)
        col = lerp(night, col, lv)
        out.append(lerp(col, (255, 175, 135), 0.35 * tw))
    return out[0], out[1]


@lru_cache(maxsize=64)
def sprites(eq: int, cond: Condition) -> tuple[tuple[Image.Image, Image.Image], ...]:
    light, shade = palette(eq, cond)
    out = []
    for w, h, puffs in _SHAPES:
        mask = Image.new("L", (w, h), 0)
        draw = ImageDraw.Draw(mask)
        for cx, cy, rx, ry in puffs:
            draw.ellipse([cx - rx, cy - ry, cx + rx, cy + ry], fill=255)
        rgb = Image.new("RGB", (w, h), shade)
        ImageDraw.Draw(rgb).rectangle([0, 0, w, round(h * 0.55)], fill=light)
        out.append((rgb, mask))
    return tuple(out)


def wind_factor(wind: float) -> float:
    """Quantised so the drift speed (and so every position) rarely jumps between refreshes."""
    return 1.0 if wind < 8 else 1.6 if wind < 20 else 2.4 if wind < 35 else 3.2


def drifting_count(cond: Condition, cloud_cover: int) -> int:
    if cond is Condition.PARTLY_CLOUDY:
        return max(3, round(cloud_cover / 100 * 7))
    if cond is Condition.CLEAR:
        return 0 if cloud_cover < 12 else min(3, round(cloud_cover / 100 * 6))
    return 0


def draw_drifters(img: Image.Image, eq: int, cond: Condition, count: int, wind: float, t: float) -> None:
    if count <= 0:
        return
    sheet = sprites(eq, cond)
    wf = wind_factor(wind)
    for shape, y, x0, speed in _DRIFTERS[:count]:
        rgb, mask = sheet[shape]
        span = W + rgb.width
        x = round((x0 + speed * wf * t) % span) - rgb.width
        img.paste(rgb, (x, y + CLOUD_DROP), mask)


@lru_cache(maxsize=32)
def layers(eq: int, cond: Condition) -> tuple[tuple[Image.Image, Image.Image, float], ...]:
    """Wrap-around cloud decks for overcast-ish conditions: (rgb, mask, speed px/s) back to front."""
    light, shade = palette(eq, cond)
    decks = []
    for seed, rows, speed, tint in ((31, 7, 1.5, 0.0), (47, 10, 2.8, 0.3)):
        rng = random.Random(seed)
        size = (W, rows + 3)
        mask = Image.new("L", size, 0)
        rgb = Image.new("RGB", size, shade)
        md, rd = ImageDraw.Draw(mask), ImageDraw.Draw(rgb)
        body, top = lerp(shade, scale(shade, 0.72), tint), lerp(light, shade, tint)
        n = 11
        for k in range(n):
            cx = k * W / n + rng.uniform(-2, 2)
            cy = rng.uniform(rows - 7, rows - 1)
            r = rng.uniform(2.5, 5.5)
            for off in (-W, 0, W):
                box = [cx + off - r, cy - r * 0.8, cx + off + r, cy + r * 0.8]
                md.ellipse(box, fill=255)
                rd.ellipse(box, fill=body)
                rd.ellipse([box[0] + 1, box[1], box[2] - 1, box[3] - r * 0.7], fill=top)
        md.rectangle([0, 0, W, max(0, rows - 8)], fill=255)
        rd.rectangle([0, 0, W, max(0, rows - 8)], fill=top)
        decks.append((rgb, mask, speed))
    return tuple(decks)


def draw_layers(img: Image.Image, eq: int, cond: Condition, wind: float, t: float, density: float = 1.0) -> None:
    wf = wind_factor(wind)
    for rgb, mask, speed in layers(eq, cond):
        off = round(speed * wf * t) % W
        if density < 1.0:
            mask = mask.point(lambda v: round(v * density))
        for x in (off - W, off):
            img.paste(rgb, (x, 0), mask)
