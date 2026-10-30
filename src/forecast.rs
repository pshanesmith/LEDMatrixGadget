//! Pure forecast helpers shared by the scene and the widgets.

use chrono::Duration;

use crate::state::{Condition, Dt, WeatherSnapshot};

fn lookahead() -> Duration {
    Duration::hours(24)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NextChange {
    /// Start of the first forecast hour with a different condition.
    pub time: Dt,
    /// The condition it changes to.
    pub condition: Condition,
}

/// Today's (low, high), rounded to whole degrees in the configured unit.
pub fn daily_low_high(weather: &WeatherSnapshot) -> (i64, i64) {
    (
        crate::gfx::pyround(weather.daily.low),
        crate::gfx::pyround(weather.daily.high),
    )
}

/// First forecast hour after `now`, within 24 h, whose condition differs from the current one.
pub fn next_change(weather: &WeatherSnapshot, now: Dt) -> Option<NextChange> {
    let current = weather.current.condition();
    for point in &weather.hourly {
        if point.time <= now {
            continue;
        }
        if point.time - now > lookahead() {
            break;
        }
        if point.condition() != current {
            return Some(NextChange { time: point.time, condition: point.condition() });
        }
    }
    None
}

/// How far past `now` the hourly forecast reaches (its last hour), or `None` when every hour is in
/// the past (an old cache, a long outage): then nothing can be said about what comes next.
pub fn forecast_horizon(weather: &WeatherSnapshot, now: Dt) -> Option<Duration> {
    let last = weather.hourly.iter().map(|p| p.time).max()?;
    (last > now).then(|| last - now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples::{sample_state, NOON};

    #[test]
    fn low_high_rounded() {
        let w = sample_state("clear_day").weather.unwrap();
        assert_eq!(daily_low_high(&w), (48, 71));
    }

    #[test]
    fn no_change_returns_none() {
        assert!(next_change(&sample_state("clear_day").weather.unwrap(), *NOON).is_none());
    }

    #[test]
    fn finds_first_different_hour() {
        let s = sample_state("rain_then_snow"); // rain now, snow from hour 6
        let nc = next_change(s.weather.as_ref().unwrap(), s.now).unwrap();
        assert_eq!(nc.condition, Condition::Snow);
        assert_eq!(nc.time, *NOON + Duration::hours(6));
    }

    #[test]
    fn change_beyond_24h_ignored() {
        let s = sample_state("snow"); // change at hour 9 (within 24h) -> found
        let nc = next_change(s.weather.as_ref().unwrap(), s.now).unwrap();
        assert_eq!(nc.condition, Condition::Overcast);
        let far = sample_state("clear_day").weather.unwrap();
        assert!(next_change(&far, *NOON - Duration::hours(30)).is_none());
    }

    #[test]
    fn horizon_is_none_once_every_hour_has_passed() {
        let w = sample_state("clear_day").weather.unwrap(); // hours NOON .. NOON+47h
        assert_eq!(forecast_horizon(&w, *NOON), Some(Duration::hours(47)));
        assert_eq!(forecast_horizon(&w, *NOON + Duration::minutes(46 * 60 + 30)), Some(Duration::minutes(30)));
        assert_eq!(forecast_horizon(&w, *NOON + Duration::hours(47)), None);
    }

    #[test]
    fn hours_at_or_before_now_are_skipped() {
        let s = sample_state("rain_then_snow"); // snow hour starts at NOON+6h
        let nc = next_change(s.weather.as_ref().unwrap(), *NOON + Duration::hours(6)).unwrap();
        assert_eq!(nc.time, *NOON + Duration::hours(7));
    }
}
