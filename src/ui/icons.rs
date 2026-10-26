//! Pixel-art icons. Each is a slice of rows; characters index a palette, '.' is clear.
//!
//! Weather icons are 9x9, lock, bulb and bolt 7x9, gate 9x9.

use crate::gfx::{Img, Rgb};
use crate::state::Condition;

pub type Icon = &'static [&'static str];

fn palette(ch: u8) -> Rgb {
    match ch {
        b'Y' => [255, 200, 20],  // sun
        b'O' => [255, 140, 20],
        b'W' => [235, 235, 245], // light cloud / snow
        b'G' => [140, 148, 165], // grey cloud
        b'D' => [90, 96, 115],   // dark cloud
        b'B' => [70, 140, 255],  // rain
        b'S' => [200, 230, 255], // snow flake
        b'F' => [120, 130, 140], // fog
        b'M' => [235, 230, 190], // moon
        _ => [255, 255, 255],
    }
}

const CLEAR: Icon = &[
    "....Y....", ".Y.....Y.", "..YYYYY..", "..YYYYY..", "Y.YYYYY.Y",
    "..YYYYY..", "..YYYYY..", ".Y.....Y.", "....Y....",
];
const PARTLY_CLOUDY: Icon = &[
    "..Y......", ".YYY..WW.", "YYYYYWWWW", ".YYWWWWWW", "..WWWWWWW",
    ".WWWWWWWW", ".WWWWWWW.", ".........", ".........",
];
const OVERCAST: Icon = &[
    ".........", "...GGG...", "..GGGGG.G", ".GGGGGGGG", "GGGGGGGGG",
    "GGGGGGGGG", ".GGGGGGG.", ".........", ".........",
];
const FOG: Icon = &[
    "..GGGG...", ".GGGGGG..", "GGGGGGGGG", ".........", "FFFFFFFF.",
    ".........", ".FFFFFFFF", ".........", "FFFFFF...",
];
const RAIN: Icon = &[
    "...GGG...", "..GGGGG..", ".GGGGGGGG", "GGGGGGGGG", ".GGGGGGG.",
    ".........", ".B..B..B.", "B..B..B..", ".B..B..B.",
];
const SNOW: Icon = &[
    "...GGG...", "..GGGGG..", ".GGGGGGGG", "GGGGGGGGG", ".GGGGGGG.",
    ".........", ".S..S..S.", "..S..S...", ".S..S..S.",
];
const THUNDER: Icon = &[
    "...DDD...", "..DDDDD..", ".DDDDDDDD", "DDDDDDDDD", ".DDDYYDD.",
    "....YY...", "...YY....", "..YYYY...", "....Y....",
];

pub fn weather(cond: Condition) -> Icon {
    match cond {
        Condition::Clear => CLEAR,
        Condition::PartlyCloudy => PARTLY_CLOUDY,
        Condition::Overcast => OVERCAST,
        Condition::Fog => FOG,
        Condition::Rain => RAIN,
        Condition::Snow => SNOW,
        Condition::Thunder => THUNDER,
    }
}

const LOCK_CLOSED: Icon = &[
    "..###..", ".#...#.", ".#...#.", "#######", "#######", "###.###", "###.###", "#######", "#######",
];
const LOCK_OPEN: Icon = &[
    "..###..", ".#...#.", ".#.....", "#######", "#######", "###.###", "###.###", "#######", "#######",
];
pub const BULB: Icon = &[
    "..###..", ".#####.", "#######", "#######", "#######", ".#####.", "..###..", "..###..", "...#...",
];
pub const BOLT: Icon = &[
    "....###", "...###.", "..###..", ".######", "....##.", "...##..", "..##...", ".##....", ".#.....",
];
const GATE_CLOSED: Icon = &[
    "#.......#", "#########", "#.#.#.#.#", "#.#.#.#.#", "#########",
    "#.#.#.#.#", "#.#.#.#.#", "#.......#", "#.......#",
];
const GATE_OPEN: Icon = &[
    "#.......#", "#.......#", "#.......#", "###...###", "#.#...#.#",
    "###...###", "#.......#", "#.......#", "#.......#",
];
pub const MOON: Icon = &[
    "...MMM...", "..MMM....", ".MMM.....", ".MMM.....", ".MMM.....",
    ".MMM.....", "..MMM...M", "...MMMMM.", ".....MM..",
];

/// The lock icon; `open` for the unlocked one.
pub fn lock(open: bool) -> Icon {
    if open {
        LOCK_OPEN
    } else {
        LOCK_CLOSED
    }
}

pub fn gate(open: bool) -> Icon {
    if open {
        GATE_OPEN
    } else {
        GATE_CLOSED
    }
}

/// Draw an icon at (x, y). `color` overrides the palette (single-colour icons).
pub fn blit(frame: &mut Img, x: i32, y: i32, rows: Icon, color: Option<Rgb>) {
    for (dy, row) in rows.iter().enumerate() {
        for (dx, ch) in row.bytes().enumerate() {
            if ch != b'.' {
                frame.put(x + dx as i32, y + dy as i32, color.unwrap_or_else(|| palette(ch)));
            }
        }
    }
}

/// A clear sky is drawn as a moon at night; other conditions look the same day and night.
pub fn draw_weather_icon(frame: &mut Img, x: i32, y: i32, cond: Condition, night: bool) {
    let icon = if night && cond == Condition::Clear { MOON } else { weather(cond) };
    blit(frame, x, y, icon, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_sizes() {
        for cond in Condition::ALL {
            let rows = weather(cond);
            assert_eq!(rows.len(), 9);
            assert!(rows.iter().all(|r| r.len() == 9), "{cond:?}");
        }
        for (icon, w) in [(lock(false), 7), (lock(true), 7), (BULB, 7), (BOLT, 7),
                          (gate(false), 9), (gate(true), 9), (MOON, 9)] {
            assert_eq!(icon.len(), 9);
            assert!(icon.iter().all(|r| r.len() == w));
        }
    }
}
