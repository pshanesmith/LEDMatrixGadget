//! The real panel, through the `rpi-led-panel` crate (a pure Rust port of hzeller's panel driver).

use anyhow::{anyhow, bail, Result};
use rpi_led_panel::{
    Canvas, HardwareMapping, LedSequence, MultiplexMapperType, RGBMatrix, RGBMatrixConfig,
    RowAddressSetterType,
};

use super::Display;
use crate::config::DisplayConfig;
use crate::gfx::Rgb;
use crate::gfx::Img;

pub struct HardwareDisplay {
    matrix: RGBMatrix,
    canvas: Option<Box<Canvas>>,
    width: usize,
    height: usize,
    brightness: u8,
    /// Pixels of the frame on the panel, when `shown_valid` (the buffer is kept to be reused).
    shown: Vec<Rgb>,
    shown_valid: bool,
}

/// `adafruit-hat-pwm` -> `AdafruitHatPwm`: the crate parses CamelCase names.
fn camel_case(name: &str) -> String {
    name.split(['-', '_', ' '])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            c.next().map(|f| f.to_uppercase().chain(c.flat_map(char::to_lowercase)).collect::<String>()).unwrap_or_default()
        })
        .collect()
}

fn row_setter(code: u32) -> Result<RowAddressSetterType> {
    Ok(match code {
        0 => RowAddressSetterType::Direct,
        1 => RowAddressSetterType::ShiftRegister,
        2 => RowAddressSetterType::DirectABCDLine,
        3 => RowAddressSetterType::ABCShiftRegister,
        4 => RowAddressSetterType::SM5266,
        other => bail!("row_address_type {other} is not supported (0-4)"),
    })
}

fn multiplexing(code: u32) -> Result<Option<MultiplexMapperType>> {
    Ok(match code {
        0 => None,
        1 => Some(MultiplexMapperType::Stripe),
        2 => Some(MultiplexMapperType::Checkered),
        3 => Some(MultiplexMapperType::Spiral),
        4 => Some(MultiplexMapperType::ZStripe08),
        other => bail!("multiplexing {other} is not supported (0-4)"),
    })
}

/// The driver's settings for a `[display]` section.
pub fn matrix_config(cfg: &DisplayConfig) -> Result<RGBMatrixConfig> {
    let mapping: HardwareMapping = camel_case(&cfg.hardware_mapping)
        .parse()
        .map_err(|e| anyhow!("hardware_mapping {:?}: {e}", cfg.hardware_mapping))?;
    let led_sequence: LedSequence = cfg
        .rgb_sequence
        .to_uppercase()
        .parse()
        .map_err(|e| anyhow!("rgb_sequence {:?}: {e}", cfg.rgb_sequence))?;
    Ok(RGBMatrixConfig {
        hardware_mapping: mapping,
        rows: cfg.rows as usize,
        cols: cfg.cols as usize,
        chain_length: cfg.chain_length as usize,
        parallel: cfg.parallel as usize,
        pwm_bits: cfg.pwm_bits as usize,
        pwm_lsb_nanoseconds: cfg.pwm_lsb_nanoseconds,
        slowdown: Some(i32::try_from(cfg.gpio_slowdown).unwrap_or(i32::MAX)),
        // 0 means "no cap" in the config; the driver always wants a target, so use its default (120 Hz).
        refresh_rate: if cfg.limit_refresh_hz == 0 { 120 } else { cfg.limit_refresh_hz as usize },
        row_setter: row_setter(cfg.row_address_type)?,
        multiplexing: multiplexing(cfg.multiplexing)?,
        led_sequence,
        led_brightness: cfg.brightness.min(cfg.max_brightness),
        drop_privs: cfg.drop_privileges,
        realtime_priority: (cfg.realtime_priority > 0).then_some(cfg.realtime_priority),
        ..RGBMatrixConfig::default()
    })
}

impl HardwareDisplay {
    /// Starts the panel driver. This is where GPIO is set up and, when configured, root privileges
    /// are dropped, so create it before starting anything that should run unprivileged.
    pub fn new(cfg: &DisplayConfig) -> Result<Self> {
        let (matrix, canvas) = RGBMatrix::new(matrix_config(cfg)?, 0).map_err(|e| anyhow!("{e}"))?;
        Ok(HardwareDisplay {
            matrix,
            width: canvas.width(),
            height: canvas.height(),
            canvas: Some(canvas),
            brightness: cfg.brightness.min(cfg.max_brightness),
            shown: Vec::new(),
            shown_valid: false,
        })
    }

    /// The driver's measured refresh rate (average over the last 60 frames), in Hz.
    pub fn refresh_rate(&self) -> usize {
        self.matrix.get_framerate()
    }
}

impl Display for HardwareDisplay {
    fn width(&self) -> usize {
        self.width
    }

    fn height(&self) -> usize {
        self.height
    }

    fn show(&mut self, image: &Img) -> Result<()> {
        self.check(image)?;
        // Most frames of a calm scene are identical to the last one; skipping them saves the copy,
        // and the panel keeps showing the previous frame.
        if self.shown_valid && self.shown == image.data {
            return Ok(());
        }
        let mut canvas = self.canvas.take().expect("canvas is always put back");
        canvas.set_brightness(self.brightness);
        for y in 0..self.height {
            for x in 0..self.width {
                let [r, g, b] = image.get(x, y);
                canvas.set_pixel(x, y, r, g, b);
            }
        }
        self.canvas = Some(self.matrix.update_on_vsync(canvas));
        self.shown.clone_from(&image.data);
        self.shown_valid = true;
        Ok(())
    }

    fn set_brightness(&mut self, percent: u8) {
        let percent = percent.clamp(1, 100);
        if percent != self.brightness {
            self.brightness = percent;
            self.shown_valid = false; // brightness is applied as pixels are set: redraw
        }
    }

    fn close(&mut self) {
        if let Some(mut canvas) = self.canvas.take() {
            canvas.fill(0, 0, 0);
            self.canvas = Some(self.matrix.update_on_vsync(canvas));
        }
        self.shown_valid = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_the_crates_camel_case() {
        assert_eq!(camel_case("adafruit-hat-pwm"), "AdafruitHatPwm");
        assert_eq!(camel_case("regular_pi1"), "RegularPi1");
        assert_eq!(camel_case("Classic"), "Classic");
    }

    #[test]
    fn display_settings_become_driver_settings() {
        let mut cfg = DisplayConfig { brightness: 90, ..DisplayConfig::default() }; // above max_brightness (70)
        let m = matrix_config(&cfg).unwrap();
        assert_eq!((m.rows, m.cols, m.pwm_bits, m.pwm_lsb_nanoseconds), (64, 64, 11, 130));
        assert_eq!((m.slowdown, m.led_brightness, m.refresh_rate, m.drop_privs), (Some(2), 70, 120, true));
        assert_eq!(m.led_sequence, LedSequence::Rgb);
        assert_eq!(m.hardware_mapping, HardwareMapping::adafruit_hat_pwm());
        cfg.limit_refresh_hz = 150;
        cfg.rgb_sequence = "rbg".into();
        let m = matrix_config(&cfg).unwrap();
        assert_eq!((m.refresh_rate, m.led_sequence), (150, LedSequence::Rbg));
        assert_eq!(m.realtime_priority, None);
        cfg.realtime_priority = 50;
        assert_eq!(matrix_config(&cfg).unwrap().realtime_priority, Some(50));
    }

    #[test]
    fn bad_settings_are_reported() {
        let mut cfg = DisplayConfig { hardware_mapping: "nonsense".into(), ..DisplayConfig::default() };
        assert!(matrix_config(&cfg).is_err());
        cfg = DisplayConfig::default();
        cfg.row_address_type = 9;
        assert!(matrix_config(&cfg).is_err());
        cfg = DisplayConfig::default();
        cfg.multiplexing = 3;
        assert!(matrix_config(&cfg).unwrap().multiplexing.is_some());
    }
}
