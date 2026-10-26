//! Fixed screen regions (DESIGN.md section 3). Widgets draw only inside their `Rect`.

pub const WIDTH: usize = 64;
pub const HEIGHT: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn x2(&self) -> i32 {
        self.x + self.w
    }

    pub fn y2(&self) -> i32 {
        self.y + self.h
    }
}

/// The date and temperature are overlaid on the scene.
pub const SCENE: Rect = Rect::new(0, 0, 64, 35);
pub const TEMPS: Rect = Rect::new(0, 35, 64, 8);
pub const NEXT_CHANGE: Rect = Rect::new(0, 43, 64, 10);
pub const HOME: Rect = Rect::new(0, 53, 64, 11);

/// Region names in drawing order.
pub const REGIONS: [(&str, Rect); 4] = [
    ("scene", SCENE),
    ("temps", TEMPS),
    ("next_change", NEXT_CHANGE),
    ("home", HOME),
];

pub fn region(name: &str) -> Option<Rect> {
    REGIONS.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_tile_the_screen_without_overlap() {
        let mut rects: Vec<Rect> = REGIONS.iter().map(|(_, r)| *r).collect();
        rects.sort_by_key(|r| r.y);
        assert_eq!(rects[0].y, 0);
        assert_eq!(rects.last().unwrap().y2(), HEIGHT as i32);
        for pair in rects.windows(2) {
            assert_eq!(pair[0].y2(), pair[1].y);
        }
        assert!(rects.iter().all(|r| r.x == 0 && r.w == WIDTH as i32));
    }
}
