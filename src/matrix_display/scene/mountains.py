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
    # four receding ridges: smooth blue-to-russet hills, then three large foreground trees
    "grove": (
        [(21, [(20, 8, 28), (60, 9, 24)], 0.0), (24, [(2, 9, 26), (40, 10, 24)], 0.0),
         (27, [(26, 9, 26), (62, 10, 24)], 0.5), (30, [(8, 8, 24), (46, 9, 24)], 0.8)],),
}
STYLE = "grove"
_PINES_X = (2, 7, 12, 16, 21, 44, 49, 53, 58, 62)
_PINES_KEPT = (3, 4, 6)         # indices into _PINES_X that stay evergreen (x = 16, 21, 49)
# Large foreground trees: (trunk x, crown width, crown height, crown bottom y, colour pick)
_BIG_TREES = ((6, 22, 20, 32, 0.66), (57, 19, 18, 33, 0.93), (41, 13, 12, 34, 0.4))
_PINE_WIDTHS = (1, 3, 3, 5, 5, 5, 7, 7, 7, 9, 9)
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
        ridges.append((tops, fol))
    pines = [(x, rng.randint(9, 12), _PINE_BASE + rng.randint(0, 1)) for x in _PINES_X]
    pines = [pines[k] for k in _PINES_KEPT]
    return ridges, pines


@lru_cache(maxsize=8)
def _big_tree(w: int, h: int, seed: int, v: float):
    """A deciduous tree as pixels: a crown of overlapping leaf clusters (each lit from the upper
    left, each a slightly different fall colour near `v`), a ragged edge, gaps that show the
    branches, and a trunk that forks up into the crown.

    Returns (leaves, branches): leaves is {(dx, dy): (shade char, colour pick)}, branches a
    list of ((x0, y0), (x1, y1)) in crown coordinates, the first starting below the crown."""
    rng = random.Random(seed)
    clusters = []
    for k in range(w * h // 9):                         # clumps fill an irregular dome
        for _ in range(20):
            cx, cy = rng.uniform(0, w), rng.uniform(0, h)
            nx, ny = (cx - w / 2) / (w / 2), (cy - h * 0.55) / (h * 0.55)
            if nx * nx + ny * ny <= 0.7 + rng.uniform(-0.1, 0.2):
                break
        clusters.append((cx, cy, rng.uniform(2.2, 4.2), rng.uniform(1.8, 3.2),
                         clamp(v + rng.uniform(-0.2, 0.2), 0, 0.999)))
    clusters.sort(key=lambda c: c[1])                    # lower clumps overlap the ones above
    leaves = {}
    for cx, cy, rx, ry, pick in clusters:
        for y in range(int(cy - ry) - 1, int(cy + ry) + 2):
            for x in range(int(cx - rx) - 1, int(cx + rx) + 2):
                nx, ny = (x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry
                d = nx * nx + ny * ny
                if d > 1 + rng.uniform(-0.25, 0.2) or not (0 <= x < w and 0 <= y < h):
                    continue
                light = nx * 0.45 + ny * 0.85
                ch = "h" if light < -0.35 else "m" if light < 0.3 else "x" if light > 0.62 else "d"
                if ch == "m" and rng.random() < 0.1:
                    ch = "s"
                leaves[(x, y)] = (ch, pick)
    for (x, y) in list(leaves):                          # ragged rim, gaps in the shade
        edge = any((x + dx, y + dy) not in leaves for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)))
        if (edge and rng.random() < 0.22) or (leaves[(x, y)][0] in "dx" and rng.random() < 0.1):
            del leaves[(x, y)]
    top = h * 0.25
    branches = [((w // 2, h + 3), (w // 2, h * 0.62)),
                ((w // 2, h * 0.7), (w * 0.25, top + h * 0.2)),
                ((w // 2, h * 0.66), (w * 0.78, top + h * 0.1)),
                ((w // 2, h * 0.6), (w * 0.5, top))]
    return leaves, branches


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
    ridges, pines = _shapes(style)
    weights = tree_weights(season * SEASON_STEP + SEASON_STEP // 2)
    lively = [(c, w) for c, w in weights if c not in (_TREES["brown"], _TREES["bare"], _TREES["conifer"])]
    total = sum(w for _, w in lively)
    leaves_pal = [(c, w / total) for c, w in lively] if total > 0.3 else weights   # bright leaves in season
    fog_col = lerp((26, 30, 44), (190, 196, 206), light)
    snow = _shade(_SNOW[0], _SNOW[1], light, tw, 0.4)
    n = len(ridges)
    warm = sum(w for c, w in weights if c in (_TREES["yellow"], _TREES["orange"], _TREES["red"]))
    mean = lerp(tuple(round(sum(c[k] * w for c, w in weights)) for k in range(3)),
                (204, 82, 30), min(0.5, warm * 0.7))      # hillsides glow russet only in the fall
    bare = sum(w for c, w in weights if c in (_TREES["brown"], _TREES["bare"]))
    leaf_loss = clamp((bare - 0.3) / 0.6) * 0.85           # how many leaves have dropped
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

    for i, (tops, fol) in enumerate(ridges):
        base_col = tone(i, None, 0)
        for x in range(W):
            top = tops[x]
            ahead = tops[min(W - 1, x + 3)]
            face = 0.88 if ahead < top - 1 else 1.0   # slopes rising to the right sit in shade
            for y in range(max(top, 0), H):
                depth = clamp((y - top) / 12)
                col = tone(i, mean, fol) if fol else base_col
                if fol and y - top < 1:
                    col = scale(col, 1.12)               # lit crest
                col = scale(col, face * (1 - 0.22 * depth))
                if snowing and y - top < 2 + (x + y) % 2:   # a dusting on the crests
                    col = lerp(col, scale(snow, face * 0.92 + 0.08), 0.85)
                img.putpixel((x, y), col + (255,))

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
    dull = tone(n, _TREES["brown"], 0.95)
    mute = clamp(leaf_loss / 0.85) * 0.85
    for k, (bx, cw, ch, cb, v) in enumerate(_BIG_TREES):       # foreground trees, drawn last
        leaves, branches = _big_tree(cw, ch, 100 + k, v)
        ox, oy = bx - cw // 2, cb - ch
        bark = lerp((14, 11, 12), (92, 66, 48), light)
        for (x0, y0), (x1, y1) in branches:                    # trunk and the limbs in the crown
            draw.line([(ox + x0, oy + y0), (ox + x1, oy + y1)], fill=bark + (255,))
        draw.rectangle([bx - 1, cb - 2, bx, H], fill=bark + (255,))   # 2 px trunk
        draw.line([(bx - 1, cb - 2), (bx - 1, H)], fill=scale(bark, 1.3) + (255,))
        draw.point([(bx + 1, y) for y in range(cb + 2, H)], fill=scale(bark, 0.7) + (255,))
        for (dx, dy), (c, pick) in leaves.items():
            if ((dx * 73856093 ^ dy * 19349663 ^ k * 83492791) % 100) / 100 < leaf_loss:
                continue                                        # bare branches show through
            leaf = lerp(tone(n, _foliage(pick, leaves_pal), 0.95), dull, mute)   # late leaves fade to brown
            shade = {"h": 1.4, "m": 1.0, "d": 0.6, "x": 0.4, "s": 1.14}[c]
            draw.point([(ox + dx, oy + dy)], fill=scale(leaf, shade) + (255,))
    return img.convert("RGB"), img.getchannel("A")
