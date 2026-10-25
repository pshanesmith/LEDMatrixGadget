"""Pixel-art icons. Each is a tuple of rows; characters index a palette, '.' is clear.

Weather icons are 9x9, lock, bulb and bolt 7x9, gate 9x9.
"""
from PIL import Image

from ..state import Condition

_PAL = {
    "Y": (255, 200, 20),    # sun
    "O": (255, 140, 20),
    "W": (235, 235, 245),   # light cloud / snow
    "G": (140, 148, 165),   # grey cloud
    "D": (90, 96, 115),     # dark cloud
    "B": (70, 140, 255),    # rain
    "S": (200, 230, 255),   # snow flake
    "F": (120, 130, 140),   # fog
    "M": (235, 230, 190),   # moon
}

_CLOUD = (
    "...WWW...",
    "..WWWWW..",
    ".WWWWWWWW",
    "WWWWWWWWW",
    ".WWWWWWW.",
)

WEATHER: dict[Condition, tuple[str, ...]] = {
    Condition.CLEAR: (
        "....Y....",
        ".Y.....Y.",
        "..YYYYY..",
        "..YYYYY..",
        "Y.YYYYY.Y",
        "..YYYYY..",
        "..YYYYY..",
        ".Y.....Y.",
        "....Y....",
    ),
    Condition.PARTLY_CLOUDY: (
        "..Y......",
        ".YYY..WW.",
        "YYYYYWWWW",
        ".YYWWWWWW",
        "..WWWWWWW",
        ".WWWWWWWW",
        ".WWWWWWW.",
        ".........",
        ".........",
    ),
    Condition.OVERCAST: (
        ".........",
        "...GGG...",
        "..GGGGG.G",
        ".GGGGGGGG",
        "GGGGGGGGG",
        "GGGGGGGGG",
        ".GGGGGGG.",
        ".........",
        ".........",
    ),
    Condition.FOG: (
        "..GGGG...",
        ".GGGGGG..",
        "GGGGGGGGG",
        ".........",
        "FFFFFFFF.",
        ".........",
        ".FFFFFFFF",
        ".........",
        "FFFFFF...",
    ),
    Condition.RAIN: (
        "...GGG...",
        "..GGGGG..",
        ".GGGGGGGG",
        "GGGGGGGGG",
        ".GGGGGGG.",
        ".........",
        ".B..B..B.",
        "B..B..B..",
        ".B..B..B.",
    ),
    Condition.SNOW: (
        "...GGG...",
        "..GGGGG..",
        ".GGGGGGGG",
        "GGGGGGGGG",
        ".GGGGGGG.",
        ".........",
        ".S..S..S.",
        "..S..S...",
        ".S..S..S.",
    ),
    Condition.THUNDER: (
        "...DDD...",
        "..DDDDD..",
        ".DDDDDDDD",
        "DDDDDDDDD",
        ".DDDYYDD.",
        "....YY...",
        "...YY....",
        "..YYYY...",
        "....Y....",
    ),
}

_LOCK_CLOSED = (
    "..###..",
    ".#...#.",
    ".#...#.",
    "#######",
    "#######",
    "###.###",
    "###.###",
    "#######",
    "#######",
)
_LOCK_OPEN = (
    "..###..",
    ".#...#.",
    ".#.....",
    "#######",
    "#######",
    "###.###",
    "###.###",
    "#######",
    "#######",
)
BULB = (
    "..###..",
    ".#####.",
    "#######",
    "#######",
    "#######",
    ".#####.",
    "..###..",
    "..###..",
    "...#...",
)
BOLT = (
    "....###",
    "...###.",
    "..###..",
    ".######",
    "....##.",
    "...##..",
    "..##...",
    ".##....",
    ".#.....",
)
_GATE_CLOSED = (
    "#.......#",
    "#########",
    "#.#.#.#.#",
    "#.#.#.#.#",
    "#########",
    "#.#.#.#.#",
    "#.#.#.#.#",
    "#.......#",
    "#.......#",
)
_GATE_OPEN = (
    "#.......#",
    "#.......#",
    "#.......#",
    "###...###",
    "#.#...#.#",
    "###...###",
    "#.......#",
    "#.......#",
    "#.......#",
)

MOON = (
    "...MMM...",
    "..MMM....",
    ".MMM.....",
    ".MMM.....",
    ".MMM.....",
    ".MMM.....",
    "..MMM...M",
    "...MMMMM.",
    ".....MM..",
)

LOCK = {False: _LOCK_CLOSED, True: _LOCK_OPEN}   # key: open
GATE = {False: _GATE_CLOSED, True: _GATE_OPEN}


def blit(frame: Image.Image, x: int, y: int, rows: tuple[str, ...], color=None) -> None:
    """Draw an icon at (x, y). `color` overrides the palette (single-colour icons)."""
    w, h = frame.size
    px = frame.load()
    for dy, row in enumerate(rows):
        for dx, ch in enumerate(row):
            if ch == ".":
                continue
            if 0 <= x + dx < w and 0 <= y + dy < h:
                px[x + dx, y + dy] = color or _PAL.get(ch, (255, 255, 255))


def draw_weather_icon(frame: Image.Image, x: int, y: int, condition: Condition,
                      night: bool = False) -> None:
    """A clear sky is drawn as a moon at night; other conditions look the same day and night."""
    blit(frame, x, y, MOON if night and condition is Condition.CLEAR else WEATHER[condition])
