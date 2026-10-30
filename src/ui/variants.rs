//! Extra `AppState`s (beyond samples.rs) that exercise the widgets' edge cases.

use std::sync::Arc;

use chrono::Duration;

use crate::samples::{sample_state, NOON};
use crate::state::{AppState, HomeSnapshot};

fn home(edit: impl FnOnce(&mut HomeSnapshot)) -> Option<Arc<HomeSnapshot>> {
    let mut h = HomeSnapshot::new(*NOON);
    edit(&mut h);
    Some(Arc::new(h))
}

fn with_home(mut state: AppState, h: Option<Arc<HomeSnapshot>>) -> AppState {
    state.home = h;
    state
}

pub fn variants() -> Vec<(&'static str, AppState)> {
    let clear = sample_state("clear_day");
    let rain = sample_state("rain_then_snow");
    let mut cold = clear.clone();
    {
        let daily = &mut Arc::make_mut(cold.weather.as_mut().unwrap()).daily;
        daily.low = -12.4;
        daily.high = 104.6;
    }
    let gate = || vec!["gate".to_string()];
    let r = |edit: &dyn Fn(&mut HomeSnapshot)| {
        let mut h = HomeSnapshot::new(*NOON);
        edit(&mut h);
        with_home(rain.clone(), Some(Arc::new(h)))
    };
    let mut stale_weather = rain.clone();
    Arc::make_mut(stale_weather.weather.as_mut().unwrap()).fetched_at = *NOON - Duration::hours(4);
    let mut cold_state = cold;
    cold_state.home = home(|h| h.lights_on = 1);
    vec![
        ("all_clear", with_home(clear, home(|_| {}))),
        ("rain_then_snow", rain.clone()),
        ("counts_zero", r(&|_| {})),
        ("counts_twelve", r(&|h| { h.locks_unlocked = 12; h.lights_on = 12; })),
        ("gate_open", r(&|h| { h.locks_unlocked = 1; h.lights_on = 3; h.gates_open = gate(); })),
        ("gate_unavailable", r(&|h| { h.locks_unavailable = 1; h.gates_unavailable = gate(); })),
        ("tier_discount", r(&|h| { h.lights_on = 3; h.power_tier = Some("discount".into()); })),
        ("tier_off_peak", r(&|h| { h.lights_on = 3; h.power_tier = Some("off_peak".into()); })),
        ("tier_on_peak", r(&|h| { h.locks_unlocked = 1; h.lights_on = 3; h.power_tier = Some("on_peak".into()); })),
        ("tier_critical", r(&|h| { h.lights_on = 3; h.power_tier = Some("critical_peak".into()); })),
        ("tier_unavailable", r(&|h| { h.lights_on = 3; h.power_tier = Some("unavailable".into()); })),
        ("tier_counts_twelve", r(&|h| { h.locks_unlocked = 12; h.lights_on = 12; h.power_tier = Some("on_peak".into()); })),
        ("tier_gate_open", r(&|h| {
            h.locks_unlocked = 1; h.lights_on = 3; h.gates_open = gate(); h.power_tier = Some("critical_peak".into());
        })),
        ("updates_pending", r(&|h| { h.lights_on = 3; h.updates_pending = 2; })),
        ("weather_none", with_home(AppState::new(*NOON), home(|h| h.lights_on = 2))),
        ("home_none", with_home(rain.clone(), None)),
        ("extreme_temps", cold_state),
        ("stale_home", r(&|h| { h.fetched_at = *NOON - Duration::minutes(10); h.lights_on = 4; })),
        ("stale_weather", stale_weather),
    ]
}
