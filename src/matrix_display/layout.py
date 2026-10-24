"""Fixed screen regions (DESIGN.md section 3). Widgets draw only inside their Rect."""
from typing import NamedTuple

WIDTH = 64
HEIGHT = 64


class Rect(NamedTuple):
    x: int
    y: int
    w: int
    h: int

    @property
    def x2(self) -> int:
        return self.x + self.w

    @property
    def y2(self) -> int:
        return self.y + self.h


DATE = Rect(0, 0, 64, 7)
SCENE = Rect(0, 7, 64, 28)
TEMPS = Rect(0, 35, 64, 8)
NEXT_CHANGE = Rect(0, 43, 64, 10)
HOME = Rect(0, 53, 64, 11)

REGIONS = {
    "date": DATE,
    "scene": SCENE,
    "temps": TEMPS,
    "next_change": NEXT_CHANGE,
    "home": HOME,
}
