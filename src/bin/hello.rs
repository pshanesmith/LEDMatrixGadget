//! Hardware smoke test through the project's own `HardwareDisplay` backend.
//!
//! Run with root (GPIO access):  sudo matrix-hello [options]
//!
//! Draws a picture that makes the usual wiring problems obvious (see display/pattern.rs):
//! `--pattern frame` (default) for the general check, `square` for missing or repeated rows,
//! `pulse` for colour order, `gray` for banding and flicker.

use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;

use matrix_display::config::load_config;
use matrix_display::display::hardware::HardwareDisplay;
use matrix_display::display::pattern::by_name;
use matrix_display::display::Display;

#[derive(Parser)]
#[command(about = "Hardware smoke test: red/blue halves, RGB bars, border and a moving square")]
struct Args {
    /// config.toml (default: search)
    #[arg(long)]
    config: Option<String>,
    #[arg(long, default_value_t = 20.0)]
    seconds: f64,
    /// print the panel's refresh rate on stderr
    #[arg(long)]
    show_refresh: bool,
    /// what to draw: frame (default), square, pulse or gray
    #[arg(long, default_value = "frame", value_parser = ["frame", "square", "pulse", "gray"])]
    pattern: String,
    /// override [display] settings
    #[arg(long)]
    rows: Option<u32>,
    #[arg(long)]
    cols: Option<u32>,
    #[arg(long)]
    gpio_slowdown: Option<u32>,
    #[arg(long)]
    pwm_bits: Option<u8>,
    #[arg(long)]
    pwm_lsb_nanoseconds: Option<u32>,
    #[arg(long)]
    row_address_type: Option<u32>,
    #[arg(long)]
    multiplexing: Option<u32>,
    #[arg(long)]
    brightness: Option<u8>,
    #[arg(long)]
    hardware_mapping: Option<String>,
    #[arg(long)]
    rgb_sequence: Option<String>,
}

fn main() -> Result<()> {
    let a = Args::parse();
    let mut cfg = match load_config(a.config.as_deref()) {
        Ok(c) => c.display,
        Err(e) => {
            eprintln!("config error: {e}");
            std::process::exit(2);
        }
    };
    macro_rules! over {
        ($($f:ident),*) => { $(if let Some(v) = a.$f.clone() { cfg.$f = v; })* };
    }
    over!(rows, cols, gpio_slowdown, pwm_bits, pwm_lsb_nanoseconds, row_address_type, multiplexing,
          brightness, hardware_mapping, rgb_sequence);
    println!("settings: {cfg:?}");

    let mut display = HardwareDisplay::new(&cfg)?; // drops root privileges after setup
    let start = Instant::now();
    let mut last_report = Instant::now();
    while start.elapsed().as_secs_f64() < a.seconds {
        if a.show_refresh && last_report.elapsed() >= Duration::from_secs(1) {
            eprintln!("refresh: {} Hz", display.refresh_rate());
            last_report = Instant::now();
        }
        let t = start.elapsed().as_secs_f64();
        display.show(&by_name(&a.pattern, display.width(), t).expect("validated by clap"))?;
        std::thread::sleep(Duration::from_millis(33));
    }
    display.close();
    Ok(())
}
