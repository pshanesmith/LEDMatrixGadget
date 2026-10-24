"""Tiny 3x5 pixel digits for the temperature readout (no dependency on the UI fonts)."""
from functools import lru_cache

from PIL import Image, ImageFilter

_GLYPHS = {
    "0": ("111", "101", "101", "101", "111"), "1": ("010", "110", "010", "010", "111"),
    "2": ("111", "001", "111", "100", "111"), "3": ("111", "001", "111", "001", "111"),
    "4": ("101", "101", "111", "001", "001"), "5": ("111", "100", "111", "001", "111"),
    "6": ("111", "100", "111", "101", "111"), "7": ("111", "001", "001", "001", "001"),
    "8": ("111", "101", "111", "101", "111"), "9": ("111", "101", "111", "001", "111"),
    "-": ("000", "000", "111", "000", "000"), "?": ("111", "001", "010", "000", "010"),
}


@lru_cache(maxsize=64)
def text_masks(text: str) -> tuple[Image.Image, Image.Image]:
    """(glyph mask, outline mask) for `text` plus a trailing degree dot, with a 1 px margin."""
    glyphs = [_GLYPHS.get(ch, _GLYPHS["?"]) for ch in text]
    width = 4 * len(glyphs) + 1          # glyphs, gap, degree dot
    glyph = Image.new("L", (width + 2, 7), 0)
    px = glyph.load()
    for i, rows in enumerate(glyphs):
        for y, row in enumerate(rows):
            for x, bit in enumerate(row):
                if bit == "1":
                    px[1 + 4 * i + x, 1 + y] = 255
    px[1 + width - 1, 1] = 255           # degree dot, top right
    outline = glyph.filter(ImageFilter.MaxFilter(3)).point(lambda v: v * 115 // 255)
    return glyph, outline


def format_temperature(value: float) -> str:
    return str(round(value))
