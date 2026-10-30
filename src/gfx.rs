//! A small RGB framebuffer with the handful of drawing operations the display needs.
//!
//! The operations follow what Pillow did for the original Python version (inclusive
//! rectangles, integer-truncated coordinates, 8-bit mask compositing), so a frame looks the same.

pub type Rgb = [u8; 3];

/// Python's `round()`: halves go to the even neighbour.
pub fn pyround(x: f64) -> i64 {
    x.round_ties_even() as i64
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub data: Vec<Rgb>,
}

impl Img {
    pub fn new(w: usize, h: usize, color: Rgb) -> Img {
        Img { w, h, data: vec![color; w * h] }
    }

    pub fn black(w: usize, h: usize) -> Img {
        Img::new(w, h, [0, 0, 0])
    }

    /// Become a copy of `src`, reusing this image's buffer.
    pub fn copy_from(&mut self, src: &Img) {
        self.w = src.w;
        self.h = src.h;
        self.data.clone_from(&src.data);
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h
    }

    pub fn get(&self, x: usize, y: usize) -> Rgb {
        self.data[y * self.w + x]
    }

    /// Set a pixel; positions outside the image are ignored.
    pub fn put(&mut self, x: i32, y: i32, c: Rgb) {
        if self.inside(x, y) {
            self.data[y as usize * self.w + x as usize] = c;
        }
    }

    pub fn put_checked(&self, x: i32, y: i32) -> Option<Rgb> {
        self.inside(x, y).then(|| self.data[y as usize * self.w + x as usize])
    }

    pub fn is_blank(&self) -> bool {
        self.data.iter().all(|p| *p == [0, 0, 0])
    }

    /// Rows `y0..y1` as a new image (full width).
    pub fn crop_rows(&self, y0: usize, y1: usize) -> Img {
        Img { w: self.w, h: y1 - y0, data: self.data[y0 * self.w..y1 * self.w].to_vec() }
    }

    /// Pillow's `draw.rectangle([x0, y0, x1, y1], fill=...)`: both corners included, clipped.
    pub fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        for y in y0.max(0)..=y1.min(self.h as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.w as i32 - 1) {
                self.data[y as usize * self.w + x as usize] = c;
            }
        }
    }

    /// Pillow's `draw.rectangle(..., outline=...)`.
    pub fn outline_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        self.line(x0, y0, x1, y0, c);
        self.line(x0, y1, x1, y1, c);
        self.line(x0, y0, x0, y1, c);
        self.line(x1, y0, x1, y1, c);
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        for_line_points(x0, y0, x1, y1, |x, y| self.put(x, y, c));
    }

    /// Connected segments through `pts`.
    pub fn polyline(&mut self, pts: &[(i32, i32)], c: Rgb) {
        for w in pts.windows(2) {
            self.line(w[0].0, w[0].1, w[1].0, w[1].1, c);
        }
    }

    pub fn fill_ellipse(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgb) {
        for_ellipse_points(x0, y0, x1, y1, |x, y| self.put(x, y, c));
    }

    /// Copy the top-left `w` x `h` pixels of `src` (clipped to it) onto this image at (x, y): `paste`
    /// of a crop, without making the crop.
    pub fn paste_part(&mut self, src: &Img, w: usize, h: usize, x: i32, y: i32) {
        for sy in 0..h.min(src.h) {
            for sx in 0..w.min(src.w) {
                let (dx, dy) = (x + sx as i32, y + sy as i32);
                if self.inside(dx, dy) {
                    self.data[dy as usize * self.w + dx as usize] = src.data[sy * src.w + sx];
                }
            }
        }
    }

    /// Composite `src` onto this image at (x, y), through `mask` when given.
    pub fn paste(&mut self, src: &Img, x: i32, y: i32, mask: Option<&Mask>) {
        for sy in 0..src.h {
            for sx in 0..src.w {
                let (dx, dy) = (x + sx as i32, y + sy as i32);
                if !self.inside(dx, dy) {
                    continue;
                }
                let s = src.data[sy * src.w + sx];
                let di = dy as usize * self.w + dx as usize;
                self.data[di] = match mask {
                    None => s,
                    Some(m) => mix(self.data[di], s, m.data[sy * m.w + sx]),
                };
            }
        }
    }

    /// Composite a solid colour at (x, y) through `mask` (the mask gives the size).
    pub fn paste_color(&mut self, c: Rgb, x: i32, y: i32, mask: &Mask) {
        for sy in 0..mask.h {
            for sx in 0..mask.w {
                let (dx, dy) = (x + sx as i32, y + sy as i32);
                if !self.inside(dx, dy) {
                    continue;
                }
                let di = dy as usize * self.w + dx as usize;
                self.data[di] = mix(self.data[di], c, mask.data[sy * mask.w + sx]);
            }
        }
    }

    /// Pillow's `Image.blend(a, b, alpha)`.
    pub fn blend(a: &Img, b: &Img, alpha: f64) -> Img {
        let mut out = b.clone();
        out.blend_from(a, alpha);
        out
    }

    /// In place, this image becomes `Img::blend(a, self, alpha)`.
    pub fn blend_from(&mut self, a: &Img, alpha: f64) {
        let alpha = alpha as f32;
        for (q, p) in self.data.iter_mut().zip(&a.data) {
            for k in 0..3 {
                q[k] = (p[k] as i32 as f32 + alpha * (q[k] as i32 - p[k] as i32) as f32) as u8;
            }
        }
    }
}

/// `dst` under `src` with 8-bit coverage `a` (Pillow's BLEND: one rounded division by 255).
fn mix(dst: Rgb, src: Rgb, a: u8) -> Rgb {
    match a {
        0 => dst,
        255 => src,
        _ => {
            let a = a as u32;
            let mut out = [0u8; 3];
            for k in 0..3 {
                let tmp = dst[k] as u32 * (255 - a) + src[k] as u32 * a + 128;
                out[k] = (((tmp >> 8) + tmp) >> 8) as u8;
            }
            out
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Mask {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Mask {
    pub fn new(w: usize, h: usize, v: u8) -> Mask {
        Mask { w, h, data: vec![v; w * h] }
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.w + x]
    }

    pub fn set(&mut self, x: i32, y: i32, v: u8) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.data[y as usize * self.w + x as usize] = v;
        }
    }

    pub fn fill_rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: u8) {
        for y in y0.max(0)..=y1.min(self.h as i32 - 1) {
            for x in x0.max(0)..=x1.min(self.w as i32 - 1) {
                self.data[y as usize * self.w + x as usize] = v;
            }
        }
    }

    pub fn fill_ellipse(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, v: u8) {
        for_ellipse_points(x0, y0, x1, y1, |x, y| self.set(x, y, v));
    }

    /// Pillow's `Image.point(f)` for an 8-bit mask.
    pub fn map(&self, f: impl Fn(u8) -> u8) -> Mask {
        Mask { w: self.w, h: self.h, data: self.data.iter().map(|v| f(*v)).collect() }
    }

    /// Pillow's `ImageFilter.MaxFilter(3)`: each pixel becomes the largest in its 3x3 neighbourhood.
    pub fn max_filter3(&self) -> Mask {
        let mut out = self.clone();
        for y in 0..self.h as i32 {
            for x in 0..self.w as i32 {
                let mut m = 0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx >= 0 && ny >= 0 && (nx as usize) < self.w && (ny as usize) < self.h {
                            m = m.max(self.data[ny as usize * self.w + nx as usize]);
                        }
                    }
                }
                out.data[y as usize * self.w + x as usize] = m;
            }
        }
        out
    }

    /// Shift the picture right by `dx` (wrapping), like `ImageChops.offset(mask, dx, 0)`.
    pub fn offset_x(&self, dx: i32) -> Mask {
        let mut out = self.clone();
        let w = self.w as i32;
        for y in 0..self.h {
            for x in 0..self.w {
                let nx = (x as i32 + dx).rem_euclid(w) as usize;
                out.data[y * self.w + nx] = self.data[y * self.w + x];
            }
        }
        out
    }
}

/// Pixels on the line from (x0, y0) to (x1, y1), both ends included (Bresenham).
pub fn line_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut pts = Vec::new();
    for_line_points(x0, y0, x1, y1, |x, y| pts.push((x, y)));
    pts
}

/// Call `f` for each pixel of `line_points`, in the same order, without collecting them.
pub fn for_line_points(x0: i32, y0: i32, x1: i32, y1: i32, mut f: impl FnMut(i32, i32)) {
    let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
    let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
    let (mut x, mut y, mut err) = (x0, y0, dx + dy);
    loop {
        f(x, y);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Pixels of the filled ellipse inscribed in the box with inclusive corners.
///
/// This traces the outline the way Pillow does (so shapes match the original display pixel for
/// pixel): in doubled coordinates, walk one quarter of the curve from its rightmost point, at each
/// step taking the neighbour (up, diagonal, left; the first wins a tie) closest to the ellipse,
/// and fill every row out to the furthest point reached on it.
pub fn ellipse_points(x0: i32, y0: i32, x1: i32, y1: i32) -> Vec<(i32, i32)> {
    let mut pts = Vec::new();
    for_ellipse_points(x0, y0, x1, y1, |x, y| pts.push((x, y)));
    pts
}

/// Call `f` for each pixel of `ellipse_points`, in the same order, without collecting them.
pub fn for_ellipse_points(x0: i32, y0: i32, x1: i32, y1: i32, mut f: impl FnMut(i32, i32)) {
    let (a, b) = ((x1 - x0) as i64, (y1 - y0) as i64);
    if a < 0 || b < 0 || (a == 0 && b == 0) {
        return;
    }
    let (a2, b2) = (a * a, b * b);
    let a2b2 = a2 * b2;
    let err = |x: i64, y: i64| (b2 * x * x + a2 * y * y - a2b2).abs();
    let (ex, ey) = (a % 2, b);
    let (mut x, mut y) = (a, b % 2);
    // (y, widest x) for each row, from the middle out. A row is complete once the walk moves on to
    // the next one, so each is filled then (in the same order as collecting them first would).
    let fill_row = |f: &mut dyn FnMut(i32, i32), y: i64, xm: i64| {
        let signs: &[i64] = if y == 0 { &[1] } else { &[1, -1] };
        for sgn in signs {
            let py = y0 as i64 + (sgn * y + b) / 2;
            let mut u = -xm;
            while u <= xm {
                f(x0 + ((u + a) / 2) as i32, py as i32);
                u += 2;
            }
        }
    };
    let mut row: Option<(i64, i64)> = None;
    loop {
        match row {
            Some(r) if r.0 == y => {}
            _ => {
                if let Some((ry, rx)) = row {
                    fill_row(&mut f, ry, rx);
                }
                row = Some((y, x));
            }
        }
        if x == ex && y == ey {
            break;
        }
        let mut best: Option<(i64, (i64, i64))> = None;
        for c in [(x, y + 2), (x - 2, y + 2), (x - 2, y)] {
            if c.0 < ex || c.1 > ey {
                continue;
            }
            let e = err(c.0, c.1);
            if best.is_none_or(|(be, _)| e < be) {
                best = Some((e, c));
            }
        }
        match best {
            Some((_, c)) => (x, y) = c,
            None => break,
        }
    }
    if let Some((ry, rx)) = row {
        fill_row(&mut f, ry, rx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_rounding_goes_to_even() {
        assert_eq!(pyround(0.5), 0);
        assert_eq!(pyround(1.5), 2);
        assert_eq!(pyround(2.5), 2);
        assert_eq!(pyround(-0.5), 0);
    }

    #[test]
    fn paste_with_mask_blends_and_clips() {
        let mut img = Img::new(4, 4, [0, 0, 0]);
        let src = Img::new(2, 2, [200, 100, 50]);
        let mask = Mask { w: 2, h: 2, data: vec![255, 0, 128, 255] };
        img.paste(&src, 3, 3, Some(&mask)); // only the top-left source pixel lands on the image
        assert_eq!(img.get(3, 3), [200, 100, 50]);
        img.paste(&src, 0, 0, Some(&mask));
        assert_eq!(img.get(0, 0), [200, 100, 50]);
        assert_eq!(img.get(1, 0), [0, 0, 0]);
        assert_eq!(img.get(0, 1), [100, 50, 25]);
    }

    #[test]
    fn rectangle_is_inclusive_and_clipped() {
        let mut img = Img::black(5, 5);
        img.fill_rect(-2, 3, 1, 9, [1, 1, 1]);
        assert_eq!(img.data.iter().filter(|p| **p == [1, 1, 1]).count(), 2 * 2);
    }

    #[test]
    fn ellipses_match_pillow() {
        // Rows of a few boxes as drawn by Pillow's ImageDraw.ellipse(..., fill=...)
        let width_of_rows = |a: i32, b: i32| -> Vec<usize> {
            let pts = ellipse_points(0, 0, a, b);
            (0..=b).map(|y| pts.iter().filter(|p| p.1 == y).count()).collect()
        };
        assert_eq!(width_of_rows(10, 8), [5, 7, 9, 11, 11, 11, 9, 7, 5]);
        assert_eq!(width_of_rows(20, 6), [11, 17, 19, 21, 19, 17, 11]);
        assert_eq!(width_of_rows(6, 6), [3, 5, 7, 7, 7, 5, 3]);
        assert_eq!(width_of_rows(2, 2), [1, 3, 1]);
        assert!(ellipse_points(0, 0, 0, 0).is_empty());
        assert_eq!(ellipse_points(5, 5, 5, 7).len(), 3);
    }

    #[test]
    fn line_includes_both_ends() {
        let pts = line_points(0, 0, 4, 2);
        assert_eq!(pts.first(), Some(&(0, 0)));
        assert_eq!(pts.last(), Some(&(4, 2)));
        assert_eq!(pts.len(), 5);
    }

    #[test]
    fn max_filter_grows_a_dot() {
        let mut m = Mask::new(5, 5, 0);
        m.set(2, 2, 255);
        let grown = m.max_filter3();
        assert_eq!(grown.data.iter().filter(|v| **v == 255).count(), 9);
    }

    #[test]
    fn offset_wraps() {
        let mut m = Mask::new(4, 1, 0);
        m.set(3, 0, 9);
        assert_eq!(m.offset_x(1).get(0, 0), 9);
    }
}
