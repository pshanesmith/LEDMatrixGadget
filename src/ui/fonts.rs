//! Tiny proportional bitmap font: 5 rows tall, 3 to 5 columns wide, no antialiasing.
//!
//! Glyphs are written as rows of '#'/'.'. Callers pass upper-case text; '^' and 'v' are
//! the up/down arrows used by the temperature row.

use crate::gfx::{Img, Rgb};

pub const HEIGHT: i32 = 5;
pub const SPACING: i32 = 1;

pub type Glyph = [&'static str; 5];

const UNKNOWN: Glyph = ["##.", "..#", ".#.", "...", ".#."];

pub fn glyph(ch: char) -> &'static Glyph {
    match ch {
        '0' => &["###", "#.#", "#.#", "#.#", "###"],
        '1' => &[".#.", "##.", ".#.", ".#.", "###"],
        '2' => &["###", "..#", "###", "#..", "###"],
        '3' => &["###", "..#", "###", "..#", "###"],
        '4' => &["#.#", "#.#", "###", "..#", "..#"],
        '5' => &["###", "#..", "###", "..#", "###"],
        '6' => &["###", "#..", "###", "#.#", "###"],
        '7' => &["###", "..#", "..#", ".#.", ".#."],
        '8' => &["###", "#.#", "###", "#.#", "###"],
        '9' => &["###", "#.#", "###", "..#", "###"],
        'A' => &[".#.", "#.#", "###", "#.#", "#.#"],
        'B' => &["##.", "#.#", "##.", "#.#", "##."],
        'C' => &[".##", "#..", "#..", "#..", ".##"],
        'D' => &["##.", "#.#", "#.#", "#.#", "##."],
        'E' => &["###", "#..", "##.", "#..", "###"],
        'F' => &["###", "#..", "##.", "#..", "#.."],
        'G' => &[".##", "#..", "#.#", "#.#", ".##"],
        'H' => &["#.#", "#.#", "###", "#.#", "#.#"],
        'I' => &["###", ".#.", ".#.", ".#.", "###"],
        'J' => &["..#", "..#", "..#", "#.#", ".#."],
        'K' => &["#.#", "#.#", "##.", "#.#", "#.#"],
        'L' => &["#..", "#..", "#..", "#..", "###"],
        'M' => &["#...#", "##.##", "#.#.#", "#...#", "#...#"],
        'N' => &["#..#", "##.#", "#.##", "#..#", "#..#"],
        'O' => &[".#.", "#.#", "#.#", "#.#", ".#."],
        'P' => &["##.", "#.#", "##.", "#..", "#.."],
        'Q' => &[".#.", "#.#", "#.#", "###", ".##"],
        'R' => &["##.", "#.#", "##.", "#.#", "#.#"],
        'S' => &[".##", "#..", ".#.", "..#", "##."],
        'T' => &["###", ".#.", ".#.", ".#.", ".#."],
        'U' => &["#.#", "#.#", "#.#", "#.#", "###"],
        'V' => &["#.#", "#.#", "#.#", "#.#", ".#."],
        'W' => &["#...#", "#...#", "#.#.#", "##.##", "#...#"],
        'X' => &["#.#", "#.#", ".#.", "#.#", "#.#"],
        'Y' => &["#.#", "#.#", ".#.", ".#.", ".#."],
        'Z' => &["###", "..#", ".#.", "#..", "###"],
        ' ' => &["..", "..", "..", "..", ".."],
        '/' => &["..#", "..#", ".#.", "#..", "#.."],
        ':' => &[".", "#", ".", "#", "."],
        '.' => &[".", ".", ".", ".", "#"],
        '-' => &["...", "...", "###", "...", "..."],
        '+' => &["...", ".#.", "###", ".#.", "..."],
        '?' => &UNKNOWN,
        '^' => &["..#..", ".###.", "#.#.#", "..#..", "..#.."],
        'v' => &["..#..", "..#..", "#.#.#", ".###.", "..#.."],
        _ => &UNKNOWN,
    }
}

/// Width in pixels of one glyph.
pub fn glyph_width(g: &Glyph) -> i32 {
    g[0].len() as i32
}

/// Pixel width of `text`, without trailing spacing.
pub fn text_width(text: &str) -> i32 {
    let mut n = 0;
    let mut total = 0;
    for ch in text.chars() {
        total += glyph_width(glyph(ch));
        n += 1;
    }
    if n == 0 {
        0
    } else {
        total + SPACING * (n - 1)
    }
}

/// Draw `text` with its top-left at (x, y); pixels outside the frame are skipped.
/// Returns the x just past the last glyph.
pub fn draw_text(frame: &mut Img, mut x: i32, y: i32, text: &str, color: Rgb) -> i32 {
    for ch in text.chars() {
        let g = glyph(ch);
        for (dy, row) in g.iter().enumerate() {
            for (dx, c) in row.bytes().enumerate() {
                if c == b'#' {
                    frame.put(x + dx as i32, y + dy as i32, color);
                }
            }
        }
        x += glyph_width(g) + SPACING;
    }
    x - SPACING
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_rectangular() {
        for ch in ('0'..='9').chain('A'..='Z').chain([' ', '/', ':', '.', '-', '+', '?', '^', 'v']) {
            let g = glyph(ch);
            assert!(g.iter().all(|r| r.len() == g[0].len()), "{ch}");
        }
    }

    #[test]
    fn widths() {
        assert_eq!(text_width(""), 0);
        assert_eq!(text_width("12"), 7);
        assert_eq!(text_width("M"), 5);
    }

    #[test]
    fn unknown_characters_show_a_question_mark() {
        assert_eq!(glyph('~'), glyph('?'));
    }
}
