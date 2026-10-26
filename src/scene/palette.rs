//! Colour helpers, time-of-day maths and sky palette for the weather scene.

use chrono::Duration;

use crate::gfx::{pyround, Rgb};
use crate::state::{Condition, Dt};

pub const W: usize = 64;
pub const H: usize = 35;

/// How much each condition greys the sky and terrain (0 = untouched); `None` is "no data yet".
pub fn gray_mix(cond: Option<Condition>) -> f64 {
    match cond {
        Some(Condition::Clear) => 0.0,
        Some(Condition::PartlyCloudy) => 0.12,
        Some(Condition::Overcast) => 0.7,
        Some(Condition::Fog) => 0.6,
        Some(Condition::Rain) => 0.75,
        Some(Condition::Snow) => 0.65,
        Some(Condition::Thunder) => 0.85,
        None => 0.6,
    }
}

pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    }
}

pub fn clamp01(x: f64) -> f64 {
    clamp(x, 0.0, 1.0)
}

pub fn lerp(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = clamp01(t);
    let mut out = [0u8; 3];
    for k in 0..3 {
        let (x, y) = (a[k] as f64, b[k] as f64);
        out[k] = pyround(x + (y - x) * t).clamp(0, 255) as u8;
    }
    out
}

pub fn scale(c: Rgb, f: f64) -> Rgb {
    let mut out = [0u8; 3];
    for k in 0..3 {
        out[k] = pyround(c[k] as f64 * f).clamp(0, 255) as u8;
    }
    out
}

pub fn gray_of(c: Rgb) -> Rgb {
    let g = pyround(0.3 * c[0] as f64 + 0.59 * c[1] as f64 + 0.11 * c[2] as f64) as u8;
    [g, g, g]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sun {
    /// -1..1; 0 on the horizon, +1 noon, -1 midnight.
    pub elev: f64,
    /// 0..1 along the sun's arc (day) or the moon's arc (night).
    pub progress: f64,
}

fn secs(d: Duration) -> f64 {
    d.num_microseconds().map_or(d.num_milliseconds() as f64 / 1e3, |us| us as f64 / 1e6)
}

/// Where the sun (day) or moon (night) is. `is_day` from the API wins when it clearly disagrees
/// with the clock (sample data, stale cache), except within 30 min of a boundary.
pub fn sun_position(now: Dt, is_day: bool, sunrise: Dt, sunset: Dt) -> Sun {
    let now = now.with_timezone(&sunrise.timezone());
    let shift = Duration::days(now.date_naive().signed_duration_since(sunrise.date_naive()).num_days());
    let (rise, sset) = (sunrise + shift, sunset + shift);
    let day = Duration::days(1);
    let (clock, clock_day) = if rise <= now && now <= sset {
        let p = secs(now - rise) / secs(sset - rise);
        (Sun { elev: (std::f64::consts::PI * p).sin(), progress: p }, true)
    } else {
        let (start, end) = if now < rise { (sset - day, rise) } else { (sset, rise + day) };
        let q = secs(now - start) / secs(end - start);
        (Sun { elev: -(std::f64::consts::PI * clamp01(q)).sin(), progress: clamp01(q) }, false)
    };
    let near = (now - rise).abs().min((now - sset).abs()) < Duration::minutes(30);
    if clock_day != is_day && !near {
        return if is_day { Sun { elev: 1.0, progress: 0.5 } } else { Sun { elev: -1.0, progress: 0.5 } };
    }
    clock
}

/// 0 in full night, 1 in full day, smooth through twilight.
pub fn light_level(elev: f64) -> f64 {
    clamp01((elev + 0.08) / 0.45)
}

/// 1 right at sunrise/sunset, fading to 0 about 0.2 away in elevation.
pub fn twilight(elev: f64) -> f64 {
    clamp01(1.0 - (elev - 0.02).abs() / 0.22)
}

// (elevation, sky top, sky horizon)
const SKY: [(f64, Rgb, Rgb); 5] = [
    (-0.40, [4, 6, 24], [10, 14, 40]),
    (-0.12, [14, 14, 50], [62, 36, 84]),
    (0.00, [40, 50, 110], [255, 140, 70]),
    (0.12, [70, 110, 190], [255, 190, 130]),
    (0.35, [56, 124, 214], [150, 200, 245]),
];

pub fn sky_colors(elev: f64) -> (Rgb, Rgb) {
    if elev <= SKY[0].0 {
        return (SKY[0].1, SKY[0].2);
    }
    for pair in SKY.windows(2) {
        let ((e0, t0, b0), (e1, t1, b1)) = (pair[0], pair[1]);
        if elev <= e1 {
            let f = (elev - e0) / (e1 - e0);
            return (lerp(t0, t1, f), lerp(b0, b1, f));
        }
    }
    (SKY[4].1, SKY[4].2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples::{sample_state, NOON};

    #[test]
    fn lerp_clamps_and_rounds() {
        assert_eq!(lerp([0, 0, 0], [255, 255, 255], 2.0), [255, 255, 255]);
        assert_eq!(lerp([0, 0, 0], [255, 255, 255], 0.5), [128, 128, 128]); // 127.5 rounds to even
    }

    #[test]
    fn noon_sun_is_high_and_midnight_moon_is_low() {
        let w = sample_state("clear_day").weather.unwrap();
        let noon = sun_position(*NOON, true, w.daily.sunrise, w.daily.sunset);
        assert!(noon.elev > 0.9);
        let midnight = *NOON + Duration::hours(12);
        let moon = sun_position(midnight, false, w.daily.sunrise, w.daily.sunset);
        assert!(moon.elev < -0.9);
    }

    #[test]
    fn api_day_flag_wins_away_from_the_boundaries() {
        let w = sample_state("clear_day").weather.unwrap();
        let s = sun_position(*NOON + Duration::hours(12), true, w.daily.sunrise, w.daily.sunset);
        assert_eq!(s, Sun { elev: 1.0, progress: 0.5 });
    }
}
