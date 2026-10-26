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
    let Some(weather) = &state.weather else {
        let mut img = (*sky::sky_image(NEUTRAL_EQ, None)).clone();
        let terrain = mountains::terrain(NEUTRAL_EQ, None, season(state));
        img.paste(&terrain.0, 0, 0, Some(&terrain.1));
        paste_text(&mut img, "?");
        paste_date(&mut img, state);
        return img;
    };

    let cur = &weather.current;
    let cond = cur.condition();
    let pos = sun_position(state.now, cur.is_day, weather.daily.sunrise, weather.daily.sunset);
    let eq = pyround(pos.elev * 40.0) as i32;
    let light = light_level(eq as f64 / 40.0);
    let mut img = (*sky::sky_image(eq, Some(cond))).clone();

    // celestial body, then clouds that can cover it
    let cloud = cur.cloud_cover as f64;
    let hide = match cond {
        Condition::Rain | Condition::Snow | Condition::Thunder => 1.0,
        Condition::Overcast | Condition::Fog => 0.92,
        _ => clamp01((cloud - 50.0) / 50.0),
    };
    if pos.elev >= 0.0 {
        sky::draw_sun(&mut img, pos.progress, pos.elev, hide, t);
    } else {
        let clear_sky = if PRECIP_SKY.contains(&cond) { 0.0 } else { clamp01(1.0 - cloud / 70.0) };
        let star_vis = clamp01((-0.05 - pos.elev) / 0.25) * clear_sky;
        sky::draw_stars(&mut img, star_vis, t);
        sky::draw_shooting_star(&mut img, star_vis, t);
        sky::draw_moon(&mut img, pos.progress, sky::moon_phase(state.now), hide);
    }

    let mut drifters = clouds::drifting_count(cond, cur.cloud_cover);
    if cond == Condition::Clear {
        drifters = drifters.max(1); // one wisp keeps a clear sky from looking frozen
    }
    clouds::draw_drifters(&mut img, eq, cond, drifters, cur.wind_speed, t);
    if PRECIP_SKY.contains(&cond) && cond != Condition::Fog {
        clouds::draw_layers(&mut img, eq, cond, cur.wind_speed, t);
    }

    let terrain = mountains::terrain(eq, Some(cond), season(state));
    img.paste(&terrain.0, 0, 0, Some(&terrain.1));

    match cond {
        Condition::Fog => particles::draw_fog(&mut img, light, t),
        Condition::Rain => {
            particles::draw_rain(&mut img, particles::rain_intensity(cur.code), cur.wind_speed, t, light)
        }
        Condition::Thunder => {
            particles::draw_rain(&mut img, 1.0, cur.wind_speed, t, light);
            particles::draw_lightning(&mut img, t);
        }
        Condition::Snow => {
            particles::draw_snow(&mut img, particles::snow_intensity(cur.code), cur.wind_speed, t)
        }
        _ => {}
    }

    paste_text(&mut img, &digits::format_temperature(cur.temperature));
    paste_date(&mut img, state);
    img
}

fn overlay(img: &mut Img, glyph: &Mask, outline: &Mask, x: i32) {
    img.paste_color([0, 0, 0], x, EDGE_MARGIN, outline);
    img.paste_color([255, 255, 255], x, EDGE_MARGIN, glyph);
}

fn paste_text(img: &mut Img, text: &str) {
    let (glyph, outline) = digits::text_masks(text);
    overlay(img, &glyph, &outline, W as i32 - glyph.w as i32 - EDGE_MARGIN);
}

/// Weekday and month/day at the top left, mirroring the temperature's margin.
fn paste_date(img: &mut Img, state: &AppState) {
    let now = state.now;
    let text = format!("{} {}/{}", now.format("%a"), now.month(), now.day()).to_uppercase();
    let (glyph, outline) = digits::date_masks(&text);
    overlay(img, &glyph, &outline, EDGE_MARGIN);
}

/// Adds the 2 s cross-fade between conditions.
///
/// The picture is a pure function of (state, t); the only memory is the previously drawn
/// state, so a condition change can blend old into new. Time going backwards or jumping
/// more than 5 s (previews, a stalled loop) resets it instead of fading.
#[derive(Default)]
pub struct SceneRenderer {
    last: Option<(Option<Condition>, AppState, f64)>,
    fade: Option<(AppState, f64)>,
}

impl SceneRenderer {
    pub fn new() -> Self {
        SceneRenderer::default()
    }

    pub fn render(&mut self, state: &AppState, t: f64) -> Img {
        let cond = state.weather.as_ref().map(|w| w.current.condition());
        if let Some((last_cond, last_state, last_t)) = self.last.take() {
            if t < last_t || t - last_t > 5.0 {
                self.fade = None;
            } else if cond != last_cond {
                self.fade = Some((last_state, t));
            }
        }
        self.last = Some((cond, state.clone(), t));
        let mut img = render_state(state, t);
        if let Some((old_state, t0)) = &self.fade {
            let alpha = (t - t0) / FADE_SECONDS;
            if alpha >= 1.0 {
                self.fade = None;
            } else {
                img = Img::blend(&render_state(old_state, t), &img, alpha);
            }
        }
        img
    }

    pub fn draw(&mut self, frame: &mut Img, rect: Rect, state: &AppState, t: f64) {
        let img = self.render(state, t);
        let (w, h) = ((rect.w as usize).min(W), (rect.h as usize).min(H));
        let mut crop = Img::black(w, h);
        for y in 0..h {
            for x in 0..w {
                crop.data[y * w + x] = img.get(x, y);
            }
        }
        frame.paste(&crop, rect.x, rect.y, None);
    }
}
