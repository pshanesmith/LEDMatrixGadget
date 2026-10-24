"""Parallax ridges, snow caps and pines. Shapes are fixed (seeded); only colours change."""
import random
from functools import lru_cache

from PIL import Image, ImageDraw

from ..state import Condition
from .palette import GRAY_MIX, H, RGB, W, clamp, gray_of, lerp, light_level, scale, twilight

SEED = 64281

# (baseline y, snowline y or None, [(peak x, height, slope)]), back to front.
_RIDGES = (
    (18, 12, [(8, 8, 0.9), (24, 10, 0.85), (40, 7, 1.0), (58, 9, 0.9)]),
    (21, 15, [(4, 6, 0.9), (18, 7, 0.8), (33, 9, 0.9), (50, 7, 0.85), (63, 8, 0.9)]),
    (24, None, [(12, 3, 0.5), (40, 4, 0.45), (60, 3, 0.5)]),
)
_PINES_X = (2, 7, 12, 16, 21, 44, 49, 53, 58, 62)
_PINE_WIDTHS = (1, 3, 3, 5, 5, 7, 7, 9)

# day / night colours
_ROCK = (((118, 140, 178), (30, 40, 76)), ((78, 96, 128), (18, 26, 52)), ((28, 52, 48), (8, 14, 22)))
_SNOW = ((236, 242, 252), (62, 76, 112))
_PINE = ((14, 44, 32), (4, 10, 12))
_FOG_AMOUNT = (0.6, 0.35, 0.1)
_WARM = (0.25, 0.25, 0.04)   # twilight tint per ridge


def _profile(rng: random.Random, base: int, peaks) -> list[int]:
    tops = []
    for x in range(W):
        h = max(ph - abs(x - px) * s for px, ph, s in peaks)
        jitter = 1 if rng.random() < 0.18 else 0
        tops.append(min(base + 3, round(base - h) + jitter))
    return tops


@lru_cache(maxsize=1)
def _shapes():
    rng = random.Random(SEED)
    ridges = []
    for base, snowline, peaks in _RIDGES:
        tops = _profile(rng, base, peaks)
        wobble = [rng.randint(0, 2) for _ in range(W)]
        ridges.append((tops, snowline, wobble))
    pines = [(x, rng.randint(5, 8), 26 + rng.randint(0, 1)) for x in _PINES_X]
    return ridges, pines


def _tone(pair, light: float, tw: float, warm: float) -> RGB:
    day, night = pair
    col = lerp(night, day, light)
    return lerp(col, (255, 150, 110), warm * tw)


@lru_cache(maxsize=96)
def terrain(eq: int, cond: Condition | None) -> tuple[Image.Image, Image.Image]:
    """(RGB, mask) of the ridges and pines at 64x28 for elevation eq/40 and a condition."""
    elev = eq / 40
    light, tw = light_level(elev), twilight(elev)
    g = GRAY_MIX[cond] * 0.7
    snowing = cond is Condition.SNOW
    ridges, pines = _shapes()
    fog_col = lerp((26, 30, 44), (190, 196, 206), light)

    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    for i, (tops, snowline, wobble) in enumerate(ridges):
        rock = _tone(_ROCK[i], light, tw, _WARM[i])
        snow = _tone(_SNOW, light, tw, 0.45 if i < 2 else 0.1)
        rock = lerp(rock, gray_of(rock), g)
        snow = lerp(snow, gray_of(snow), g * 0.5)
        if cond is Condition.FOG:
            rock = lerp(rock, fog_col, _FOG_AMOUNT[i])
            snow = lerp(snow, fog_col, _FOG_AMOUNT[i])
        line = snowline
        if snowing:
            line = snowline + 3 if snowline else min(tops) + 1   # near hills: a dusting
        for x in range(W):
            top = tops[x]
            ahead = tops[min(W - 1, x + 3)]
            face = 0.8 if ahead < top - 1 else 1.0   # slopes rising to the right sit in shade
            for y in range(max(top, 0), H):
                depth = clamp((y - top) / 10)
                col = scale(rock, face * (1 - 0.3 * depth))
                if line is not None and y <= line + wobble[x] and top <= line:
                    col = scale(snow, face * 0.92 + 0.08)
                img.putpixel((x, y), col + (255,))

    pine = lerp(_PINE[1], _PINE[0], light)
    pine = lerp(pine, gray_of(pine), g)
    tip = _tone(_SNOW, light, tw, 0.3)
    for x, h, base in pines:
        for r in range(h):
            half = _PINE_WIDTHS[min(r, len(_PINE_WIDTHS) - 1)] // 2
            y = base - h + r + 1
            col = tip if snowing and r < 2 else pine
            draw.line([(x - half, y), (x + half, y)], fill=col + (255,))
        draw.point([(x, base + 1)], fill=scale(pine, 0.7) + (255,))
    return img.convert("RGB"), img.getchannel("A")
