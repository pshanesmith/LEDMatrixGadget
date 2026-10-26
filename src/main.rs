use std::process::ExitCode;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use chrono::Local;
use clap::Parser;

use matrix_display::compositor::build_default_compositor;
use matrix_display::config::{load_config, Config};
use matrix_display::display::emulator::{save_gif, EmulatorDisplay, RecordingDisplay};
use matrix_display::display::Display;
use matrix_display::runner::{state_dir, DataSources, Runner, Sources, StaticSources};
use matrix_display::samples::{sample_names, sample_state};
use matrix_display::state::AppState;

#[derive(Parser)]
#[command(name = "matrix-display", version, about = "64x64 LED matrix status display")]
struct Args {
    /// path to config.toml
    #[arg(long)]
    config: Option<String>,
    /// use the emulator instead of the panel
    #[arg(long)]
    emulate: bool,
    /// render a single frame and exit (implies --emulate)
    #[arg(long)]
    once: bool,
    /// with --emulate: run this long and save --out
    #[arg(long)]
    seconds: Option<f64>,
    /// PNG (or .gif with --seconds) for emulator output
    #[arg(long, default_value = "frame.png")]
    out: String,
    /// PNG enlargement factor
    #[arg(long, default_value_t = 8)]
    scale: usize,
    /// use a built-in sample state
    #[arg(long, value_parser = clap::builder::PossibleValuesParser::new(sample_names()))]
    sample: Option<String>,
    /// no network: use --sample data (default clear_day)
    #[arg(long)]
    offline: bool,
    /// outline the screen regions
    #[arg(long)]
    debug_layout: bool,
    #[arg(short, long)]
    verbose: bool,
}

fn init_logging(verbose: bool) {
    use std::io::Write;
    let default = if verbose { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default))
        .format_timestamp(None)
        .format(|buf, record| writeln!(buf, "{} {}: {}", record.level(), record.target(), record.args()))
        .init();
}

fn main() -> ExitCode {
    let args = Args::parse();
    init_logging(args.verbose);
    let cfg = match load_config(args.config.as_deref()) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("config error: {e}");
            return ExitCode::from(2);
        }
    };
    match run(args, cfg) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args, cfg: Config) -> Result<()> {
    let fixed = args.sample.clone().or_else(|| args.offline.then(|| "clear_day".to_string()));
    let mut compositor = build_default_compositor(true, args.debug_layout);
    let (width, height) = (cfg.display.width(), cfg.display.height());
    let led_look = args.scale >= 4;

    if args.once {
        let state = match &fixed {
            Some(name) => sample_state(name),
            None => AppState::new(Local::now().fixed_offset()),
        };
        let mut display = EmulatorDisplay::new(width, height);
        display.show(&compositor.render(&state, 0.0))?;
        display.save_png(&args.out, args.scale, led_look)?;
        println!("wrote {}", args.out);
        return Ok(());
    }

    // Create the display first: on hardware it drops root privileges, and everything started
    // afterwards (network threads, cache writes) then runs unprivileged.
    if args.emulate {
        let fps = cfg.display.scene_fps.max(1) as f64;
        let display = RecordingDisplay::new(width, height, args.scale.min(4));
        let display = drive(&args, cfg, display, compositor, fixed)?;
        if !display.frames.is_empty() {
            if args.out.ends_with(".gif") {
                save_gif(&args.out, &display.frames, (1000.0 / fps) as u32)?;
            } else {
                display.inner.save_png(&args.out, args.scale, led_look)?;
            }
            println!("wrote {} ({} frames)", args.out, display.frames.len());
        }
        return Ok(());
    }
    hardware(&args, cfg, compositor, fixed)
}

#[cfg(feature = "hardware")]
fn hardware(args: &Args, cfg: Config, compositor: matrix_display::compositor::Compositor,
            fixed: Option<String>) -> Result<()> {
    let display = matrix_display::display::hardware::HardwareDisplay::new(&cfg.display)?;
    drive(args, cfg, display, compositor, fixed).map(|_| ())
}

#[cfg(not(feature = "hardware"))]
fn hardware(_: &Args, _: Config, _: matrix_display::compositor::Compositor, _: Option<String>) -> Result<()> {
    anyhow::bail!("this build has no panel driver; use --emulate, or build with `--features hardware` on the Pi")
}

/// Run the live loop on `display`, then close it and hand it back.
fn drive<D: Display>(args: &Args, cfg: Config, display: D, compositor: matrix_display::compositor::Compositor,
                     fixed: Option<String>) -> Result<D> {
    let sources: Box<dyn DataSources> = match &fixed {
        Some(name) => {
            let s = sample_state(name);
            Box::new(StaticSources::new(s.weather, s.home))
        }
        None => Box::new(Sources::new(&cfg, &state_dir())?),
    };
    let max_frames = args.seconds.map(|s| (s * cfg.display.scene_fps as f64) as u64);

    let config_path = args.config.clone();
    let static_sample = fixed.clone();
    let mut runner = Runner::new(cfg, display, sources, compositor).with_reload(Box::new(move || {
        let cfg = load_config(config_path.as_deref()).map_err(|e| e.to_string())?;
        let sources: Box<dyn DataSources> = match &static_sample {
            Some(name) => {
                let s = sample_state(name);
                Box::new(StaticSources::new(s.weather, s.home))
            }
            None => Box::new(Sources::new(&cfg, &state_dir()).map_err(|e| e.to_string())?),
        };
        Ok((cfg, sources))
    }));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, runner.stop.clone()).context("SIGTERM handler")?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, runner.stop.clone()).context("SIGINT handler")?;
    signal_hook::flag::register(signal_hook::consts::SIGHUP, runner.reload_requested.clone())
        .context("SIGHUP handler")?;

    runner.sources.start();
    let result = runner.run(max_frames);
    runner.sources.stop();
    runner.display.close();
    runner.stop.store(true, Ordering::Relaxed);
    result?;
    Ok(runner.display)
}
