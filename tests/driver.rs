//! Guards on how the panel driver is pulled in (our fork of rpi_led_panel with the aarch64 timer fix).

mod common;

use common::root;

const FORK: &str = "https://github.com/pshanesmith/rpi_led_panel";

#[test]
fn the_driver_comes_from_the_fork_at_a_pinned_commit() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    let line = manifest.lines().find(|l| l.starts_with("rpi-led-panel")).expect("dependency line");
    assert!(line.contains(FORK) && line.contains("rev = \""), "{line}");
    let rev = line.split("rev = \"").nth(1).unwrap().split('"').next().unwrap();
    assert_eq!(rev.len(), 40, "pin a full commit hash, not a branch");
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).unwrap();
    let entry = lock.split("[[package]]").find(|p| p.contains("name = \"rpi-led-panel\"")).expect("in Cargo.lock");
    assert!(entry.contains(&format!("{FORK}?rev={rev}")), "Cargo.lock does not use the pinned fork commit:\n{entry}");
    assert!(!manifest.contains("[patch.crates-io]"), "the vendored copy was replaced by the fork");
    assert!(!root().join("vendor").exists());
}
