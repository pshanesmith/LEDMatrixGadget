//! Look up a ZIP / postal code once and write it to [location] in config/config.toml.
//!
//!   matrix-geocode 10001                 # US by default, asks you to confirm
//!   matrix-geocode 10001 --country US --units fahrenheit
//!   matrix-geocode --lat 40.748 --lon -73.994 --label "New York, NY"   # manual override

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;

use matrix_display::data::geocode::{geocode, http_get_json, write_location, Place, Throttle, NOMINATIM_MIN_INTERVAL};

#[derive(Parser)]
#[command(about = "Look up a ZIP / postal code once and write it to [location] in config.toml")]
struct Args {
    postal_code: Option<String>,
    /// ISO country code
    #[arg(long, default_value = "US")]
    country: String,
    #[arg(long, default_value = "config/config.toml")]
    config: String,
    #[arg(long, value_parser = ["fahrenheit", "celsius"])]
    units: Option<String>,
    /// accept the first match without asking
    #[arg(long)]
    yes: bool,
    #[arg(long, allow_hyphen_values = true)]
    lat: Option<f64>,
    #[arg(long, allow_hyphen_values = true)]
    lon: Option<f64>,
    #[arg(long, default_value = "")]
    label: String,
}

fn choose(places: &[Place], assume_yes: bool) -> Option<Place> {
    for (i, p) in places.iter().enumerate() {
        let (lat, lon) = p.rounded();
        println!("  {}. {}  ({lat}, {lon})  [{}]", i + 1, p.label, p.provider);
    }
    if assume_yes {
        return places.first().cloned();
    }
    print!("Use which match? [1-{}, Enter = 1, n = cancel] ", places.len());
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).ok()?;
    let answer = answer.trim().to_lowercase();
    if matches!(answer.as_str(), "n" | "no" | "q") {
        return None;
    }
    let pick = if answer.is_empty() { Some(1) } else { answer.parse::<usize>().ok() };
    match pick.and_then(|n| n.checked_sub(1)).and_then(|i| places.get(i)) {
        Some(p) => Some(p.clone()),
        None => {
            eprintln!("not a valid choice");
            None
        }
    }
}

fn main() -> ExitCode {
    let a = Args::parse();
    let place = if a.lat.is_some() || a.lon.is_some() {
        let (Some(lat), Some(lon)) = (a.lat, a.lon) else {
            eprintln!("error: --lat and --lon go together");
            return ExitCode::from(2);
        };
        if !((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon)) {
            eprintln!("error: coordinates out of range");
            return ExitCode::from(2);
        }
        let label = if a.label.is_empty() { format!("{lat:.3}, {lon:.3}") } else { a.label.clone() };
        Place::new(&label, lat, lon, &a.country.to_uppercase(), "manual")
    } else if let Some(code) = &a.postal_code {
        let mut throttle = Throttle::new(NOMINATIM_MIN_INTERVAL);
        let places = match geocode(code, &a.country, &http_get_json, &mut throttle) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(1);
            }
        };
        match choose(&places, a.yes) {
            Some(p) => p,
            None => {
                println!("nothing written");
                return ExitCode::from(2);
            }
        }
    } else {
        eprintln!("error: give a postal code, or --lat/--lon");
        return ExitCode::from(2);
    };

    if let Err(e) = write_location(Path::new(&a.config), &place, a.postal_code.as_deref().unwrap_or(""), a.units.as_deref()) {
        eprintln!("error: {e}");
        return ExitCode::from(1);
    }
    let (lat, lon) = place.rounded();
    println!("wrote [location] to {}: {} ({lat}, {lon})", a.config, place.label);
    ExitCode::SUCCESS
}
