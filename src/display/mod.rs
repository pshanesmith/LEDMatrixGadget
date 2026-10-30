//! Output backends: the real panel (feature `hardware`) and an emulator that needs no hardware.

pub mod emulator;
pub mod pattern;
#[cfg(feature = "hardware")]
pub mod hardware;

use anyhow::{bail, Result};

use crate::gfx::Img;

/// A 64x64 RGB output.
pub trait Display {
    fn width(&self) -> usize;
    fn height(&self) -> usize;

    /// Present a frame. `image` must be exactly width x height.
    fn show(&mut self, image: &Img) -> Result<()>;

    /// Set brightness 1-100 (clamped to the configured max by the caller).
    fn set_brightness(&mut self, percent: u8);

    /// Show a black frame (the sleep window). The backend keeps refreshing the panel; only the picture is dark.
    fn blank(&mut self) -> Result<()> {
        self.show(&Img::black(self.width(), self.height()))
    }

    /// Blank the display and release resources.
    fn close(&mut self) {}

    fn check(&self, image: &Img) -> Result<()> {
        if image.w != self.width() || image.h != self.height() {
            bail!("frame is {}x{}, display is {}x{}", image.w, image.h, self.width(), self.height());
        }
        Ok(())
    }
}
