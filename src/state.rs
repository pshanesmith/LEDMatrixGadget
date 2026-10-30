//! Shared, immutable data contracts between data fetchers, scene and widgets.
//!
//! All datetimes carry a fixed UTC offset, the forecast location's.

use std::sync::Arc;

use chrono::{DateTime, FixedOffset};

pub type Dt = DateTime<FixedOffset>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Condition {
    Clear,
    PartlyCloudy,
    Overcast,
    Fog,
    Rain,
    Snow,
    Thunder,
}

impl Condition {
    pub const ALL: [Condition; 7] = [
        Condition::Clear,
        Condition::PartlyCloudy,
        Condition::Overcast,
        Condition::Fog,
        Condition::Rain,
        Condition::Snow,
        Condition::Thunder,
    ];
}

/// Map an Open-Meteo WMO weather code to a display category (DESIGN.md section 4).
///
/// Unknown codes fall back to `Overcast` rather than failing, so a new code from the API can
/// never crash the display.
pub fn condition_from_wmo(code: i64) -> Condition {
    match code {
        0 | 1 => Condition::Clear,
        2 => Condition::PartlyCloudy,
        3 => Condition::Overcast,
        45 | 48 => Condition::Fog,
        51..=67 | 80..=82 => Condition::Rain,
        71..=77 | 85 | 86 => Condition::Snow,
        95 | 96 | 99 => Condition::Thunder,
        _ => Condition::Overcast,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurrentWeather {
    pub temperature: f64, // in the configured unit
    pub code: i64,        // WMO code
    pub cloud_cover: i64, // percent
    pub wind_speed: f64,
    pub is_day: bool,
}

impl CurrentWeather {
    pub fn condition(&self) -> Condition {
        condition_from_wmo(self.code)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct HourlyPoint {
    pub time: Dt,
    pub code: i64,
    pub temperature: f64,
    pub precipitation_probability: i64,
}

impl HourlyPoint {
    pub fn condition(&self) -> Condition {
        condition_from_wmo(self.code)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DailyWeather {
    pub low: f64,
    pub high: f64,
    pub sunrise: Dt,
    pub sunset: Dt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WeatherSnapshot {
    pub fetched_at: Dt,
    pub current: CurrentWeather,
    pub hourly: Vec<HourlyPoint>,
    pub daily: DailyWeather,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HomeSnapshot {
    pub fetched_at: Dt,
    /// Unlocked, jammed, or in motion: needs attention.
    pub locks_unlocked: u32,
    /// State unknown; shown with a marker, not counted.
    pub locks_unavailable: u32,
    pub lights_on: u32,
    /// Entity ids of gates that are open.
    pub gates_open: Vec<String>,
    /// Gates whose state is unknown.
    pub gates_unavailable: Vec<String>,
    /// Electricity rate tier from a Home Assistant sensor ("discount", "off_peak", "on_peak",
    /// "critical_peak"). `None`: not configured, so no bolt is drawn. Anything else the sensor
    /// reports (including "unavailable") is kept and shown as an unknown tier.
    pub power_tier: Option<String>,
    /// Home Assistant `update.*` entities that are `on` (an update is waiting to be installed).
    pub updates_pending: u32,
}

impl HomeSnapshot {
    pub fn new(fetched_at: Dt) -> HomeSnapshot {
        HomeSnapshot {
            fetched_at,
            locks_unlocked: 0,
            locks_unavailable: 0,
            lights_on: 0,
            gates_open: Vec::new(),
            gates_unavailable: Vec::new(),
            power_tier: None,
            updates_pending: 0,
        }
    }
}

/// What one frame shows. The snapshots are shared (`Arc`): building the state for a frame, and
/// keeping the previous one for the scene's cross-fade, copies no forecast data. Edit one in place
/// with `Arc::make_mut`.
#[derive(Clone, Debug, PartialEq)]
pub struct AppState {
    pub now: Dt,
    pub weather: Option<Arc<WeatherSnapshot>>,
    pub home: Option<Arc<HomeSnapshot>>,
}

impl AppState {
    pub fn new(now: Dt) -> AppState {
        AppState { now, weather: None, home: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_mapping() {
        let cases = [
            (0, Condition::Clear), (1, Condition::Clear), (2, Condition::PartlyCloudy),
            (3, Condition::Overcast), (45, Condition::Fog), (48, Condition::Fog),
            (51, Condition::Rain), (67, Condition::Rain), (81, Condition::Rain),
            (71, Condition::Snow), (77, Condition::Snow), (86, Condition::Snow),
            (95, Condition::Thunder), (99, Condition::Thunder), (1234, Condition::Overcast),
        ];
        for (code, expected) in cases {
            assert_eq!(condition_from_wmo(code), expected, "code {code}");
        }
    }
}
