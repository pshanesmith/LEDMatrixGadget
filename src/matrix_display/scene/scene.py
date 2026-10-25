"""The weather scene: draw_scene(frame, rect, state, t) for layout.SCENE (64x28)."""
from PIL import Image

from ..layout import Rect
from ..state import AppState, Condition
from . import clouds, digits, mountains, particles, sky
from .palette import H, W, clamp, light_level, sun_position
from PIL import ImageDraw

FADE_SECONDS = 2.0
_NEUTRAL_EQ = -6
_PRECIP_SKY = (Condition.OVERCAST, Condition.FOG, Condition.RAIN, Condition.SNOW, Condition.THUNDER)


def render_state(state: AppState, t: float) -> Image.Image:
    """One 64x28 frame. A pure function of (state, t)."""
    weather = state.weather
    if weather is None:
        img = sky.sky_image(_NEUTRAL_EQ, None).copy()
        rgb, mask = mountains.terrain(_NEUTRAL_EQ, None, _season(state))
        img.paste(rgb, (0, 0), mask)
        _paste_text(img, "?")
        _paste_date(img, state)
        return img

    cur, cond = weather.current, weather.current.condition
    pos = sun_position(state.now, cur.is_day, weather.daily.sunrise, weather.daily.sunset)
    eq = round(pos.elev * 40)
    light = light_level(eq / 40)
    img = sky.sky_image(eq, cond).copy()

    # celestial body, then clouds that can cover it
    cloud = cur.cloud_cover
    hide = 1.0 if cond in (Condition.RAIN, Condition.SNOW, Condition.THUNDER) else \
        0.92 if cond in (Condition.OVERCAST, Condition.FOG) else clamp((cloud - 50) / 50)
    if pos.elev >= 0:
        sky.draw_sun(img, pos.progress, pos.elev, hide, t)
    else:
        star_vis = clamp((-0.05 - pos.elev) / 0.25) * (0.0 if cond in _PRECIP_SKY else clamp(1 - cloud / 70))
        sky.draw_stars(img, star_vis, t)
        sky.draw_shooting_star(img, star_vis, t)
        sky.draw_moon(img, pos.progress, sky.moon_phase(state.now), hide)

    drifters = clouds.drifting_count(cond, cloud)
    if cond is Condition.CLEAR:
        drifters = max(drifters, 1)        # one wisp keeps a clear sky from looking frozen
    clouds.draw_drifters(img, eq, cond, drifters, cur.wind_speed, t)
    if cond in _PRECIP_SKY and cond is not Condition.FOG:
        clouds.draw_layers(img, eq, cond, cur.wind_speed, t)

    rgb, mask = mountains.terrain(eq, cond, _season(state))
    img.paste(rgb, (0, 0), mask)

    if cond is Condition.FOG:
        particles.draw_fog(img, light, t)
    elif cond is Condition.RAIN:
        particles.draw_rain(img, particles.rain_intensity(cur.code), cur.wind_speed, t, light)
    elif cond is Condition.THUNDER:
        particles.draw_rain(img, 1.0, cur.wind_speed, t, light)
        particles.draw_lightning(img, t)
    elif cond is Condition.SNOW:
        particles.draw_snow(img, particles.snow_intensity(cur.code), cur.wind_speed, t)

    _paste_text(img, digits.format_temperature(cur.temperature))
    _paste_date(img, state)
    return img


def _season(state: AppState) -> int:
    return mountains.season_key(state.now.timetuple().tm_yday)


def _overlay(img: Image.Image, glyph: Image.Image, outline: Image.Image, x: int) -> None:
    img.paste((0, 0, 0), (x, 0), outline)
    img.paste((255, 255, 255), (x, 0), glyph)


def _paste_text(img: Image.Image, text: str) -> None:
    glyph, outline = digits.text_masks(text)
    _overlay(img, glyph, outline, W - glyph.width)


def _paste_date(img: Image.Image, state: AppState) -> None:
    """Weekday and month/day at the top left, mirroring the temperature's margin."""
    now = state.now
    glyph, outline = digits.date_masks(f"{now:%a} {now.month}/{now.day}".upper())
    _overlay(img, glyph, outline, 0)


class SceneRenderer:
    """Adds the 2 s cross-fade between conditions.

    The picture is a pure function of (state, t); the only memory is the previously drawn
    state, so a condition change can blend old into new. Time going backwards or jumping
    more than 5 s (previews, a stalled loop) resets it instead of fading.
    """

    def __init__(self):
        self._last: tuple[Condition | None, AppState, float] | None = None
        self._fade: tuple[AppState, float] | None = None

    def render(self, state: AppState, t: float) -> Image.Image:
        cond = state.weather.current.condition if state.weather else None
        if self._last is not None:
            last_cond, last_state, last_t = self._last
            if t < last_t or t - last_t > 5:
                self._fade = None
            elif cond is not last_cond:
                self._fade = (last_state, t)
        self._last = (cond, state, t)
        img = render_state(state, t)
        if self._fade is not None:
            old_state, t0 = self._fade
            alpha = (t - t0) / FADE_SECONDS
            if alpha >= 1:
                self._fade = None
            else:
                img = Image.blend(render_state(old_state, t), img, alpha)
        return img

    def draw(self, frame: Image.Image, rect: Rect, state: AppState, t: float) -> None:
        img = self.render(state, t)
        frame.paste(img.crop((0, 0, min(rect.w, W), min(rect.h, H))), (rect.x, rect.y))


_default = SceneRenderer()


def draw_scene(frame: Image.Image, rect: Rect, state: AppState, t: float) -> None:
    """RegionDrawer for layout.SCENE."""
    _default.draw(frame, rect, state, t)
