"""Rolling Blue Ridge ridges and pines. Shapes are fixed (seeded); only colours change.

The ridges are rounded, forested and hazy-blue in the distance. Their foliage colour is a
seeded per-pixel mix of tree colours whose proportions follow the day of the year, so the
shape of a ridge never changes but a hillside goes from green to red, orange and yellow in
October and to bare brown in winter.
"""
import math
import random
from functools import lru_cache

from PIL import Image, ImageDraw

from ..state import Condition
from .palette import GRAY_MIX, H, RGB, W, clamp, gray_of, lerp, light_level, scale, twilight

SEED = 64281

# (baseline y, [(peak x, height, width)]), back to front. Rounded domes with soft valleys.
_RIDGES = (
    (28, [(10, 15, 13), (34, 17, 12), (58, 14, 12)]),
    (31, [(0, 11, 10), (22, 12, 9), (46, 11, 10), (64, 12, 9)]),
    (34, [(14, 7, 9), (40, 7, 10), (60, 6, 8)]),
)
_PINES_X = (2, 7, 12, 16, 21, 44, 49, 53, 58, 62)
_PINE_WIDTHS = (1, 3, 3, 5, 5, 7, 7, 9)
_PINE_BASE = 34

# Foliage colours, in the order a hillside's weights are applied.
_TREES = {
    "conifer": (24, 62, 40), "green": (48, 108, 48), "spring": (116, 168, 68),
    "yellow": (226, 184, 44), "orange": (222, 110, 30), "red": (182, 44, 32),
    "brown": (112, 80, 52), "bare": (112, 94, 86), "blossom": (206, 150, 168),
}
# (day of year, {tree: weight}); linear in between, wrapping through the new year.
_SEASONS = (
    (15, {"conifer": 0.2, "bare": 0.55, "brown": 0.25}),
    (75, {"conifer": 0.2, "bare": 0.5, "brown": 0.2, "spring": 0.1}),
    (110, {"conifer": 0.12, "spring": 0.45, "green": 0.2, "blossom": 0.13, "bare": 0.1}),
    (150, {"conifer": 0.15, "green": 0.65, "spring": 0.2}),
    (200, {"conifer": 0.25, "green": 0.75}),
    (260, {"conifer": 0.2, "green": 0.6, "yellow": 0.12, "orange": 0.08}),
    (290, {"conifer": 0.12, "green": 0.2, "yellow": 0.24, "orange": 0.24, "red": 0.2}),
    (320, {"conifer": 0.15, "brown": 0.4, "orange": 0.2, "bare": 0.15, "yellow": 0.1}),
    (350, {"conifer": 0.2, "bare": 0.5, "brown": 0.3}),
)
_NIGHT_TINT = (14, 20, 40)
_SNOW = ((236, 242, 252), (62, 76, 112))
_PINE = ((14, 44, 32), (4, 10, 12))
_HAZE = (0.42, 0.2, 0.04)         # how far each ridge fades toward the distance haze
_WARM = (0.25, 0.25, 0.04)        # twilight tint per ridge
_FOG_AMOUNT = (0.6, 0.35, 0.1)
SEASON_STEP = 4                   # days per terrain cache entry


def season_key(doy: int) -> int:
    return doy // SEASON_STEP


def tree_weights(doy: int) -> list[tuple[RGB, float]]:
    """(colour, weight) for the foliage on `doy`, weights summing to 1."""
    pts = _SEASONS + ((_SEASONS[0][0] + 365, _SEASONS[0][1]),)
    d = doy if doy >= _SEASONS[0][0] else doy + 365
    for (d0, w0), (d1, w1) in zip(pts, pts[1:]):
        if d <= d1:
            f = (d - d0) / (d1 - d0)
            break
    mix = {k: w0.get(k, 0) * (1 - f) + w1.get(k, 0) * f for k in _TREES}
    total = sum(mix.values())
    return [(_TREES[k], v / total) for k, v in mix.items() if v > 0]


def _foliage(u: float, weights) -> RGB:
    acc = 0.0
    for col, w in weights:
        acc += w
        if u < acc:
            return col
    return weights[-1][0]


def _profile(rng: random.Random, base: int, peaks) -> list[int]:
    tops = []
    for x in range(W):
        h = max(ph * math.exp(-(((x - px) / pw) ** 2)) for px, ph, pw in peaks)
        h += 0.7 * math.sin(x * 0.55 + base)             # small knolls on the ridgeline
        jitter = 1 if rng.random() < 0.12 else 0
        tops.append(min(base + 3, round(base - h) + jitter))
    return tops


@lru_cache(maxsize=1)
def _shapes():
    rng = random.Random(SEED)
    ridges = []
    for base, peaks in _RIDGES:
        tops = _profile(rng, base, peaks)
        blocks = {(bx, by): rng.random() for bx in range(W // 2 + 1) for by in range(H // 2 + 1)}
        u = [[blocks[(x // 2, y // 2)] if rng.random() > 0.3 else rng.random() for y in range(H)]
             for x in range(W)]
        ridges.append((tops, u))
    pines = [(x, rng.randint(5, 8), _PINE_BASE + rng.randint(0, 1)) for x in _PINES_X]
    return ridges, pines


def _lit(day: RGB, light: float, tw: float, warm: float) -> RGB:
    night = lerp(scale(day, 0.2), _NIGHT_TINT, 0.45)
    return lerp(lerp(night, day, light), (255, 150, 110), warm * tw)


@lru_cache(maxsize=256)
def terrain(eq: int, cond: Condition | None, season: int) -> tuple[Image.Image, Image.Image]:
    """(RGB, mask) of the ridges and pines at 64x35 for elevation eq/40, a condition and a
    season_key()."""
    elev = eq / 40
    light, tw = light_level(elev), twilight(elev)
    g = GRAY_MIX[cond] * 0.7
    snowing = cond is Condition.SNOW
    ridges, pines = _shapes()
    weights = tree_weights(season * SEASON_STEP + SEASON_STEP // 2)
    haze = lerp((20, 26, 48), (158, 178, 210), light)
    fog_col = lerp((26, 30, 44), (190, 196, 206), light)
    snow = _lit(_SNOW[0], light, tw, 0.4)

    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    for i, (tops, u) in enumerate(ridges):
        for x in range(W):
            top = tops[x]
            ahead = tops[min(W - 1, x + 3)]
            face = 0.82 if ahead < top - 1 else 1.0   # slopes rising to the right sit in shade
            for y in range(max(top, 0), H):
                depth = clamp((y - top) / 12)
                col = _lit(_foliage(u[x][y], weights), light, tw, _WARM[i])
                col = lerp(col, haze, _HAZE[i])
                col = lerp(col, gray_of(col), g)
                if cond is Condition.FOG:
                    col = lerp(col, fog_col, _FOG_AMOUNT[i])
                col = scale(col, face * (1 - 0.28 * depth))
                if snowing and y - top < 2 + (x + y) % 2:   # a dusting on the crests
                    col = lerp(col, scale(snow, face * 0.92 + 0.08), 0.85)
                img.putpixel((x, y), col + (255,))

    pine = lerp(_PINE[1], _PINE[0], light)
    pine = lerp(lerp(pine, haze, 0.05), gray_of(pine), g)
    tip = _lit(_SNOW[0], light, tw, 0.3)
    for x, h, base in pines:
        for r in range(h):
            half = _PINE_WIDTHS[min(r, len(_PINE_WIDTHS) - 1)] // 2
            y = base - h + r + 1
            col = tip if snowing and r < 2 else pine
            draw.line([(x - half, y), (x + half, y)], fill=col + (255,))
        draw.point([(x, base + 1)], fill=scale(pine, 0.7) + (255,))
    return img.convert("RGB"), img.getchannel("A")
