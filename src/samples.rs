//! Named sample `AppState`s for previews, golden images and tests (no network needed).

use std::sync::LazyLock;

use chrono::{DateTime, Duration, FixedOffset, TimeZone, Timelike};

use crate::state::*;

pub static TZ: LazyLock<FixedOffset> = LazyLock::new(|| FixedOffset::west_opt(4 * 3600).unwrap());
pub static NOON: LazyLock<Dt> =
    LazyLock::new(|| TZ.with_ymd_and_hms(2026, 10, 23, 12, 0, 0).unwrap());

struct Scenario {
    name: &'static str,
    code: i64,
    is_day: bool,
    cloud: i64,
    wind: f64,
    change_hours: Option<usize>,
    change_code: i64,
}

const fn sc(name: &'static str, code: i64, is_day: bool, cloud: i64, wind: f64,
            change_hours: Option<usize>, change_code: i64) -> Scenario {
    Scenario { name, code, is_day, cloud, wind, change_hours, change_code }
}

// name, current code, is_day, cloud %, wind, hours-from-now, code after the change
const SCENARIOS: [Scenario; 8] = [
    sc("clear_day", 0, true, 5, 4.0, None, 0),
    sc("clear_night", 0, false, 0, 3.0, None, 0),
    sc("partly_cloudy", 2, true, 40, 8.0, Some(5), 61),
    sc("overcast", 3, true, 100, 6.0, Some(7), 3),
    sc("fog", 45, true, 90, 1.0, Some(4), 0),
    sc("rain_then_snow", 63, true, 100, 12.0, Some(6), 73),
    sc("snow", 73, true, 100, 10.0, Some(9), 3),
    sc("thunder", 95, false, 100, 20.0, Some(3), 61),
];

pub fn sample_names() -> Vec<&'static str> {
    SCENARIOS.iter().map(|s| s.name).collect()
}

pub fn sample_state(name: &str) -> AppState {
    sample_state_at(name, *NOON)
}

pub fn sample_state_at(name: &str, now: Dt) -> AppState {
    let s = SCENARIOS
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("unknown sample {name:?}"));
    let hourly = (0..48)
        .map(|h| {
            let code = match s.change_hours {
                Some(c) if h >= c => s.change_code,
                _ => s.code,
            };
            HourlyPoint {
                time: now + Duration::hours(h as i64),
                code,
                temperature: 60.0 - h as f64 * 0.2,
                precipitation_probability: 20,
            }
        })
        .collect();
    let at = |hour, minute| now.with_hour(hour).unwrap().with_minute(minute).unwrap();
    let weather = WeatherSnapshot {
        fetched_at: now,
        current: CurrentWeather {
            temperature: 61.0,
            code: s.code,
            cloud_cover: s.cloud,
            wind_speed: s.wind,
            is_day: s.is_day,
        },
        hourly,
        daily: DailyWeather { low: 48.0, high: 71.0, sunrise: at(7, 40), sunset: at(18, 20) },
    };
    let mut home = HomeSnapshot::new(now);
    home.locks_unlocked = 1;
    home.lights_on = 8;
    AppState { now, weather: Some(weather), home: Some(home) }
}

/// Convenience for tests: a datetime in the sample time zone.
pub fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<FixedOffset> {
    TZ.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
}
