//! List Home Assistant locks, lights and gate candidates so they can be chosen for config.toml.
//! Read-only. The token is read from MATRIX_HA_TOKEN, an env file, or a hidden prompt; it is
//! never taken from the command line and never printed.

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;

use matrix_display::hadiscover::{fetch_states, load_settings, report, Options};

#[derive(Parser)]
#[command(about = "List Home Assistant locks, lights and gate candidates (read-only)")]
struct Args {
    /// Home Assistant base URL (default from MATRIX_HA_URL or http://homeassistant:8123)
    #[arg(long)]
    url: Option<String>,
    /// env file holding MATRIX_HA_TOKEN
    #[arg(long)]
    env_file: Option<String>,
    /// also list covers/binary sensors with a door-like device class
    #[arg(long)]
    all_doors: bool,
    /// also list status-LED / access-point lights normally hidden
    #[arg(long)]
    all_lights: bool,
    /// print a config.toml snippet
    #[arg(long)]
    toml: bool,
}

fn main() -> ExitCode {
    let a = Args::parse();
    let environ = std::env::vars().collect();
    let (url, token) = match load_settings(a.env_file.as_deref(), &environ, a.url.as_deref(), std::io::stdin().is_terminal()) {
        Ok(v) => v,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("Querying {url} ...");
    let states = match fetch_states(&url, &token) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::FAILURE;
        }
    };
    let opts = Options { all_doors: a.all_doors, all_lights: a.all_lights, toml: a.toml };
    match report(&mut std::io::stdout(), &states, &opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
