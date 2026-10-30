//! Text and icon widgets. Each draws into a frame within its region: `(frame, rect, state, t)`.

use chrono::{Duration, Timelike};

use super::fonts::{draw_text, text_width, HEIGHT};
use super::icons;
use crate::forecast;
use crate::gfx::{Img, Rgb};
use crate::layout::Rect;
use crate::state::{AppState, Condition, Dt, WeatherSnapshot};

pub const LOW_COLOR: Rgb = [80, 150, 255];
pub const HIGH_COLOR: Rgb = [255, 140, 40];
pub const LABEL_COLOR: Rgb = [200, 200, 210];
pub const DIM: Rgb = [60, 60, 70];
pub const RED: Rgb = [255, 40, 30];
pub const AMBER: Rgb = [255, 170, 0];
pub const GREEN: Rgb = [30, 220, 70];
pub const BULB_ON: Rgb = [255, 215, 90];
pub const LOCK_OK: Rgb = [110, 130, 150];
pub const BLUE: Rgb = [80, 150, 255];
pub const ORANGE: Rgb = [255, 140, 40];
pub const YELLOW: Rgb = [255, 230, 0];

/// Electricity rate tier -> bolt colour. Unknown or unavailable tiers show a dim bolt.
pub fn tier_color(tier: &str) -> Rgb {
    match tier {
        "discount" => GREEN,
        "off_peak" => BLUE,
        "on_peak" => ORANGE,
        "critical_peak" => RED,
        _ => DIM,
    }
}

/// Gate and bolt x offsets within the home row when the bolt is shown: 9 px gate, 3 px gap, 7 px bolt.
pub const BOLT_GATE_X: i32 = 41;
pub const BOLT_X: i32 = 53;

fn home_stale() -> Duration {
    Duration::minutes(5)
}

fn weather_stale() -> Duration {
    Duration::hours(3)
}

fn condition_name(cond: Condition) -> &'static str {
    match cond {
        Condition::Clear => "CLEAR",
        Condition::PartlyCloudy => "PARTLY",
        Condition::Overcast => "CLOUDY",
        Condition::Fog => "FOG",
        Condition::Rain => "RAIN",
        Condition::Snow => "SNOW",
        Condition::Thunder => "STORM",
    }
}

fn text_y(rect: Rect) -> i32 {
    rect.y + (rect.h - HEIGHT) / 2
}

fn center_x(rect: Rect, width: i32, cx: Option<i32>) -> i32 {
    let cx = cx.unwrap_or(rect.x + rect.w / 2);
    rect.x.max(cx - width / 2)
}

pub fn hour_label(dt: &Dt) -> String {
    let h = match dt.hour() % 12 {
        0 => 12,
        h => h,
    };
    format!("{h}{}", if dt.hour() < 12 { 'A' } else { 'P' })
}

pub fn draw_temps(frame: &mut Img, rect: Rect, state: &AppState, _t: f64) {
    let y = text_y(rect);
    let Some(weather) = &state.weather else {
        for cx in [rect.x + 16, rect.x + 48] {
            draw_text(frame, center_x(rect, text_width("--"), Some(cx)), y, "--", DIM);
        }
        return;
    };
    let (low, high) = forecast::daily_low_high(weather);
    for (cx, arrow, val, color) in [(rect.x + 16, 'v', low, LOW_COLOR), (rect.x + 48, '^', high, HIGH_COLOR)] {
        let text = format!("{arrow}{val}");
        draw_text(frame, center_x(rect, text_width(&text), Some(cx)), y, &text, color);
    }
}

/// True when `when` falls before today's sunrise or after today's sunset (local time of day).
fn is_night(weather: &WeatherSnapshot, when: &Dt) -> bool {
    let tod = when.with_timezone(&weather.daily.sunrise.timezone()).time();
    tod < weather.daily.sunrise.time() || tod >= weather.daily.sunset.time()
}

pub fn draw_next_change(frame: &mut Img, rect: Rect, state: &AppState, _t: f64) {
    let y = text_y(rect);
    let Some(weather) = &state.weather else {
        draw_text(frame, center_x(rect, text_width("--"), None), y, "--", DIM);
        return;
    };
    let (cond, text, night) = match forecast::next_change(weather, state.now) {
        None => {
            // "Steady" only for as far as the forecast reaches; nothing at all once it is all in the past.
            let Some(horizon) = forecast::forecast_horizon(weather, state.now) else {
                draw_text(frame, center_x(rect, text_width("--"), None), y, "--", DIM);
                return;
            };
            // The two-day request ends at 23:00 tomorrow, so late evening reaches 23 h: call that a full day.
            let hours = if horizon.num_hours() >= 23 { 24 } else { horizon.num_hours().clamp(1, 24) };
            (weather.current.condition(), format!("STEADY {hours}H"), !weather.current.is_day)
        }
        Some(change) => (
            change.condition,
            format!("{} {}", condition_name(change.condition), hour_label(&change.time)),
            is_night(weather, &change.time),
        ),
    };
    let width = 9 + 3 + text_width(&text);
    let x = center_x(rect, width, None);
    icons::draw_weather_icon(frame, x, rect.y + (rect.h - 9) / 2, cond, night);
    draw_text(frame, x + 12, y, &text, LABEL_COLOR);
}

fn count(n: u32) -> String {
    n.min(99).to_string()
}

#[allow(clippy::too_many_arguments)]
fn icon_count(frame: &mut Img, cell_x: i32, rect: Rect, icon: icons::Icon, icon_w: i32,
              icon_color: Rgb, text: &str, text_color: Rgb, cell: i32) {
    let width = icon_w + 2 + text_width(text);
    let x = cell_x + 0.max((cell - width) / 2);
    icons::blit(frame, x, rect.y + 1, icon, Some(icon_color));
    draw_text(frame, x + icon_w + 2, rect.y + 3, text, text_color);
}

pub fn draw_home(frame: &mut Img, rect: Rect, state: &AppState, t: f64) {
    let home = state.home.as_ref();
    // With the rate bolt, the lock and bulb (which have digits) get 19 px cells and the
    // gate and bolt (which do not) sit close together in the remaining 26 px; without
    // it the row is the original three 21 px cells. Icons and digits keep their size.
    let bolt = home.is_some_and(|h| h.power_tier.is_some());
    let cell = if bolt { 19 } else { 21 };
    let (c0, c1, c2) = (rect.x, rect.x + cell, rect.x + 2 * cell);
    let gate_x = if bolt { rect.x + BOLT_GATE_X } else { c2 + (21 - 9) / 2 };
    match home {
        None => {
            for cx in [c0, c1, c2] {
                draw_text(frame, cx + 9, rect.y + 3, "--", DIM);
            }
        }
        Some(home) => {
            // locks
            let n = home.locks_unlocked;
            let color = if n > 0 { RED } else { DIM };
            let mut text = count(n);
            if home.locks_unavailable > 0 {
                text.push('?');
            }
            let icon_color = if n > 0 { color } else { LOCK_OK };
            let text_color = if n == 0 && home.locks_unavailable > 0 { AMBER } else { color };
            icon_count(frame, c0, rect, icons::lock(n > 0), 7, icon_color, &text, text_color, cell);
            // lights
            let n = home.lights_on;
            let lit = if n > 0 { BULB_ON } else { DIM };
            icon_count(frame, c1, rect, icons::BULB, 7, lit, &count(n), lit, cell);
            // gate
            if !home.gates_open.is_empty() {
                let on = (t as i64) % 2 == 0;
                let color = if on { RED } else { [70, 12, 10] };
                icons::blit(frame, gate_x, rect.y + 1, icons::gate(true), Some(color));
            } else if !home.gates_unavailable.is_empty() {
                icons::blit(frame, gate_x, rect.y + 1, icons::gate(false), Some(AMBER));
            } else {
                icons::blit(frame, gate_x, rect.y + 1, icons::gate(false), Some(GREEN));
            }
            if bolt {
                let tier = home.power_tier.as_deref().unwrap_or("");
                icons::blit(frame, rect.x + BOLT_X, rect.y + 1, icons::BOLT, Some(tier_color(tier)));
            }
        }
    }
    staleness_dot(frame, rect, state);
    maintenance_dot(frame, rect, state);
}

/// Yellow pixel left of the staleness dot while Home Assistant has updates waiting.
fn maintenance_dot(frame: &mut Img, rect: Rect, state: &AppState) {
    if state.home.as_ref().is_some_and(|h| h.updates_pending > 0) {
        frame.put(rect.x2() - 2, rect.y2() - 1, YELLOW);
    }
}

fn staleness_dot(frame: &mut Img, rect: Rect, state: &AppState) {
    let now = state.now;
    let color = if state.weather.as_ref().is_some_and(|w| now - w.fetched_at > weather_stale()) {
        RED
    } else if state.home.as_ref().is_some_and(|h| now - h.fetched_at > home_stale()) {
        AMBER
    } else if state.weather.is_none() || state.home.is_none() {
        [90, 90, 90]
    } else {
        return;
    };
    frame.put(rect.x2() - 1, rect.y2() - 1, color);
}

pub type Widget = fn(&mut Img, Rect, &AppState, f64);

/// Every widget with the region it draws in.
pub fn widgets() -> [(&'static str, Widget); 3] {
    [("temps", draw_temps), ("next_change", draw_next_change), ("home", draw_home)]
}
