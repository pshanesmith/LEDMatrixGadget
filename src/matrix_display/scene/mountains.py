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

# Terrain styles. Each ridge is (baseline y, [(peak x, height, width)], foliage 0..1, day blue,
# night colour), back to front. Wide, low domes keep the slopes gentle like old Appalachian
# ridges; far ridges are plain atmospheric blue and only the near ones carry autumn colour.
_BLUES = ((90, 128, 200), (68, 106, 178), (52, 84, 144), (42, 66, 108), (32, 50, 78))
_NIGHTS = ((36, 46, 88), (29, 38, 72), (22, 31, 58), (16, 24, 44), (11, 17, 32))
STYLES = {
    # three broad rolling ridges
    "rolling": (
        [(25, [(16, 10, 26), (52, 11, 24)], 0.0), (30, [(0, 9, 24), (34, 10, 22), (64, 9, 24)], 0.5),
         (35, [(20, 9, 22), (56, 9, 22)], 0.9)], 0, 0),
    # five receding layers, like the photo: blue distance, colour in the near hills
    "layered": (
        [(22, [(22, 7, 28), (60, 8, 24)], 0.0), (25, [(4, 8, 26), (40, 9, 24)], 0.0),
         (28, [(26, 9, 26), (62, 9, 24)], 0.15), (31, [(8, 9, 24), (46, 10, 24)], 0.55),
         (35, [(28, 9, 24), (62, 8, 22)], 0.9)], 0, 0),
    # layered blue distance with a band of big round foreground trees
    "trees": (
        [(23, [(20, 8, 28), (58, 9, 24)], 0.0), (26, [(2, 9, 26), (38, 9, 24)], 0.0),
         (29, [(26, 9, 26), (62, 9, 24)], 0.3), (33, [(10, 8, 24), (48, 8, 24)], 0.75)], 1, 12),
    # smooth blue-to-russet hills (no patchwork) with two rows of individually coloured trees
    "grove": (
        [(21, [(20, 8, 28), (60, 9, 24)], 0.0), (24, [(2, 9, 26), (40, 10, 24)], 0.0),
         (27, [(26, 9, 26), (62, 10, 24)], 0.5), (30, [(8, 8, 24), (46, 9, 24)], 0.8)], 2, 1),
}
STYLE = "layered"
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
_SNOW = ((236, 242, 252), (62, 76, 112))
_PINE = ((14, 44, 32), (4, 10, 12))
_FOG_AMOUNT = (0.6, 0.5, 0.4, 0.3, 0.2)
_PINE_NIGHT = (7, 12, 20)
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
        h += 0.5 * math.sin(x * 0.4 + base)              # faint knolls on the ridgeline
        jitter = 1 if rng.random() < 0.06 else 0
        tops.append(min(base + 3, round(base - h) + jitter))
    return tops


@lru_cache(maxsize=4)
def _shapes(style: str):
    rng = random.Random(SEED)
    ridges = []
    for base, peaks, fol in STYLES[style][0]:
        tops = _profile(rng, base, peaks)
        blocks = {(bx, by): rng.random() for bx in range(W // 4 + 2) for by in range(H // 3 + 1)}
        u = [[blocks[((x + (y // 3) % 2 * 2) // 4, y // 3)] if rng.random() > 0.1 else rng.random() for y in range(H)]
             for x in range(W)]
        ridges.append((tops, u, fol))
    pines = [(x, rng.randint(5, 8), _PINE_BASE + rng.randint(0, 1)) for x in _PINES_X]
    blobs = []        # (x, centre y, radius, colour pick, brightness), back row first
    rows = ((30, 4, 4, 0.78), (33, 5, 5, 1.0))[2 - STYLES[style][1]:]
    for row, (cy, rmax, step, bright) in enumerate(rows):
        for k, x in enumerate(range(-2 + 3 * row, W + 3, step)):
            blobs.append((x, cy + rng.randint(-1, 2), rng.randint(3, rmax),
                          (k * 0.37 + row * 0.5 + rng.random() * 0.25) % 1, bright))
    return ridges, pines, blobs


def _shade(day: RGB, night: RGB, light: float, tw: float, warm: float) -> RGB:
    return lerp(lerp(night, day, light), (255, 150, 110), warm * tw)


@lru_cache(maxsize=256)
def terrain(eq: int, cond: Condition | None, season: int,
            style: str | None = None) -> tuple[Image.Image, Image.Image]:
    """(RGB, mask) of the ridges and trees at 64x35 for elevation eq/40, a condition and a
    season_key(). Foliage colour fades out with the light: at night the hills are plain
    blue silhouettes, stepped so each ridge stays distinct from the sky and its neighbours."""
    style = style or STYLE
    elev = eq / 40
    light, tw = light_level(elev), twilight(elev)
    g = GRAY_MIX[cond] * 0.7
    snowing = cond is Condition.SNOW
    ridges, pines, blobs = _shapes(style)
    weights = tree_weights(season * SEASON_STEP + SEASON_STEP // 2)
    fog_col = lerp((26, 30, 44), (190, 196, 206), light)
    snow = _shade(_SNOW[0], _SNOW[1], light, tw, 0.4)
    n = len(ridges)
    mean = lerp(tuple(round(sum(c[k] * w for c, w in weights)) for k in range(3)), (204, 82, 30), 0.5)
    ramp = n - 1 if n > 1 else 1

    def tone(i: int, foliage: RGB | None, fol: float) -> RGB:
        k = min(len(_BLUES) - 1, round(i * (len(_BLUES) - 1) / ramp))
        day = _BLUES[k]
        if foliage is not None and fol:
            day = lerp(day, scale(foliage, 0.6 + 0.4 * fol), fol)   # far hills darker, near ones bright
        night = _NIGHTS[k]
        col = _shade(day, night, light, tw, 0.15 + 0.1 * (i / ramp))
        col = lerp(col, gray_of(col), g)
        if cond is Condition.FOG:
            col = lerp(col, fog_col, _FOG_AMOUNT[min(i, 4)])
        return col

    img = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)

    for i, (tops, u, fol) in enumerate(ridges):
        base_col = tone(i, None, 0)
        for x in range(W):
            top = tops[x]
            ahead = tops[min(W - 1, x + 3)]
            face = 0.88 if ahead < top - 1 else 1.0   # slopes rising to the right sit in shade
            for y in range(max(top, 0), H):
                depth = clamp((y - top) / 12)
                if not fol:
                    col = base_col
                elif STYLES[style][2] == 1:                  # smooth: the season's average foliage
                    col = tone(i, mean, fol)
                    if y - top < 1:
                        col = scale(col, 1.12)               # lit crest
                else:
                    col = tone(i, _foliage(u[x][y], weights), fol)
                col = scale(col, face * (1 - 0.22 * depth))
                if snowing and y - top < 2 + (x + y) % 2:   # a dusting on the crests
                    col = lerp(col, scale(snow, face * 0.92 + 0.08), 0.85)
                img.putpixel((x, y), col + (255,))

    for x, cy, r, v, bright in blobs:                     # round foreground trees, each its own colour
        col = scale(tone(n, _foliage(v, weights), 0.95), bright)
        lo, hi = scale(col, 0.72), scale(col, 1.18)
        draw.ellipse([x - r, cy - r, x + r, cy + r], fill=lo + (255,))
        draw.ellipse([x - r, cy - r, x + r - 1, cy + r - 1], fill=col + (255,))
        draw.point([(x - 1, cy - r + 1)], fill=hi + (255,))

    pine = lerp(_PINE_NIGHT, _PINE[0], light)
    pine = lerp(pine, gray_of(pine), g)
    tip = _shade(_SNOW[0], _SNOW[1], light, tw, 0.3)
    for x, h, base in pines:
        for r in range(h):
            half = _PINE_WIDTHS[min(r, len(_PINE_WIDTHS) - 1)] // 2
            y = base - h + r + 1
            col = tip if snowing and r < 2 else pine
            draw.line([(x - half, y), (x + half, y)], fill=col + (255,))
        draw.point([(x, base + 1)], fill=scale(pine, 0.7) + (255,))
    return img.convert("RGB"), img.getchannel("A")
