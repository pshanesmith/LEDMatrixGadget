//! Procedural weather scene (DESIGN.md section 4): `SceneRenderer` draws into `layout::SCENE`.

pub mod cache;
pub mod clouds;
pub mod digits;
pub mod mountains;
pub mod palette;
pub mod particles;
pub mod sky;

use rand::SeedableRng;

/// The generator behind every fixed shape in the scene (stars, ridges, trees, rain, snow). ChaCha8 is portable and
/// its output for a seed is stable across platforms and `rand` versions (the sampling helpers on top are not
/// promised stable across `rand` versions, so a `rand` upgrade can change the shapes; `Cargo.lock` pins it and
/// the golden images in `tests/golden/` notice).
pub type SceneRng = rand_chacha::ChaCha8Rng;

pub fn rng(seed: u64) -> SceneRng {
    SceneRng::seed_from_u64(seed)
}

use chrono::Datelike;

use crate::gfx::{pyround, Img, Mask};
use crate::layout::Rect;
use crate::state::{AppState, Condition};
use palette::{clamp01, light_level, sun_position, H, W};

pub const FADE_SECONDS: f64 = 2.0;
const NEUTRAL_EQ: i32 = -6;
const PRECIP_SKY: [Condition; 5] =
    [Condition::Overcast, Condition::Fog, Condition::Rain, Condition::Snow, Condition::Thunder];

/// Pixels between the date/temperature text and the panel edge.
pub const EDGE_MARGIN: i32 = 1;

fn season(state: &AppState) -> i32 {
    mountains::season_key(state.now.ordinal())
}

/// One 64x35 frame. A pure function of (state, t).
pub fn render_state(state: &AppState, t: f64) -> Img {
    let mut img = Img::black(W, H);
    render_state_into(state, t, &mut img);
    img
}

/// `render_state` into an existing image, which is overwritten (and reuses its buffer).
pub fn render_state_into(state: &AppState, t: f64, img: &mut Img) {
    let Some(weather) = &state.weather else {
        img.copy_from(&sky::sky_image(NEUTRAL_EQ, None));
        let terrain = mountains::terrain(NEUTRAL_EQ, None, season(state));
        img.paste(&terrain.0, 0, 0, Some(&terrain.1));
        let masks = digits::text_masks_cached("?");
        paste_masks(img, &masks);
        paste_date(img, state);
        return;
    };

    let cur = &weather.current;
    let cond = cur.condition();
    let pos = sun_position(state.now, cur.is_day, weather.daily.sunrise, weather.daily.sunset);
    let eq = pyround(pos.elev * 40.0) as i32;
    let light = light_level(eq as f64 / 40.0);
    img.copy_from(&sky::sky_image(eq, Some(cond)));

    // celestial body, then clouds that can cover it
    let cloud = cur.cloud_cover as f64;
    let hide = match cond {
        Condition::Rain | Condition::Snow | Condition::Thunder => 1.0,
        Condition::Overcast | Condition::Fog => 0.92,
        _ => clamp01((cloud - 50.0) / 50.0),
    };
    if pos.elev >= 0.0 {
        sky::draw_sun(img, pos.progress, pos.elev, hide, t);
    } else {
        let clear_sky = if PRECIP_SKY.contains(&cond) { 0.0 } else { clamp01(1.0 - cloud / 70.0) };
        let star_vis = clamp01((-0.05 - pos.elev) / 0.25) * clear_sky;
        sky::draw_stars(img, star_vis, t);
        sky::draw_shooting_star(img, star_vis, t);
        sky::draw_moon(img, pos.progress, sky::moon_phase(state.now), hide);
    }

    let mut drifters = clouds::drifting_count(cond, cur.cloud_cover);
    if cond == Condition::Clear {
        drifters = drifters.max(1); // one wisp keeps a clear sky from looking frozen
    }
    clouds::draw_drifters(img, eq, cond, drifters, cur.wind_speed, t);
    if PRECIP_SKY.contains(&cond) && cond != Condition::Fog {
        clouds::draw_layers(img, eq, cond, cur.wind_speed, t);
    }

    let terrain = mountains::terrain(eq, Some(cond), season(state));
    img.paste(&terrain.0, 0, 0, Some(&terrain.1));

    match cond {
        Condition::Fog => particles::draw_fog(img, light, t),
        Condition::Rain => {
            particles::draw_rain(img, particles::rain_intensity(cur.code), cur.wind_speed, t, light)
        }
        Condition::Thunder => {
            particles::draw_rain(img, 1.0, cur.wind_speed, t, light);
            particles::draw_lightning(img, t);
        }
        Condition::Snow => {
            particles::draw_snow(img, particles::snow_intensity(cur.code), cur.wind_speed, t)
        }
        _ => {}
    }

    paste_masks(img, &digits::temperature_masks_cached(cur.temperature));
    paste_date(img, state);
}

fn overlay(img: &mut Img, glyph: &Mask, outline: &Mask, x: i32) {
    img.paste_color([0, 0, 0], x, EDGE_MARGIN, outline);
    img.paste_color([255, 255, 255], x, EDGE_MARGIN, glyph);
}

/// The temperature (or "?") at the top right.
fn paste_masks(img: &mut Img, masks: &(Mask, Mask)) {
    let (glyph, outline) = masks;
    overlay(img, glyph, outline, W as i32 - glyph.w as i32 - EDGE_MARGIN);
}

/// Weekday and month/day at the top left, mirroring the temperature's margin.
fn paste_date(img: &mut Img, state: &AppState) {
    let now = state.now;
    let masks = digits::date_masks_cached(now.date_naive(), || {
        format!("{} {}/{}", now.format("%a"), now.month(), now.day()).to_uppercase()
    });
    let (glyph, outline) = &*masks;
    overlay(img, glyph, outline, EDGE_MARGIN);
}

/// Adds the 2 s cross-fade between conditions.
///
/// The picture is a pure function of (state, t); the only memory is the previously drawn
/// state, so a condition change can blend old into new. Time going backwards or jumping
/// more than 5 s (previews, a stalled loop) resets it instead of fading.
pub struct SceneRenderer {
    last: Option<(Option<Condition>, AppState, f64)>,
    fade: Option<(AppState, f64)>,
    /// The frame being drawn, and the old state's during a fade (kept to reuse their buffers).
    buf: Img,
    old_buf: Img,
}

impl Default for SceneRenderer {
    fn default() -> Self {
        SceneRenderer { last: None, fade: None, buf: Img::black(W, H), old_buf: Img::black(W, H) }
    }
}

impl SceneRenderer {
    pub fn new() -> Self {
        SceneRenderer::default()
    }

    pub fn render(&mut self, state: &AppState, t: f64) -> Img {
        self.render_into_buf(state, t);
        self.buf.clone()
    }

    fn render_into_buf(&mut self, state: &AppState, t: f64) {
        let cond = state.weather.as_ref().map(|w| w.current.condition());
        if let Some((last_cond, last_state, last_t)) = self.last.take() {
            if t < last_t || t - last_t > 5.0 {
                self.fade = None;
            } else if cond != last_cond {
                self.fade = Some((last_state, t));
            }
        }
        self.last = Some((cond, state.clone(), t)); // cheap: the snapshots are shared
        render_state_into(state, t, &mut self.buf);
        if let Some((old_state, t0)) = &self.fade {
            let alpha = (t - t0) / FADE_SECONDS;
            if alpha >= 1.0 {
                self.fade = None;
            } else {
                render_state_into(old_state, t, &mut self.old_buf);
                self.buf.blend_from(&self.old_buf, alpha);
            }
        }
    }

    pub fn draw(&mut self, frame: &mut Img, rect: Rect, state: &AppState, t: f64) {
        self.render_into_buf(state, t);
        let (w, h) = ((rect.w as usize).min(W), (rect.h as usize).min(H));
        frame.paste_part(&self.buf, w, h, rect.x, rect.y);
    }
}
