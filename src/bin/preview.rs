//! Render a sample state to PNG, or an animated GIF, using the emulator.
//!
//!   matrix-preview --sample rain_then_snow --out rain.png
//!   matrix-preview --sample thunder --gif --frames 45 --fps 15 --out storm.gif
//!
//! `--all` writes every sample (and extra UI variants) to docs/previews/ui-<name>.png;
//! `--scenes` writes just the weather scene of every sample: scene-<name>.png and a 2 s scene-<name>.gif.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use matrix_display::compositor::build_default_compositor;
use matrix_display::display::emulator::{save_gif, EmulatorDisplay};
use matrix_display::display::Display;
use matrix_display::gfx::Img;
use matrix_display::samples::{sample_names, sample_state};
use matrix_display::scene::render_state;
use matrix_display::ui::variants::variants;

#[derive(Parser)]
#[command(about = "Render a sample state to PNG or GIF with the emulator")]
struct Args {
    #[arg(long, default_value = "clear_day", value_parser = clap::builder::PossibleValuesParser::new(sample_names()))]
    sample: String,
    #[arg(long, default_value = "preview.png")]
    out: String,
    #[arg(long, default_value_t = 8)]
    scale: usize,
    #[arg(long)]
    gif: bool,
    /// scene time in seconds for a still image (a GIF starts here)
    #[arg(long, default_value_t = 0.0)]
    t: f64,
    #[arg(long, default_value_t = 30)]
    frames: usize,
    #[arg(long, default_value_t = 15)]
    fps: u32,
    #[arg(long)]
    debug_layout: bool,
    /// write every sample and UI variant to <out-dir>/ui-<name>.png
    #[arg(long)]
    all: bool,
    /// write the weather scene of every sample (PNG and a 2 s GIF) to <out-dir>/scene-<name>.png|gif
    #[arg(long)]
    scenes: bool,
    #[arg(long, default_value = "docs/previews")]
    out_dir: PathBuf,
}

fn main() -> Result<()> {
    let a = Args::parse();
    let led_look = a.scale >= 4;
    let mut comp = build_default_compositor(true, a.debug_layout);
    let mut disp = EmulatorDisplay::new(64, 64);

    if a.scenes {
        std::fs::create_dir_all(&a.out_dir)?;
        // The scene alone (64x35), every pixel readable: no LED look. Animation starts at t = 3 s.
        let big = |im: &Img| {
            let mut out = Img::black(im.w * a.scale, im.h * a.scale);
            for y in 0..out.h {
                for x in 0..out.w {
                    out.data[y * out.w + x] = im.get(x / a.scale, y / a.scale);
                }
            }
            out
        };
        for name in sample_names() {
            let state = sample_state(name);
            let png = a.out_dir.join(format!("scene-{name}.png"));
            matrix_display::display::emulator::to_rgb_image(&big(&render_state(&state, 3.0))).save(&png)?;
            let frames: Vec<Img> = (0..30).map(|i| big(&render_state(&state, 3.0 + i as f64 / 15.0))).collect();
            save_gif(a.out_dir.join(format!("scene-{name}.gif")), &frames, 1000 / 15)?;
            println!("wrote {}", png.display());
        }
        return Ok(());
    }

    if a.all {
        std::fs::create_dir_all(&a.out_dir)?;
        let mut states: Vec<(String, _)> = sample_names().into_iter().map(|n| (n.to_string(), sample_state(n))).collect();
        states.extend(variants().into_iter().map(|(n, s)| (format!("var-{n}"), s)));
        for (name, st) in states {
            disp.show(&comp.render(&st, 0.0))?;
            let path = a.out_dir.join(format!("ui-{name}.png"));
            disp.save_png(&path, a.scale, led_look)?;
            println!("wrote {}", path.display());
        }
        return Ok(());
    }

    let state = sample_state(&a.sample);
    if !a.gif {
        disp.show(&comp.render(&state, a.t))?;
        disp.save_png(&a.out, a.scale, led_look)?;
    } else {
        let mut frames = Vec::new();
        for i in 0..a.frames {
            disp.show(&comp.render(&state, a.t + i as f64 / a.fps as f64))?;
            frames.push(disp.render(a.scale, led_look));
        }
        save_gif(&a.out, &frames, 1000 / a.fps)?;
    }
    println!("wrote {}", a.out);
    Ok(())
}
