//! The weather scene: regression images (`UPDATE_GOLDEN=1 cargo test --test scene` rewrites them), plus behaviour.

mod common;

use chrono::{Duration, TimeZone};
use common::*;
use matrix_display::gfx::Img;
use matrix_display::layout;
use matrix_display::samples::{sample_names, sample_state, sample_state_at, NOON, TZ};
use matrix_display::scene::palette::sun_position;
use matrix_display::scene::{mountains, particles, render_state, SceneRenderer};
use matrix_display::state::{AppState, Condition, Dt};
use serde_json::Value;

fn state_for(case: &Value) -> AppState {
    let d: Vec<u32> = case["date"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u32).collect();
    let now = TZ.with_ymd_and_hms(d[0] as i32, d[1], d[2], d[3], d[4], 0).unwrap();
    sample_state_at(case["sample"].as_str().unwrap(), now)
}

#[test]
fn matches_the_golden_renders() {
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(golden_dir().join("scene-cases.json")).unwrap()).unwrap();
    assert!(cases.len() > 40);
    let mut failures = Vec::new();
    for case in &cases {
        let file = case["file"].as_str().unwrap();
        let img = if let Some(steps) = case.get("sequence") {
            let mut renderer = SceneRenderer::new();
            let mut last = None;
            for step in steps.as_array().unwrap() {
                last = Some(renderer.render(&state_for(step), step["t"].as_f64().unwrap()));
            }
            last.unwrap()
        } else {
            render_state(&state_for(case), case["t"].as_f64().unwrap())
        };
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            save_png(&img, &golden_dir().join(file));
            continue;
        }
        let want = load_png(&golden_dir().join(file));
        if let Some(bbox) = diff_bbox(&img, &want) {
            let n = img.data.iter().zip(&want.data).filter(|(a, b)| a != b).count();
            failures.push(format!("{file}: {n} pixels differ within {bbox:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn with_current(state: &AppState, edit: impl FnOnce(&mut matrix_display::state::CurrentWeather)) -> AppState {
    let mut s = state.clone();
    edit(&mut std::sync::Arc::make_mut(s.weather.as_mut().unwrap()).current);
    s
}

#[test]
fn every_sample_renders_and_animates() {
    for name in sample_names() {
        let state = sample_state(name);
        let (a, b) = (render_state(&state, 0.0), render_state(&state, 2.5));
        assert_eq!((a.w, a.h), (64, 35), "{name}");
        if name != "clear_day" && name != "overcast" {
            assert!(diff_bbox(&a, &b).is_some(), "{name} should move"); // those two drift only slightly
        }
        assert!(count_distinct(&a) > 12, "{name} is a flat fill");
    }
}

#[test]
fn all_conditions_covered_by_samples() {
    let seen: std::collections::HashSet<Condition> =
        sample_names().iter().map(|n| sample_state(n).weather.unwrap().current.condition()).collect();
    assert_eq!(seen.len(), Condition::ALL.len());
}

#[test]
fn pure_function_of_state_and_t() {
    let state = sample_state("rain_then_snow");
    assert_eq!(render_state(&state, 4.2), render_state(&state, 4.2));
}

#[test]
fn draw_scene_stays_inside_rect() {
    let mut frame = Img::new(64, 64, [1, 2, 3]);
    SceneRenderer::new().draw(&mut frame, layout::SCENE, &sample_state("clear_day"), 0.0);
    for y in layout::SCENE.y2() as usize..64 {
        for x in 0..64 {
            assert_eq!(frame.get(x, y), [1, 2, 3]);
        }
    }
    assert_ne!(frame.get(10, 10), [1, 2, 3]);
}

#[test]
fn no_weather_draws_dim_neutral_scene() {
    let img = render_state(&AppState::new(*NOON), 0.0);
    let mut sum = 0u32;
    for y in 0..12 {
        for x in 0..40 {
            sum += img.get(x, y).iter().map(|c| *c as u32).sum::<u32>();
        }
    }
    assert!((sum as f64 / (40.0 * 12.0 * 3.0)) < 80.0);
}

fn region_sum(img: &Img, x0: usize, y0: usize, x1: usize, y1: usize) -> u64 {
    (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| img.get(x, y).iter().map(|c| *c as u64).sum::<u64>()).sum()
}

#[test]
fn day_is_brighter_than_night() {
    let day = region_sum(&render_state(&sample_state("clear_day"), 0.0), 0, 8, 40, 14);
    let night = region_sum(&render_state(&sample_state("clear_night"), 0.0), 0, 8, 40, 14);
    assert!(day > 3 * night);
}

#[test]
fn temperature_changes_pixels_top_right() {
    let s = sample_state("clear_day");
    let a = render_state(&s, 0.0);
    let b = render_state(&with_current(&s, |c| c.temperature = -8.0), 0.0);
    let (x0, _, _, y1) = diff_bbox(&a, &b).expect("temperature is drawn");
    assert!(x0 >= 40 && y1 <= 8);
}

#[test]
fn wind_and_cloud_cover_extremes_do_not_crash() {
    let s = sample_state("partly_cloudy");
    for wind in [0.0, 15.0, 80.0] {
        for cover in [0, 50, 100] {
            render_state(&with_current(&s, |c| { c.wind_speed = wind; c.cloud_cover = cover; }), 7.0);
        }
    }
    render_state(&with_current(&s, |c| { c.temperature = 123.4; c.code = 1234; }), 1.0); // unknown WMO code
}

#[test]
fn sunrise_and_sunset_follow_the_clock() {
    let d = sample_state("clear_day").weather.unwrap().daily.clone();
    assert!(sun_position(*NOON, true, d.sunrise, d.sunset).elev > 0.9);
    assert!(sun_position(d.sunrise, true, d.sunrise, d.sunset).elev.abs() < 0.01);
    let at_hour = |h| -> Dt { chrono::Timelike::with_hour(&*NOON, h).unwrap() };
    assert!(sun_position(at_hour(23), false, d.sunrise, d.sunset).elev < -0.5);
    assert!(sun_position(at_hour(2), false, d.sunrise, d.sunset).elev < -0.5);
    // stale snapshot from yesterday still maps onto today's clock
    assert!(sun_position(*NOON + Duration::days(1), true, d.sunrise, d.sunset).elev > 0.9);
}

#[test]
fn lightning_is_deterministic_and_occasional() {
    // A flash peaks for well under 10 ms, so sample finely (200 Hz) over two minutes rather than hope to land on one.
    let levels: Vec<f64> = (0..200 * 120).map(|i| particles::flash_level(i as f64 / 200.0)).collect();
    assert!(levels.iter().cloned().fold(0.0, f64::max) > 0.9);
    assert!((levels.iter().filter(|v| **v > 0.3).count() as f64) < levels.len() as f64 * 0.15);
    assert_eq!(particles::flash_level(12.34), particles::flash_level(12.34));
}

#[test]
fn cross_fade_blends_for_two_seconds() {
    let mut r = SceneRenderer::new();
    let (clear, rain) = (sample_state("clear_day"), sample_state("rain_then_snow"));
    r.render(&clear, 10.0);
    let start = r.render(&rain, 10.5); // change observed here: starts as the old scene
    let mid = r.render(&rain, 11.5);
    let end = r.render(&rain, 12.6);
    assert_eq!(start, render_state(&clear, 10.5)); // alpha 0: still the old scene
    assert_eq!(end, render_state(&rain, 12.6));
    assert_ne!(mid, render_state(&rain, 11.5));
}

#[test]
fn time_jump_resets_instead_of_fading() {
    let mut r = SceneRenderer::new();
    r.render(&sample_state("clear_day"), 1.0);
    let rain = sample_state("rain_then_snow");
    assert_eq!(r.render(&rain, 100.0), render_state(&rain, 100.0));
}

#[test]
fn frame_cost_is_low() {
    let state = sample_state("thunder");
    render_state(&state, 0.0); // warm the caches
    let t0 = std::time::Instant::now();
    for i in 0..30 {
        render_state(&state, i as f64 / 15.0);
    }
    let per_frame = t0.elapsed().as_secs_f64() / 30.0;
    eprintln!("scene frame: {:.2} ms", per_frame * 1000.0);
    // 66 ms is the budget at 15 fps; a release build is far below it, a debug build still under
    assert!(per_frame < 0.066, "{per_frame}");
}

#[test]
fn date_overlay_is_top_left_and_readable_day_and_night() {
    for name in ["clear_day", "clear_night"] {
        let img = render_state(&sample_state(name), 0.0);
        let mut bbox: Option<(usize, usize)> = None;
        for y in 0..8 {
            for x in 0..36 {
                if img.get(x, y).iter().all(|c| *c > 240) {
                    bbox = Some(bbox.map_or((x, y), |(bx, by)| (bx.min(x), by.min(y))));
                }
            }
        }
        let (x, y) = bbox.unwrap_or_else(|| panic!("{name}: no white text"));
        assert!(x >= 2 && y >= 2, "{name}: text must clear the edge");
        assert!(img.get(1, 3).iter().all(|c| *c < 200), "{name}: outline darkens the sky");
    }
}

#[test]
fn date_changes_with_the_day() {
    let s = sample_state("clear_day");
    let mut other = s.clone();
    other.now = s.now + Duration::days(1);
    let (x0, _, _, y1) = diff_bbox(&render_state(&s, 0.0), &render_state(&other, 0.0)).expect("date is drawn");
    assert!(x0 < 40 && y1 <= 8);
}

#[test]
fn ridge_colours_follow_the_season() {
    let avg = |doy: u32| -> [f64; 3] {
        let t = mountains::terrain(40, Some(Condition::Clear), mountains::season_key(doy));
        let (rgb, mask) = &*t;
        let px: Vec<_> = rgb.data.iter().zip(&mask.data).filter(|(_, m)| **m > 0).map(|(p, _)| *p).collect();
        let mut out = [0.0; 3];
        for k in 0..3 {
            out[k] = px.iter().map(|p| p[k] as f64).sum::<f64>() / px.len() as f64;
        }
        out
    };
    let (summer, october, winter) = (avg(190), avg(293), avg(15));
    assert!(summer[1] > summer[0] * 1.15); // green dominates in summer
    assert!(october[0] > summer[0] + 20.0); // reds and oranges in October
    assert!((winter[1] - winter[0]).abs() < 25.0); // bare browns and greys in winter
}

fn changed_fraction(name: &str, seconds: f64, step: f64) -> f64 {
    use matrix_display::compositor::build_default_compositor;
    let mut comp = build_default_compositor(true, false);
    let st = sample_state(name);
    let frames: Vec<Img> =
        (0..(seconds / step) as usize).map(|i| comp.render(&st, i as f64 * step).crop_rows(0, 35)).collect();
    let mut changed = std::collections::HashSet::new();
    for pair in frames.windows(2) {
        for y in 0..35 {
            for x in 0..64 {
                if pair[0].get(x, y) != pair[1].get(x, y) {
                    changed.insert((x, y));
                }
            }
        }
    }
    changed.len() as f64 / (64.0 * 35.0)
}

#[test]
fn clear_day_is_not_frozen() {
    assert!(changed_fraction("clear_day", 10.0, 0.25) > 0.03);
}

#[test]
fn clear_night_twinkles_and_has_shooting_star() {
    assert!(changed_fraction("clear_night", 25.0, 0.25) > 0.03);
}

#[test]
fn deterministic_for_same_time() {
    use matrix_display::compositor::build_default_compositor;
    let st = sample_state("clear_night");
    let a = build_default_compositor(true, false).render(&st, 7.3);
    let b = build_default_compositor(true, false).render(&st, 7.3);
    assert_eq!(a, b);
}
