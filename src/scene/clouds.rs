//! Drifting clouds (clear / partly cloudy) and the continuous overcast layers.

use std::rc::Rc;

use super::cache::Memo;
use super::palette::*;
use crate::gfx::{pyround, Img, Mask, Rgb};
use crate::pyrandom::PyRandom;
use crate::state::Condition;

type Sprite = (Img, Mask);
type Deck = (Img, Mask, f64);

thread_local! {
    static SPRITES: Memo<(i32, Condition), Vec<Sprite>> = Memo::new(64);
    static LAYERS: Memo<(i32, Condition), Vec<Deck>> = Memo::new(32);
}

/// (light, shade) by condition in full daylight.
fn day_colors(cond: Condition) -> (Rgb, Rgb) {
    match cond {
        Condition::Clear | Condition::PartlyCloudy => ([240, 244, 252], [186, 198, 216]),
        Condition::Overcast | Condition::Fog => ([168, 174, 186], [128, 134, 148]),
        Condition::Rain => ([150, 156, 170], [108, 114, 130]),
        Condition::Snow => ([158, 164, 178], [116, 122, 138]),
        Condition::Thunder => ([92, 94, 112], [58, 60, 76]),
    }
}

struct Shape {
    w: i32,
    h: i32,
    puffs: &'static [(i32, i32, i32, i32)], // (cx, cy, rx, ry)
}

const SHAPES: [Shape; 3] = [
    Shape { w: 20, h: 7, puffs: &[(5, 5, 4, 3), (10, 4, 5, 4), (15, 5, 4, 3), (10, 6, 8, 3)] },
    Shape { w: 14, h: 6, puffs: &[(4, 4, 3, 3), (8, 3, 4, 3), (11, 4, 3, 3), (7, 5, 6, 2)] },
    Shape { w: 24, h: 8, puffs: &[(5, 6, 4, 3), (10, 4, 5, 4), (16, 5, 5, 4), (20, 6, 3, 3), (12, 7, 10, 3)] },
];

/// The sky is taller than it was; keep the clouds off the date row.
pub const CLOUD_DROP: i32 = 4;

// (shape, y, x0, speed px/s)
const DRIFTERS: [(usize, i32, f64, f64); 7] = [
    (0, 3, 4.0, 2.0), (2, 9, 40.0, 1.4), (1, 6, 22.0, 2.6), (0, 12, 58.0, 1.7),
    (1, 2, 50.0, 2.2), (2, 5, 8.0, 1.2), (1, 10, 30.0, 2.9),
];

pub fn palette(eq: i32, cond: Condition) -> (Rgb, Rgb) {
    let elev = eq as f64 / 40.0;
    let (light, shade) = day_colors(cond);
    let (lv, tw) = (light_level(elev), twilight(elev));
    let tone = |col: Rgb| {
        let night = lerp(scale(col, 0.2), [30, 40, 70], 0.25);
        lerp(lerp(night, col, lv), [255, 175, 135], 0.35 * tw)
    };
    (tone(light), tone(shade))
}

fn sprites(eq: i32, cond: Condition) -> Rc<Vec<Sprite>> {
    SPRITES.with(|m| {
        m.get((eq, cond), || {
            let (light, shade) = palette(eq, cond);
            SHAPES
                .iter()
                .map(|s| {
                    let mut mask = Mask::new(s.w as usize, s.h as usize, 0);
                    for &(cx, cy, rx, ry) in s.puffs {
                        mask.fill_ellipse(cx - rx, cy - ry, cx + rx, cy + ry, 255);
                    }
                    let mut rgb = Img::new(s.w as usize, s.h as usize, shade);
                    rgb.fill_rect(0, 0, s.w, pyround(s.h as f64 * 0.55) as i32, light);
                    (rgb, mask)
                })
                .collect()
        })
    })
}

/// Quantised so the drift speed (and so every position) rarely jumps between refreshes.
pub fn wind_factor(wind: f64) -> f64 {
    if wind < 8.0 {
        1.0
    } else if wind < 20.0 {
        1.6
    } else if wind < 35.0 {
        2.4
    } else {
        3.2
    }
}

pub fn drifting_count(cond: Condition, cloud_cover: i64) -> usize {
    match cond {
        Condition::PartlyCloudy => 3.max(pyround(cloud_cover as f64 / 100.0 * 7.0)) as usize,
        Condition::Clear => {
            if cloud_cover < 12 {
                0
            } else {
                3.min(pyround(cloud_cover as f64 / 100.0 * 6.0)) as usize
            }
        }
        _ => 0,
    }
}

pub fn draw_drifters(img: &mut Img, eq: i32, cond: Condition, count: usize, wind: f64, t: f64) {
    if count == 0 {
        return;
    }
    let sheet = sprites(eq, cond);
    let wf = wind_factor(wind);
    for &(shape, y, x0, speed) in DRIFTERS.iter().take(count) {
        let (rgb, mask) = &sheet[shape];
        let span = (W + rgb.w) as f64;
        let x = pyround((x0 + speed * wf * t).rem_euclid(span)) as i32 - rgb.w as i32;
        img.paste(rgb, x, y + CLOUD_DROP, Some(mask));
    }
}

/// Wrap-around cloud decks for overcast-ish conditions: (rgb, mask, speed px/s) back to front.
fn layers(eq: i32, cond: Condition) -> Rc<Vec<Deck>> {
    LAYERS.with(|m| {
        m.get((eq, cond), || {
            let (light, shade) = palette(eq, cond);
            let mut decks = Vec::new();
            for (seed, rows, speed, tint) in [(31u64, 7i32, 1.5, 0.0), (47, 10, 2.8, 0.3)] {
                let mut rng = PyRandom::new(seed);
                let (w, h) = (W as i32, rows + 3);
                let mut mask = Mask::new(w as usize, h as usize, 0);
                let mut rgb = Img::new(w as usize, h as usize, shade);
                let body = lerp(shade, scale(shade, 0.72), tint);
                let top = lerp(light, shade, tint);
                let n = 11;
                for k in 0..n {
                    let cx = k as f64 * W as f64 / n as f64 + rng.uniform(-2.0, 2.0);
                    let cy = rng.uniform((rows - 7) as f64, (rows - 1) as f64);
                    let r = rng.uniform(2.5, 5.5);
                    for off in [-w, 0, w] {
                        let (x0, y0) = (cx + off as f64 - r, cy - r * 0.8);
                        let (x1, y1) = (cx + off as f64 + r, cy + r * 0.8);
                        mask.fill_ellipse(x0 as i32, y0 as i32, x1 as i32, y1 as i32, 255);
                        rgb.fill_ellipse(x0 as i32, y0 as i32, x1 as i32, y1 as i32, body);
                        rgb.fill_ellipse((x0 + 1.0) as i32, y0 as i32, (x1 - 1.0) as i32, (y1 - r * 0.7) as i32, top);
                    }
                }
                mask.fill_rect(0, 0, w, 0.max(rows - 8), 255);
                rgb.fill_rect(0, 0, w, 0.max(rows - 8), top);
                decks.push((rgb, mask, speed));
            }
            decks
        })
    })
}

pub fn draw_layers(img: &mut Img, eq: i32, cond: Condition, wind: f64, t: f64) {
    let wf = wind_factor(wind);
    for (rgb, mask, speed) in layers(eq, cond).iter() {
        let off = pyround(speed * wf * t).rem_euclid(W as i64) as i32;
        for x in [off - W as i32, off] {
            img.paste(rgb, x, 0, Some(mask));
        }
    }
}
