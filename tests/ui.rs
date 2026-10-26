//! UI widget tests: golden images (scene excluded, so they are deterministic), text overflow,
//! icons, brightness and the electricity-tier bolt. Regenerate goldens with UPDATE_GOLDEN=1.

mod common;

use chrono::Timelike;
use common::*;
use matrix_display::compositor::{brightness_for, build_default_compositor};
use matrix_display::config::DisplayConfig;
use matrix_display::data::homeassistant as ha;
use matrix_display::config::HomeAssistantConfig;
use matrix_display::gfx::Img;
use matrix_display::layout;
use matrix_display::samples::{sample_state, NOON};
use matrix_display::state::AppState;
use matrix_display::ui::fonts::text_width;
use matrix_display::ui::{icons, widgets};
use serde_json::json;

const GOLDEN_CASES: [&str; 16] = [
    "all_clear", "rain_then_snow", "counts_zero", "counts_twelve", "gate_open", "weather_none",
    "stale_home", "stale_weather", "tier_discount", "tier_off_peak", "tier_on_peak", "tier_critical",
    "tier_unavailable", "tier_counts_twelve", "tier_gate_open", "updates_pending",
];

fn render(state: &AppState, t: f64) -> Img {
    build_default_compositor(false, false).render(state, t)
}

#[test]
fn matches_the_python_goldens() {
    let mut failures = Vec::new();
    for name in GOLDEN_CASES {
        let img = render(&variant(name), 0.0);
        let want = load_png(&golden_dir().join(format!("ui-{name}.png")));
        if let Some(bbox) = diff_bbox(&img, &want) {
            failures.push(format!("ui-{name}: differs within {bbox:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn gate_blinks() {
    let state = variant("gate_open");
    assert!(diff_bbox(&render(&state, 0.0), &render(&state, 1.0)).is_some());
}

#[test]
fn none_state_does_not_panic() {
    render(&AppState::new(*NOON), 0.0);
}

#[test]
fn all_widgets_registered() {
    let comp = build_default_compositor(false, false);
    for region in ["temps", "next_change", "home"] {
        assert!(comp.has_drawer(region));
    }
    assert!(!comp.has_drawer("scene"));
}

#[test]
fn compositor_blank_debug_and_register() {
    use matrix_display::compositor::Compositor;
    let state = sample_state("clear_day");
    assert!(Compositor::new(false).render(&state, 0.0).is_blank());
    assert!(!Compositor::new(true).render(&state, 0.0).is_blank());
    let mut comp = Compositor::new(false);
    comp.register("temps", Box::new(|f, r, _, _| f.put(r.x, r.y, [255, 0, 0]))).unwrap();
    assert_eq!(comp.render(&state, 0.0).get(0, layout::TEMPS.y as usize), [255, 0, 0]);
    assert!(comp.register("nope", Box::new(|_, _, _, _| {})).is_err());
}

#[test]
fn widgets_stay_inside_their_region() {
    for (low, high) in [(-40.0, 130.0), (-9.0, 9.0), (0.0, 100.0), (-100.0, 999.0), (5.0, 15.0)] {
        for count in [0u32, 7, 12, 99, 250] {
            for t in [0.0, 1.0] {
                let mut state = variant("rain_then_snow");
                {
                    let w = state.weather.as_mut().unwrap();
                    w.daily.low = low;
                    w.daily.high = high;
                    let h = state.home.as_mut().unwrap();
                    h.locks_unlocked = count;
                    h.lights_on = count;
                    h.locks_unavailable = 1;
                    h.gates_open = vec!["g".into()];
                }
                for (name, drawer) in widgets::widgets() {
                    let rect = layout::region(name).unwrap();
                    let mut frame = Img::black(64, 64);
                    drawer(&mut frame, rect, &state, t);
                    let (x0, y0, x1, y1) = content_bbox(&frame).unwrap_or_else(|| panic!("{name} drew nothing"));
                    assert!(x0 as i32 >= rect.x && y0 as i32 >= rect.y, "{name} {low} {high} {count}");
                    assert!(x1 as i32 <= rect.x2() && y1 as i32 <= rect.y2(), "{name} {low} {high} {count}");
                }
            }
        }
    }
}

#[test]
fn text_widths_fit() {
    for text in ["FRI 10/23", "WED 12/31", "THUNDER 11A", "STEADY 24H", "PARTLY 12P", "v-100", "^999", "99?", "STORM 12A"] {
        assert!(text_width(text) <= 64, "{text}");
    }
    assert!(9 + 3 + text_width("PARTLY 12P") <= 64);
}

#[test]
fn brightness() {
    let cfg = |brightness: u8| DisplayConfig { brightness, max_brightness: 70, ..DisplayConfig::default() };
    let sample = sample_state("clear_day");
    let daily = sample.weather.as_ref().map(|w| &w.daily); // sunrise 07:40, sunset 18:20
    let hour = |h| NOON.with_hour(h).unwrap();
    assert_eq!(brightness_for(*NOON, daily, &cfg(50)), 50);
    assert_eq!(brightness_for(hour(22), daily, &cfg(50)), 25);
    assert_eq!(brightness_for(hour(5), daily, &cfg(50)), 25);
    assert_eq!(brightness_for(*NOON, None, &cfg(50)), 50);
    assert_eq!(brightness_for(*NOON, daily, &cfg(90)), 70);
    assert!(brightness_for(hour(22), daily, &cfg(1)) >= 1);
}

fn icon_colors(name: &str) -> std::collections::HashSet<[u8; 3]> {
    let state = sample_state(name);
    let mut frame = Img::black(64, 64);
    widgets::draw_next_change(&mut frame, layout::NEXT_CHANGE, &state, 0.0);
    let r = layout::NEXT_CHANGE;
    (r.x..r.x + 20).flat_map(|x| (r.y..r.y2()).map(move |y| (x, y))).map(|(x, y)| frame.get(x as usize, y as usize)).collect()
}

const SUN: [u8; 3] = [255, 200, 20];
const MOON: [u8; 3] = [235, 230, 190];

#[test]
fn clear_night_shows_moon_not_sun() {
    let colors = icon_colors("clear_night");
    assert!(colors.contains(&MOON) && !colors.contains(&SUN));
}

#[test]
fn clear_day_shows_sun_not_moon() {
    let colors = icon_colors("clear_day");
    assert!(colors.contains(&SUN) && !colors.contains(&MOON));
}

// --- the electricity rate tier ---------------------------------------------------------------

const TIER: &str = "sensor.duke_energy_r_tou_cpp_tier";

fn ha_config(tier_entity: &str) -> HomeAssistantConfig {
    HomeAssistantConfig {
        url: "http://ha.local".into(),
        token: "t".into(),
        gates: vec!["cover.gate".into()],
        power_tier_entity: tier_entity.into(),
        ..HomeAssistantConfig::default()
    }
}

fn payload(extra: serde_json::Value) -> serde_json::Value {
    let mut base = json!({"locks": [], "lights": ["light.a"], "gates": ["off"]});
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

#[test]
fn tier_is_read_from_the_sensor() {
    for state in ["discount", "off_peak", "on_peak", "critical_peak", "unavailable"] {
        let snap = ha::parse_template_result(&payload(json!({"tier": state})), &ha_config(TIER), None).unwrap();
        assert_eq!(snap.power_tier.as_deref(), Some(state));
    }
}

#[test]
fn not_configured_means_no_bolt_even_if_the_key_is_present() {
    let snap = ha::parse_template_result(&payload(json!({"tier": "on_peak"})), &ha_config(""), None).unwrap();
    assert_eq!(snap.power_tier, None);
}

#[test]
fn configured_but_missing_tier_is_unknown() {
    for extra in [json!({}), json!({"tier": null}), json!({"tier": ""})] {
        let snap = ha::parse_template_result(&payload(extra), &ha_config(TIER), None).unwrap();
        assert_eq!(snap.power_tier.as_deref(), Some("unknown"));
    }
}

#[test]
fn template_names_the_entity_and_rejects_bad_ids() {
    let gates = vec!["cover.gate".to_string()];
    let template = ha::build_template(&gates, TIER).unwrap();
    assert!(template.contains(&format!("\"{TIER}\"")) && template.contains("states(tier_entity)"));
    assert!(ha::build_template(&gates, "").unwrap().contains("set tier_entity = \"\""));
    assert!(ha::build_template(&[], "sensor.x') }}{{ evil").is_err());
}

#[test]
fn cache_round_trip_keeps_the_tier() {
    let snap = ha::parse_template_result(&payload(json!({"tier": "critical_peak"})), &ha_config(TIER), None).unwrap();
    let mut data = ha::snapshot_to_json(&snap);
    assert_eq!(ha::snapshot_from_json(&data).unwrap().power_tier.as_deref(), Some("critical_peak"));
    data.as_object_mut().unwrap().remove("power_tier"); // a cache written before the bolt existed
    assert_eq!(ha::snapshot_from_json(&data).unwrap().power_tier, None);
}

fn bolt_pixels(state: &AppState) -> std::collections::HashSet<[u8; 3]> {
    let img = render(state, 0.0);
    let (x0, y0) = (layout::HOME.x + widgets::BOLT_X, layout::HOME.y + 1);
    let mut out = std::collections::HashSet::new();
    for (dy, row) in icons::BOLT.iter().enumerate() {
        for (dx, ch) in row.bytes().enumerate() {
            if ch != b'.' {
                out.insert(img.get((x0 + dx as i32) as usize, (y0 + dy as i32) as usize));
            }
        }
    }
    out
}

#[test]
fn bolt_colour_follows_the_tier() {
    for (name, color) in [
        ("tier_discount", widgets::GREEN), ("tier_off_peak", widgets::BLUE), ("tier_on_peak", widgets::ORANGE),
        ("tier_critical", widgets::RED), ("tier_unavailable", widgets::DIM),
    ] {
        assert_eq!(bolt_pixels(&variant(name)), [color].into_iter().collect(), "{name}");
    }
}

#[test]
fn tier_colours_are_the_requested_ones() {
    assert!(widgets::tier_color("discount")[1] > 200); // green
    assert!(widgets::tier_color("off_peak")[2] > 200); // blue
    let [r, g, b] = widgets::tier_color("on_peak");
    assert!(r > 200 && (101..180).contains(&g) && b < 80); // orange
    let [r, g, b] = widgets::tier_color("critical_peak");
    assert!(r > 200 && g < 80 && b < 80); // red
}

#[test]
fn no_bolt_without_a_tier_and_layout_is_the_old_three_cells() {
    let base = variant("counts_zero");
    assert!(base.home.as_ref().unwrap().power_tier.is_none());
    let mut with_tier = base.clone();
    with_tier.home.as_mut().unwrap().power_tier = Some("on_peak".into());
    let (a, b) = (render(&base, 0.0), render(&with_tier, 0.0));
    assert_ne!(a, b);
    // gate icon sits in the middle of the third 21 px cell without the bolt
    assert_eq!(a.get((layout::HOME.x + 42 + 6) as usize, (layout::HOME.y + 1) as usize), widgets::GREEN);
}

#[test]
fn gate_and_bolt_are_close_together_but_clear_of_the_counts() {
    let img = render(&variant("tier_counts_twelve"), 0.0);
    let lit = |x: i32| (layout::HOME.y + 1..layout::HOME.y + 10).any(|y| img.get(x as usize, y as usize) != [0, 0, 0]);
    let gate_right = widgets::BOLT_GATE_X + 9;
    assert_eq!(widgets::BOLT_X - gate_right, 3); // tight, as the gate has no digits
    assert!(!(gate_right..widgets::BOLT_X).any(lit));
    assert!(lit(widgets::BOLT_GATE_X) && lit(widgets::BOLT_X + 3));
    // two-digit lock and bulb counts never reach the gate
    assert!(!(widgets::BOLT_GATE_X - 2..widgets::BOLT_GATE_X).any(lit));
}

#[test]
fn bolt_row_stays_inside_the_region() {
    for count in [0u32, 9, 12, 99, 250] {
        for t in [0.0, 1.0] {
            let mut state = variant("tier_counts_twelve");
            let h = state.home.as_mut().unwrap();
            h.locks_unlocked = count;
            h.lights_on = count;
            let img = render(&state, t);
            let mut empty = state.clone();
            empty.home = None;
            let reference = render(&empty, t);
            for y in 0..layout::HOME.y as usize {
                for x in 0..64 {
                    assert_eq!(img.get(x, y), reference.get(x, y));
                }
            }
        }
    }
}

// --- maintenance dot -------------------------------------------------------------------------

#[test]
fn pending_updates_are_counted_after_exclusions() {
    let mut cfg = ha_config("");
    cfg.exclude_updates = vec!["update.*_firmware".into()];
    let data = payload(json!({"updates": ["update.core", "update.hub_firmware", "update.addon"]}));
    assert_eq!(ha::parse_template_result(&data, &cfg, None).unwrap().updates_pending, 2);
    assert_eq!(ha::parse_template_result(&payload(json!({})), &cfg, None).unwrap().updates_pending, 0);
}

#[test]
fn updates_survive_the_cache_and_old_caches_load() {
    let snap = ha::parse_template_result(&payload(json!({"updates": ["update.core"]})), &ha_config(""), None).unwrap();
    let mut data = ha::snapshot_to_json(&snap);
    assert_eq!(ha::snapshot_from_json(&data).unwrap().updates_pending, 1);
    data.as_object_mut().unwrap().remove("updates_pending");
    assert_eq!(ha::snapshot_from_json(&data).unwrap().updates_pending, 0);
}

#[test]
fn maintenance_dot_is_yellow_beside_the_corner() {
    let state = variant("updates_pending");
    let mut quiet = state.clone();
    quiet.home.as_mut().unwrap().updates_pending = 0;
    for (st, expect) in [(state, widgets::YELLOW), (quiet, [0, 0, 0])] {
        let mut frame = Img::black(64, 64);
        widgets::draw_home(&mut frame, layout::HOME, &st, 0.0);
        assert_eq!(frame.get(62, 63), expect);
    }
}
