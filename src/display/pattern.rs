//! The hardware smoke-test picture. It makes the usual wiring problems obvious:
//!   - top half red-ish, bottom half blue-ish  -> a duplicated/missing half means the E jumper
//!   - white 1px border                         -> clipped or shifted edges
//!   - R, G, B bars across the middle           -> wrong color order (try rgb_sequence)
//!   - row ticks every 8 pixels on the left     -> missing or repeated rows
//!   - a small moving white square              -> flicker and tearing while animating

use crate::gfx::Img;

pub fn test_pattern(size: usize, t: f64) -> Img {
    let s = size as i32;
    let half = s / 2;
    let mut img = Img::black(size, size);
    img.fill_rect(0, 0, s - 1, half - 1, [40, 0, 0]);
    img.fill_rect(0, half, s - 1, s - 1, [0, 0, 40]);
    for (i, color) in [[255, 0, 0], [0, 255, 0], [0, 0, 255]].into_iter().enumerate() {
        let y = half - 6 + i as i32 * 4;
        img.fill_rect(8, y, s - 9, y + 3, color);
    }
    for y in (0..s).step_by(8) {
        img.line(1, y, if y % 16 != 0 { 4 } else { 7 }, y, [255, 255, 0]);
    }
    img.outline_rect(0, 0, s - 1, s - 1, [255, 255, 255]);
    let x = 6 + ((s - 18) as f64 * (0.5 + 0.5 * (t * 2.0).sin())) as i32;
    img.fill_rect(x, 6, x + 5, 11, [255, 255, 255]);
    img.fill_rect(x, s - 12, x + 5, s - 7, [255, 255, 255]);
    img
}

/// A rotating square (hzeller's demo 0): every row of the panel must be lit, with no duplicated half.
pub fn rotating_square(size: usize, t: f64) -> Img {
    let mut img = Img::black(size, size);
    let (c, ang) = (size as f64 / 2.0, t * 0.8);
    let half = size as f64 * 0.3;
    let (sin, cos) = ang.sin_cos();
    for y in 0..size {
        for x in 0..size {
            let (dx, dy) = (x as f64 + 0.5 - c, y as f64 + 0.5 - c);
            let (u, v) = (dx * cos + dy * sin, -dx * sin + dy * cos);
            if u.abs() <= half && v.abs() <= half {
                let hue = (u + half) / (2.0 * half);
                img.data[y * size + x] = [(255.0 * hue) as u8, (255.0 * (1.0 - hue)) as u8, 160];
            }
        }
    }
    img
}

/// Smooth colour pulses (demo 4): wrong colours (red and green swapped?) show up here.
pub fn pulse(size: usize, t: f64) -> Img {
    let level = |phase: f64| (127.5 + 127.5 * (t * 1.5 + phase).sin()) as u8;
    Img::new(size, size, [level(0.0), level(2.1), level(4.2)])
}

/// Sixteen grey steps (demo 5): banding or flicker shows up here.
pub fn gray_steps(size: usize) -> Img {
    let mut img = Img::black(size, size);
    for y in 0..size {
        for x in 0..size {
            let v = ((x * 16 / size) * 17) as u8;
            img.data[y * size + x] = [v, v, v];
        }
    }
    img
}

/// Pattern by name: `frame`, `square`, `pulse` or `gray`.
pub fn by_name(name: &str, size: usize, t: f64) -> Option<Img> {
    Some(match name {
        "frame" => test_pattern(size, t),
        "square" => rotating_square(size, t),
        "pulse" => pulse(size, t),
        "gray" => gray_steps(size),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn other_patterns_fill_the_whole_panel() {
        let sq = rotating_square(64, 0.0);
        assert_ne!(sq.get(32, 32), [0, 0, 0]);
        for y in [0usize, 63] {
            // rows at the very top and bottom are dark, but the middle band is lit end to end
            assert!((0..64).all(|x| sq.get(x, y) == [0, 0, 0]));
        }
        assert_ne!(pulse(64, 0.0), pulse(64, 1.0));
        let g = gray_steps(64);
        assert_eq!((g.get(0, 5), g.get(63, 5)), ([0, 0, 0], [255, 255, 255]));
        assert!(by_name("frame", 64, 0.0).is_some() && by_name("nope", 64, 0.0).is_none());
    }

    #[test]
    fn halves_bars_border_and_square() {
        let img = test_pattern(64, 0.0);
        assert_eq!(img.get(20, 3), [40, 0, 0]);
        assert_eq!(img.get(20, 60), [0, 0, 40]);
        assert_eq!(img.get(0, 20), [255, 255, 255]); // border
        assert_eq!(img.get(30, 26), [255, 0, 0]); // R bar
        assert_eq!(img.get(30, 30), [0, 255, 0]);
        assert_eq!(img.get(30, 34), [0, 0, 255]);
        assert_ne!(test_pattern(64, 0.0), test_pattern(64, 0.4)); // the square moves
    }
}
