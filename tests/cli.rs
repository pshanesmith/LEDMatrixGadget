//! The command-line programs, run as a user would.

mod common;

use std::process::Command;

use common::*;

fn bin(name: &str) -> Command {
    let path = match name {
        "matrix-display" => env!("CARGO_BIN_EXE_matrix-display"),
        "matrix-preview" => env!("CARGO_BIN_EXE_matrix-preview"),
        "matrix-set-config" => env!("CARGO_BIN_EXE_matrix-set-config"),
        other => panic!("{other}"),
    };
    let mut c = Command::new(path);
    c.current_dir(root()).env_remove("MATRIX_HA_TOKEN").env_remove("MATRIX_STATE_DIR");
    c
}

#[test]
fn emulate_once_writes_png() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("frame.png");
    let r = bin("matrix-display")
        .args(["--emulate", "--once", "--scale", "1", "--config", "config/config.example.toml", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let img = load_png(&out);
    assert_eq!((img.w, img.h), (64, 64));
}

#[test]
fn offline_sample_png_matches_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("frame.png");
    let r = bin("matrix-display")
        .args(["--emulate", "--once", "--offline", "--sample", "thunder", "--scale", "1", "--config",
               "config/config.example.toml", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let mut comp = matrix_display::compositor::build_default_compositor(true, false);
    let want = comp.render(&matrix_display::samples::sample_state("thunder"), 0.0);
    assert_eq!(load_png(&out), want);
}

#[test]
fn emulated_run_saves_a_gif() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("demo.gif");
    let r = bin("matrix-display")
        .args(["--emulate", "--offline", "--sample", "rain_then_snow", "--seconds", "1", "--scale", "2",
               "--config", "config/config.example.toml", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    assert!(String::from_utf8_lossy(&r.stdout).contains("15 frames"));
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"GIF89a"));
}

#[test]
fn bad_config_exits_with_status_2() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("bad.toml");
    std::fs::write(&cfg, "[display]\nbogus = 1\n").unwrap();
    let r = bin("matrix-display").args(["--emulate", "--once", "--config"]).arg(&cfg).output().unwrap();
    assert_eq!(r.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&r.stderr).contains("unknown key(s) in [display]: bogus"));
    let missing = bin("matrix-display").args(["--once", "--config", "/nonexistent.toml"]).output().unwrap();
    assert_eq!(missing.status.code(), Some(2));
}

#[test]
fn without_the_hardware_feature_the_panel_is_refused_clearly() {
    if cfg!(feature = "hardware") {
        return;
    }
    let r = bin("matrix-display").args(["--offline", "--config", "config/config.example.toml"]).output().unwrap();
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("--emulate"));
}

#[test]
fn preview_renders_samples_and_variants() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("p.png");
    let r = bin("matrix-preview").args(["--sample", "fog", "--scale", "1", "--out"]).arg(&out).output().unwrap();
    assert!(r.status.success());
    assert_eq!(load_png(&out).w, 64);
    let r = bin("matrix-preview").args(["--all", "--scale", "1", "--out-dir"]).arg(dir.path()).output().unwrap();
    assert!(r.status.success());
    assert!(dir.path().join("ui-var-tier_critical.png").exists() && dir.path().join("ui-thunder.png").exists());
}

#[test]
fn set_config_tool_edits_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("c.toml");
    std::fs::write(&f, "[display]\npwm_bits = 11\n").unwrap();
    let r = bin("matrix-set-config").arg(&f).args(["display", "pwm_bits", "10"]).output().unwrap();
    assert!(r.status.success());
    assert!(std::fs::read_to_string(&f).unwrap().contains("pwm_bits = 10"));
    assert!(dir.path().join("c.toml.bak").exists());
    assert_eq!(bin("matrix-set-config").output().unwrap().status.code(), Some(2));
}

#[test]
fn fixtures_parse() {
    for name in ["weather_open_meteo_recorded.json", "ha_template_mixed.json", "ha_template_quiet.json",
                 "geocode_nominatim_10001_us.json", "geocode_openmeteo_10001.json", "geocode_openmeteo_10001_us.json"] {
        let text = std::fs::read_to_string(root().join("tests/fixtures").join(name)).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}
