//! Per-condition effects: rain, snow, fog bands and lightning. All pure functions of t.

use std::f64::consts::PI;
use std::rc::Rc;

use super::cache::Memo;
use super::palette::*;
use crate::gfx::{pyround, Img, Mask};
use rand::Rng;

const N_RAIN: usize = 44;
const N_SNOW: usize = 44;

/// WMO code -> intensity 0..1
pub fn rain_intensity(code: i64) -> f64 {
    match code {
        51 => 0.35, 53 => 0.45, 55 => 0.55, 56 => 0.4, 57 => 0.55, 61 => 0.55, 63 => 0.8, 65 => 1.0,
        66 => 0.7, 67 => 1.0, 80 => 0.6, 81 => 0.85, 82 => 1.0, 95 | 96 | 99 => 1.0,
        _ => 0.7,
    }
}

pub fn snow_intensity(code: i64) -> f64 {
    match code {
        71 => 0.5, 73 => 0.8, 75 => 1.0, 77 => 0.45, 85 => 0.7, 86 => 1.0,
        _ => 0.7,
    }
}

struct Drop {
    x0: f64,
    y0: f64,
    speed: f64,
    length: f64,
}

struct Flake {
    x0: f64,
    y0: f64,
    vy: f64,
    amp: f64,
    freq: f64,
    phase: f64,
    bright: bool,
}

thread_local! {
    static DROPS: Memo<(), Vec<Drop>> = Memo::new(1);
    static FLAKES: Memo<(), Vec<Flake>> = Memo::new(1);
    static FOG: Memo<(), (Mask, Mask)> = Memo::new(1);
}

fn rain_drops() -> Rc<Vec<Drop>> {
    DROPS.with(|m| {
        m.get((), || {
            let mut rng = crate::scene::rng(902);
            (0..N_RAIN)
                .map(|_| {
                    let x0 = rng.random_range(0.0..W as f64);
                    let y0 = rng.random_range(0.0..(H + 4) as f64);
                    let speed = rng.random_range(34.0..52.0);
                    let length = [2.0_f64, 3.0, 3.0][rng.random_range(0..3)];
                    Drop { x0, y0, speed, length }
                })
                .collect()
        })
    })
}

fn flakes() -> Rc<Vec<Flake>> {
    FLAKES.with(|m| {
        m.get((), || {
            let mut rng = crate::scene::rng(311);
            (0..N_SNOW)
                .map(|_| {
                    let x0 = rng.random_range(0.0..W as f64);
                    let y0 = rng.random_range(0.0..H as f64);
                    let vy = rng.random_range(4.0..9.0);
                    let amp = rng.random_range(0.5..2.0);
                    let freq = rng.random_range(0.6..1.8);
                    let phase = rng.random_range(0.0..std::f64::consts::TAU);
                    let bright = rng.random::<f64>() < 0.55;
                    Flake { x0, y0, vy, amp, freq, phase, bright }
                })
                .collect()
        })
    })
}

pub fn draw_rain(img: &mut Img, intensity: f64, wind: f64, t: f64, light: f64) {
    let n = 6.max(pyround(N_RAIN as f64 * intensity)) as usize;
    let slant = clamp(wind / 45.0, 0.0, 0.6);
    let col = lerp([70, 90, 140], [150, 180, 235], light);
    for d in rain_drops().iter().take(n) {
        let y = (d.y0 + d.speed * t).rem_euclid(H as f64 + d.length + 2.0) - d.length;
        let x = (d.x0 + slant * (d.speed * t)).rem_euclid(W as f64);
        img.line(x as i32, y as i32, (x - slant * d.length) as i32, (y - d.length) as i32, col);
    }
}

pub fn draw_snow(img: &mut Img, intensity: f64, wind: f64, t: f64) {
    let n = 8.max(pyround(N_SNOW as f64 * intensity)) as usize;
    let drift = clamp(wind / 30.0, 0.0, 1.5) * 2.5;
    for f in flakes().iter().take(n) {
        let y = (f.y0 + f.vy * t).rem_euclid(H as f64 + 2.0) - 1.0;
        let x = (f.x0 + drift * t + f.amp * (f.freq * t + f.phase).sin()).rem_euclid(W as f64);
        img.put(
            pyround(x) as i32,
            pyround(y) as i32,
            if f.bright { [250, 252, 255] } else { [160, 172, 195] },
        );
    }
}

/// Two horizontally periodic band masks (they wrap, so they can scroll forever).
fn fog_masks() -> Rc<(Mask, Mask)> {
    FOG.with(|m| {
        m.get((), || {
            let profile = [50.0, 105.0, 150.0, 105.0, 50.0];
            let mut out = Vec::new();
            for (seed, rows) in [(5u64, [13i32, 21, 29]), (9, [17, 25, 32])] {
                let mut rng = crate::scene::rng(seed);
                let mut mask = Mask::new(W, H, 0);
                let waves: Vec<(i32, f64)> =
                    rows.iter().map(|_| (rng.random_range(1..=3), rng.random_range(0.0..std::f64::consts::TAU))).collect();
                for (&(k, ph), &row) in waves.iter().zip(&rows) {
                    for x in 0..W {
                        let a = 0.65 + 0.35 * (2.0 * PI * k as f64 * x as f64 / W as f64 + ph).sin();
                        for (i, v) in profile.iter().enumerate() {
                            let y = row - 2 + i as i32;
                            if (0..H as i32).contains(&y) {
                                let cur = mask.get(x, y as usize);
                                mask.set(x as i32, y, cur.max(pyround(v * a) as u8));
                            }
                        }
                    }
                }
                out.push(mask);
            }
            let b = out.pop().unwrap();
            let a = out.pop().unwrap();
            (a, b)
        })
    })
}

pub fn draw_fog(img: &mut Img, light: f64, t: f64) {
    let color = lerp([34, 38, 54], [206, 212, 222], light);
    let masks = fog_masks();
    for (mask, speed) in [(&masks.0, 1.6), (&masks.1, -2.6)] {
        let dx = pyround(speed * t).rem_euclid(W as i64) as i32;
        img.paste_color(color, 0, 0, &mask.offset_x(dx));
    }
}

// --- lightning ---------------------------------------------------------------------------

const PERIOD: f64 = 5.0;

struct Strike {
    start: f64,
    pts: Vec<(i32, i32)>,
}

fn strike(slot: i64) -> Option<Strike> {
    let mut rng = crate::scene::rng((slot * 7919 + 13) as u64);
    if rng.random::<f64>() > 0.8 {
        return None;
    }
    let start = rng.random_range(0.4..PERIOD - 1.0);
    let mut x = rng.random_range(10..=54);
    let mut pts = vec![(x, 0)];
    let mut y = 0;
    while y < 27 {
        y += rng.random_range(2..=4);
        x += rng.random_range(-3..=3);
        pts.push((x, y));
    }
    Some(Strike { start, pts })
}

/// 0..1 brightness of the flash at time t (a main strike plus a quick flicker).
pub fn flash_level(t: f64) -> f64 {
    let slot = (t / PERIOD).floor() as i64;
    let Some(strike) = strike(slot) else {
        return 0.0;
    };
    let local = t - slot as f64 * PERIOD;
    let mut level: f64 = 0.0;
    for (delay, power) in [(0.0, 1.0), (0.18, 0.6)] {
        let dt = local - strike.start - delay;
        if (0.0..0.5).contains(&dt) {
            level = level.max(power * (-dt / 0.07).exp());
        }
    }
    level
}

pub fn draw_lightning(img: &mut Img, t: f64) {
    let level = flash_level(t);
    if level < 0.05 {
        return;
    }
    if level > 0.3 {
        if let Some(s) = strike((t / PERIOD).floor() as i64) {
            img.polyline(&s.pts, [255, 250, 200]);
        }
    }
    let step = pyround(level * 5.0) as u32;
    img.paste_color([235, 238, 255], 0, 0, &Mask::new(W, H, (step * 22).min(255) as u8));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flash_is_zero_between_strikes_and_bounded() {
        let mut seen_flash = false;
        for i in 0..2000 {
            let l = flash_level(i as f64 * 0.05);
            assert!((0.0..=1.0).contains(&l));
            seen_flash |= l > 0.5;
        }
        assert!(seen_flash, "some strike should land in 100 s");
    }

    #[test]
    fn intensity_tables_fall_back() {
        assert_eq!(rain_intensity(65), 1.0);
        assert_eq!(rain_intensity(1), 0.7);
        assert_eq!(snow_intensity(73), 0.8);
    }
}
