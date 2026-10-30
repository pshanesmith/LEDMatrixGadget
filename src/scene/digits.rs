//! Overlay text for the scene: the 3x5 temperature digits and the date, each with a dark outline.

use std::rc::Rc;

use chrono::NaiveDate;

use super::cache::Recent;
use crate::gfx::Mask;
use crate::ui::fonts::{glyph, glyph_width, SPACING};

fn digit_rows(ch: char) -> [&'static str; 5] {
    match ch {
        '0' => ["111", "101", "101", "101", "111"],
        '1' => ["010", "110", "010", "010", "111"],
        '2' => ["111", "001", "111", "100", "111"],
        '3' => ["111", "001", "111", "001", "111"],
        '4' => ["101", "101", "111", "001", "001"],
        '5' => ["111", "100", "111", "001", "111"],
        '6' => ["111", "100", "111", "101", "111"],
        '7' => ["111", "001", "001", "001", "001"],
        '8' => ["111", "101", "111", "101", "111"],
        '9' => ["111", "101", "111", "001", "111"],
        '-' => ["000", "000", "111", "000", "000"],
        _ => ["111", "001", "010", "000", "010"], // '?'
    }
}

fn outline_of(glyph: &Mask) -> Mask {
    glyph.max_filter3().map(|v| (v as u32 * 115 / 255) as u8)
}

/// (glyph mask, outline mask) for `text` plus a trailing degree dot, with a 1 px margin.
pub fn text_masks(text: &str) -> (Mask, Mask) {
    let glyphs: Vec<[&str; 5]> = text.chars().map(digit_rows).collect();
    let width = 4 * glyphs.len() + 1; // glyphs, gap, degree dot
    let mut mask = Mask::new(width + 2, 7, 0);
    for (i, rows) in glyphs.iter().enumerate() {
        for (y, row) in rows.iter().enumerate() {
            for (x, bit) in row.bytes().enumerate() {
                if bit == b'1' {
                    mask.set((1 + 4 * i + x) as i32, (1 + y) as i32, 255);
                }
            }
        }
    }
    mask.set(width as i32, 1, 255); // degree dot, top right
    let outline = outline_of(&mask);
    (mask, outline)
}

/// (glyph mask, outline mask) for `text` in the UI font, with the same 1 px margin as the temperature.
pub fn date_masks(text: &str) -> (Mask, Mask) {
    let glyphs: Vec<_> = text.chars().map(glyph).collect();
    let n = glyphs.len() as i32;
    let width: i32 = glyphs.iter().map(|g| glyph_width(g)).sum::<i32>() + SPACING * (n - 1).max(0);
    let mut mask = Mask::new((width + 2) as usize, 7, 0);
    let mut x = 1;
    for g in glyphs {
        for (y, row) in g.iter().enumerate() {
            for (dx, on) in row.bytes().enumerate() {
                if on == b'#' {
                    mask.set(x + dx as i32, 1 + y as i32, 255);
                }
            }
        }
        x += glyph_width(g) + SPACING;
    }
    let outline = outline_of(&mask);
    (mask, outline)
}

thread_local! {
    // A few of each: during a cross-fade the old and the new state are drawn in turn.
    static TEXT: Recent<String, (Mask, Mask)> = const { Recent::new(4) };
    static DATE: Recent<NaiveDate, (Mask, Mask)> = const { Recent::new(4) };
    static TEMPERATURE: Recent<i64, (Mask, Mask)> = const { Recent::new(4) };
}

/// `text_masks(&format_temperature(value))`, remembered by the rounded value.
pub fn temperature_masks_cached(value: f64) -> Rc<(Mask, Mask)> {
    let degrees = crate::gfx::pyround(value);
    TEMPERATURE.with(|m| m.get_by(|d| *d == degrees, || (degrees, text_masks(&degrees.to_string()))))
}

/// `text_masks`, remembered: the temperature changes a few times an hour, not every frame.
pub fn text_masks_cached(text: &str) -> Rc<(Mask, Mask)> {
    TEXT.with(|m| m.get_by(|k| k == text, || (text.to_string(), text_masks(text))))
}

/// `date_masks(&make_text())`, remembered per day, so the text is only formatted when the day changes.
pub fn date_masks_cached(day: NaiveDate, make_text: impl FnOnce() -> String) -> Rc<(Mask, Mask)> {
    DATE.with(|m| m.get_by(|d| *d == day, || (day, date_masks(&make_text()))))
}

pub fn format_temperature(value: f64) -> String {
    crate::gfx::pyround(value).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_mask_has_degree_dot_and_margin() {
        let (g, o) = text_masks("72");
        assert_eq!((g.w, g.h), (4 * 2 + 1 + 2, 7));
        assert_eq!(g.get(g.w - 2, 1), 255); // degree dot
        assert_eq!(g.get(0, 0), 0);
        assert_eq!(o.get(0, 0), 115); // outline reaches into the margin
        assert!(o.data.iter().all(|v| *v == 0 || *v == 115));
    }

    #[test]
    fn rounds_like_python() {
        assert_eq!(format_temperature(61.5), "62");
        assert_eq!(format_temperature(62.5), "62");
        assert_eq!(format_temperature(-8.4), "-8");
    }
}
