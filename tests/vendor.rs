//! Guards on the patched copy of the panel driver (vendor/rpi-led-panel).

mod common;

use common::root;

#[test]
fn the_patched_driver_is_what_cargo_builds() {
    let manifest = std::fs::read_to_string(root().join("Cargo.toml")).unwrap();
    assert!(manifest.contains("[patch.crates-io]") && manifest.contains("vendor/rpi-led-panel"));
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).unwrap();
    // a path source has no `source = "registry..."` line, so the lock entry for the crate must not carry one
    let entry = lock.split("[[package]]").find(|p| p.contains("name = \"rpi-led-panel\"")).expect("in Cargo.lock");
    assert!(!entry.contains("registry+"), "Cargo.lock still points at the crates.io copy:\n{entry}");
}

#[test]
fn the_timer_is_not_read_with_an_unaligned_64_bit_load() {
    // Upstream read the system timer (offset 0x4) as one 8-byte volatile value: a 64-bit load from a 4-byte
    // aligned address, which raises SIGBUS on aarch64 (see vendor/rpi-led-panel/PATCHES.md).
    let src = std::fs::read_to_string(root().join("vendor/rpi-led-panel/src/registers.rs")).unwrap();
    assert!(!src.contains("struct TimeRegister "), "the 8-byte timer read is back");
    assert!(src.contains("ST_CHI"), "the timer's high word should be read separately");
    assert!(root().join("vendor/rpi-led-panel/PATCHES.md").is_file());
}
