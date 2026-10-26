//! Sky gradient, stars, sun and moon.

use std::f64::consts::PI;
use std::rc::Rc;

use chrono::{TimeZone, Utc};

use super::cache::Memo;
use super::palette::*;
use crate::gfx::{pyround, Img, Mask};
use crate::scene_rng::SceneRng;
use crate::state::{Condition, Dt};

/// Sun/moon centre sits here at sunrise/sunset.
pub const HORIZON: i32 = 28;
/// And here at its highest.
pub const PEAK: i32 = 8;

thread_local! {
    static SKY: Memo<(i32, Option<Condition>), Img> = Memo::new(96);
    static STARS: Memo<(), Vec<Star>> = Memo::new(1);
}

pub fn sky_image(eq: i32, cond: Option<Condition>) -> Rc<Img> {
    SKY.with(|m| m.get((eq, cond), || build_sky(eq, cond)))
}

fn build_sky(eq: i32, cond: Option<Condition>) -> Img {
    let elev = eq as f64 / 40.0;
    let (mut top, mut bottom) = sky_colors(elev);
    match cond {
        None => {
            // no data yet: dim neutral sky
            top = [22, 26, 38];
            bottom = [44, 50, 66];
        }
        Some(c) => {
            let light = light_level(elev);
            let gray = lerp([16, 18, 28], [128, 136, 150], light);
            let mix = gray_mix(cond);
            let dark = if c == Condition::Thunder { 0.82 } else { 1.0 };
            top = scale(lerp(top, gray, mix), dark);
            bottom = scale(lerp(bottom, lerp(gray, [150, 156, 168], 0.3 * light), mix), dark);
        }
    }
    let mut img = Img::black(W, H);
    for y in 0..H {
        let c = lerp(top, bottom, (y as f64 / (HORIZON + 1) as f64).powf(1.3));
        for x in 0..W {
            img.data[y * W + x] = c;
        }
    }
    img
}

#[derive(Clone, Copy)]
struct Star {
    x: i32,
    y: i32,
    base: f64,
    phase: f64,
}

fn stars() -> Rc<Vec<Star>> {
    STARS.with(|m| {
        m.get((), || {
            let mut rng = SceneRng::new(1717);
            (0..26)
                .map(|_| {
                    let x = rng.randrange(0, W as i32);
                    let y = rng.randrange(0, 22);
                    let base = rng.uniform(0.5, 1.0);
                    let phase = rng.uniform(0.0, 6.28);
                    Star { x, y, base, phase }
                })
                .collect()
        })
    })
}

/// Stars twinkle at their own rates (periods of about 2-6 s) and dip to nearly dark.
pub fn draw_stars(img: &mut Img, visibility: f64, t: f64) {
    if visibility <= 0.02 {
        return;
    }
    for s in stars().iter() {
        let rate = 1.0 + (s.phase * 3.1).rem_euclid(2.0);
        let wave = 0.5 + 0.5 * (t * rate + s.phase).sin();
        let b = visibility * s.base * (0.1 + 0.9 * wave.powf(1.5));
        let v = pyround(28.0 + 215.0 * clamp01(b)) as i32;
        img.put(s.x, s.y, [v as u8, v as u8, (v + 25).min(255) as u8]);
    }
}

/// Seconds between shooting stars.
pub const SHOOTING_PERIOD: f64 = 23.0;
/// Seconds a streak is visible.
pub const SHOOTING_LENGTH: f64 = 1.1;

/// An occasional streak across the upper sky; position varies per cycle, all derived from t.
pub fn draw_shooting_star(img: &mut Img, visibility: f64, t: f64) {
    if visibility < 0.5 {
        return;
    }
    let cycle = (t / SHOOTING_PERIOD).floor();
    let local = t - cycle * SHOOTING_PERIOD;
    if local > SHOOTING_LENGTH {
        return;
    }
    let mut rng = SceneRng::new((cycle as i64 * 7919 + 13) as u64);
    let mut x0 = rng.uniform(6.0, 40.0);
    let y0 = rng.uniform(1.0, 12.0);
    let dx = 30.0 * rng.choice(&[1.0, 1.0, -1.0]);
    let dy = rng.uniform(7.0, 13.0);
    if dx < 0.0 {
        x0 += 20.0;
    }
    for k in 0..5 {
        // head plus a fading tail
        let tt = local - k as f64 * 0.035;
        if tt < 0.0 {
            continue;
        }
        let fade = (1.0 - k as f64 / 5.0) * f64::min(1.0, (SHOOTING_LENGTH - local) / 0.4);
        let v = pyround(60.0 + 195.0 * clamp01(fade * visibility)) as i32;
        let (px, py) = (pyround(x0 + dx * tt) as i32, pyround(y0 + dy * tt) as i32);
        if (0..W as i32).contains(&px) && (0..HORIZON).contains(&py) {
            img.put(px, py, [v as u8, v as u8, (v + 20).min(255) as u8]);
        }
    }
}

pub fn arc_position(progress: f64) -> (i32, i32) {
    (
        pyround(6.0 + progress * 52.0) as i32,
        pyround(HORIZON as f64 - (PI * progress).sin() * (HORIZON - PEAK) as f64) as i32,
    )
}

pub fn draw_sun(img: &mut Img, progress: f64, elev: f64, hide: f64, t: f64) {
    if hide >= 0.97 {
        return;
    }
    let (x, y) = arc_position(progress);
    let sky = img.put_checked(x, y).unwrap_or([0, 0, 0]);
    let core = lerp([255, 120, 40], [255, 236, 130], elev / 0.3);
    let glow = lerp(sky, core, 0.2 * (1.0 - hide));
    for dy in -4..=4 {
        for dx in -4..=4 {
            let d = dx * dx + dy * dy;
            if 6 < d && d <= 14 {
                img.put(x + dx, y + dy, glow);
            }
        }
    }
    // sparkle: eight rays that alternate on and off twice a second, fading with cloud cover
    let ray = lerp(sky, core, 0.55 * (1.0 - hide));
    let step = (t * 2.0) as i64;
    for k in 0..8i64 {
        if (k + step).rem_euclid(2) == 0 {
            let ang = k as f64 * PI / 4.0;
            img.put(x + pyround(5.0 * ang.cos()) as i32, y + pyround(5.0 * ang.sin()) as i32, ray);
        }
    }
    let body = lerp(core, sky, hide);
    for dy in -2..=2i32 {
        let half = if dy.abs() == 2 { 1 } else { 2 };
        img.line(x - half, y + dy, x + half, y + dy, body);
    }
}

/// 0 new, 0.5 full (mean synodic month from the 2000-01-06 new moon).
pub fn moon_phase(now: Dt) -> f64 {
    let new_moon = Utc.with_ymd_and_hms(2000, 1, 6, 18, 14, 0).unwrap();
    let ms = (now.with_timezone(&Utc) - new_moon).num_milliseconds() as f64;
    let days = ms / 1000.0 / 86400.0;
    days.rem_euclid(29.530588) / 29.530588
}

fn moon_sprite(phase_q: i32) -> (Img, Mask) {
    let ph = 2.0 * PI * phase_q as f64 / 32.0;
    let k = ph.cos();
    let waxing = phase_q < 16;
    let mut rgb = Img::black(7, 7);
    let mut mask = Mask::new(7, 7, 0);
    for py in 0..7i32 {
        for px in 0..7i32 {
            let (dx, dy) = (px - 3, py - 3);
            if dx * dx + dy * dy > 10 {
                continue;
            }
            let (xn, yn) = (dx as f64 / 3.2, dy as f64 / 3.2);
            let w = (1.0 - yn * yn).max(0.0).sqrt();
            let lit = if waxing { xn > k * w } else { xn < -k * w };
            if lit {
                rgb.put(px, py, [232, 232, 205]);
                mask.set(px, py, 255);
            } else {
                rgb.put(px, py, [36, 42, 66]);
                mask.set(px, py, 110);
            }
        }
    }
    (rgb, mask)
}

pub fn draw_moon(img: &mut Img, progress: f64, phase: f64, hide: f64) {
    if hide >= 0.97 {
        return;
    }
    let (x, y) = arc_position(progress);
    let (rgb, mask) = moon_sprite((pyround(phase * 32.0) % 32) as i32);
    let faded = mask.map(|v| pyround(v as f64 * (1.0 - hide)) as u8);
    img.paste(&rgb, x - 3, y - 3, Some(&faded));
}
