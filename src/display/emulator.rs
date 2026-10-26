use std::path::Path;

use anyhow::Result;
use image::RgbImage;

use super::Display;
use crate::gfx::Img;

/// Keeps frames in memory and writes previews; no hardware needed.
pub struct EmulatorDisplay {
    width: usize,
    height: usize,
    pub brightness: u8,
    pub last_frame: Option<Img>,
}

pub fn to_rgb_image(img: &Img) -> RgbImage {
    let mut out = RgbImage::new(img.w as u32, img.h as u32);
    for (i, p) in img.data.iter().enumerate() {
        out.put_pixel((i % img.w) as u32, (i / img.w) as u32, image::Rgb(*p));
    }
    out
}

impl EmulatorDisplay {
    pub fn new(width: usize, height: usize) -> Self {
        EmulatorDisplay { width, height, brightness: 100, last_frame: None }
    }

    /// The last frame enlarged. `led_look` draws round dots on black.
    pub fn render(&self, scale: usize, led_look: bool) -> Img {
        let blank = Img::black(self.width, self.height);
        let frame = self.last_frame.as_ref().unwrap_or(&blank);
        let (w, h) = (self.width * scale, self.height * scale);
        if !led_look || scale < 4 {
            let mut out = Img::black(w, h);
            for y in 0..h {
                for x in 0..w {
                    out.data[y * w + x] = frame.get(x / scale, y / scale);
                }
            }
            return out;
        }
        let mut out = Img::new(w, h, [6, 6, 6]);
        let s = scale as i32;
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                out.fill_ellipse(x * s + 1, y * s + 1, (x + 1) * s - 2, (y + 1) * s - 2,
                                 frame.get(x as usize, y as usize));
            }
        }
        out
    }

    pub fn save_png(&self, path: impl AsRef<Path>, scale: usize, led_look: bool) -> Result<()> {
        to_rgb_image(&self.render(scale, led_look)).save(path)?;
        Ok(())
    }
}

impl Display for EmulatorDisplay {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn show(&mut self, image: &Img) -> Result<()> {
        self.check(image)?;
        self.last_frame = Some(image.clone());
        Ok(())
    }

    fn set_brightness(&mut self, percent: u8) {
        self.brightness = percent.clamp(1, 100);
    }
}

/// Write frames as an endlessly looping animated GIF, `delay_ms` per frame.
pub fn save_gif(path: impl AsRef<Path>, frames: &[Img], delay_ms: u32) -> Result<()> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, RgbaImage};

    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = GifEncoder::new(file);
    enc.set_repeat(Repeat::Infinite)?;
    for f in frames {
        let mut rgba = RgbaImage::new(f.w as u32, f.h as u32);
        for (i, p) in f.data.iter().enumerate() {
            rgba.put_pixel((i % f.w) as u32, (i / f.w) as u32, image::Rgba([p[0], p[1], p[2], 255]));
        }
        enc.encode_frame(Frame::from_parts(rgba, 0, 0, Delay::from_numer_denom_ms(delay_ms, 1)))?;
    }
    Ok(())
}

/// Emulator that keeps every frame so a run can be saved as an animated GIF.
pub struct RecordingDisplay {
    pub inner: EmulatorDisplay,
    pub scale: usize,
    pub frames: Vec<Img>,
}

impl RecordingDisplay {
    pub fn new(width: usize, height: usize, scale: usize) -> Self {
        RecordingDisplay { inner: EmulatorDisplay::new(width, height), scale, frames: Vec::new() }
    }
}

impl Display for RecordingDisplay {
    fn width(&self) -> usize {
        self.inner.width()
    }

    fn height(&self) -> usize {
        self.inner.height()
    }

    fn show(&mut self, image: &Img) -> Result<()> {
        self.inner.show(image)?;
        self.frames.push(self.inner.render(self.scale, self.scale >= 4));
        Ok(())
    }

    fn set_brightness(&mut self, percent: u8) {
        self.inner.set_brightness(percent);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_wrong_size_and_scales() {
        let mut d = EmulatorDisplay::new(64, 64);
        assert!(d.show(&Img::black(32, 32)).is_err());
        d.show(&Img::new(64, 64, [1, 2, 3])).unwrap();
        let big = d.render(8, true);
        assert_eq!((big.w, big.h), (512, 512));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        d.save_png(&path, 2, false).unwrap();
        let png = image::open(&path).unwrap();
        assert_eq!((png.width(), png.height()), (128, 128));
    }

    #[test]
    fn led_look_leaves_gaps_between_dots() {
        let mut d = EmulatorDisplay::new(64, 64);
        d.show(&Img::new(64, 64, [200, 0, 0])).unwrap();
        let big = d.render(8, true);
        assert_eq!(big.get(0, 0), [6, 6, 6]); // corner of a cell is background
        assert_eq!(big.get(4, 4), [200, 0, 0]); // centre is lit
    }
}
