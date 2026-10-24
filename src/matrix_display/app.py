"""Frame composition. Regions are drawn by pluggable callables so phases 3 and 4
can each own their part without editing this file."""
from collections.abc import Callable

from PIL import Image, ImageDraw

from . import layout
from .state import AppState

# draw(frame, rect, state, t): draw into `frame` only within `rect`; t is seconds.
RegionDrawer = Callable[[Image.Image, layout.Rect, AppState, float], None]


class Compositor:
    def __init__(self, debug_layout: bool = False):
        self.debug_layout = debug_layout
        self._drawers: dict[str, RegionDrawer] = {}

    def register(self, region: str, drawer: RegionDrawer) -> None:
        if region not in layout.REGIONS:
            raise KeyError(f"unknown region {region!r}; known: {sorted(layout.REGIONS)}")
        self._drawers[region] = drawer

    def render(self, state: AppState, t: float = 0.0) -> Image.Image:
        frame = Image.new("RGB", (layout.WIDTH, layout.HEIGHT), (0, 0, 0))
        for name, rect in layout.REGIONS.items():
            drawer = self._drawers.get(name)
            if drawer:
                drawer(frame, rect, state, t)
        if self.debug_layout:
            draw = ImageDraw.Draw(frame)
            for rect in layout.REGIONS.values():
                draw.rectangle([rect.x, rect.y, rect.x2 - 1, rect.y2 - 1], outline=(60, 60, 60))
        return frame


def build_default_compositor(include_scene: bool = True, debug_layout: bool = False) -> Compositor:
    """Compositor with the UI widgets and, when available, the weather scene."""
    from .ui.widgets import WIDGETS

    comp = Compositor(debug_layout=debug_layout)
    for name, drawer in WIDGETS.items():
        comp.register(name, drawer)
    if include_scene:
        try:
            from .scene import draw_scene
        except ImportError:
            draw_scene = None
        if draw_scene is not None:
            comp.register("scene", draw_scene)
    return comp


def brightness_for(now, daily, cfg_display) -> int:
    """Brightness percent: the configured default by day, about half from sunset to
    sunrise, never above `max_brightness`. `daily` may be None (no weather yet)."""
    pct = cfg_display.brightness
    if daily is not None:
        tz = now.tzinfo
        sunrise = daily.sunrise.astimezone(tz).time() if tz else daily.sunrise.time()
        sunset = daily.sunset.astimezone(tz).time() if tz else daily.sunset.time()
        tod = now.time()
        if tod >= sunset or tod < sunrise:
            pct = pct // 2
    return max(1, min(pct, cfg_display.max_brightness))
