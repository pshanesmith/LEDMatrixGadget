//! Packaging a release and installing from it (up to the point that needs a Raspberry Pi).

mod common;

use std::path::Path;
use std::process::{Command, Output};

use common::root;

const PROGRAMS: [&str; 5] = ["matrix-display", "matrix-hello", "matrix-geocode", "matrix-set-config", "matrix-ha-discover"];

fn have(tool: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {tool}")]).output().is_ok_and(|o| o.status.success())
}

/// Fake "cross-built" programs, so the packaging script can run anywhere.
fn fake_bins(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for p in PROGRAMS {
        std::fs::write(dir.join(p), format!("#!/bin/sh\necho {p} fake\n")).unwrap();
    }
}

fn package(bin_dir: &Path, out: &Path) -> Output {
    Command::new("bash")
        .arg(root().join("tools/package-release.sh"))
        .args(["--bin-dir"]).arg(bin_dir).args(["--out"]).arg(out).args(["--version", "9.9.9"])
        .output()
        .unwrap()
}

fn install(base: &Path, extra: &[&str]) -> Output {
    Command::new("bash")
        .arg(root().join("install.sh"))
        .args(extra)
        .env("MATRIX_RELEASE_BASE", format!("file://{}", base.display()))
        .output()
        .unwrap()
}

fn tools_present() -> bool {
    have("tar") && have("curl") && (have("sha256sum") || have("shasum"))
}

#[test]
fn the_release_holds_everything_the_installer_needs() {
    if !tools_present() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (bins, out) = (dir.path().join("bins"), dir.path().join("out"));
    fake_bins(&bins);
    let r = package(&bins, &out);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    for f in ["matrix-display-aarch64.tar.gz", "matrix-display-aarch64.tar.gz.sha256", "install.sh"] {
        assert!(out.join(f).is_file(), "{f}");
    }
    let listing = Command::new("tar").arg("-tzf").arg(out.join("matrix-display-aarch64.tar.gz")).output().unwrap();
    let listing = String::from_utf8(listing.stdout).unwrap();
    let mut want: Vec<String> = PROGRAMS.iter().map(|p| format!("bin/{p}")).collect();
    want.extend(
        ["VERSION", "setup/03-install-service.sh", "setup/02-matrix-setup.sh", "setup/lib/common.sh",
         "systemd/matrix-display.service", "systemd/matrix-health.timer", "config/config.example.toml",
         "tools/health-check.sh", "docs/operations.md"].map(String::from),
    );
    for f in want {
        assert!(listing.lines().any(|l| l.ends_with(&format!("matrix-display-aarch64/{f}"))), "{f} missing:\n{listing}");
    }
    assert!(!listing.contains("/._"), "macOS metadata files in the archive");
}

#[test]
fn installer_downloads_verifies_and_unpacks() {
    if !tools_present() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (bins, out) = (dir.path().join("bins"), dir.path().join("out"));
    fake_bins(&bins);
    assert!(package(&bins, &out).status.success());
    let r = install(&out, &["--dry-run"]);
    let text = String::from_utf8_lossy(&r.stdout).to_string() + &String::from_utf8_lossy(&r.stderr);
    assert!(r.status.success(), "{text}");
    assert!(text.contains("release 9.9.9") && text.contains("--release") && text.contains("bin/matrix-display"), "{text}");
}

#[test]
fn a_corrupt_download_is_refused() {
    if !tools_present() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let (bins, out) = (dir.path().join("bins"), dir.path().join("out"));
    fake_bins(&bins);
    assert!(package(&bins, &out).status.success());
    let tarball = out.join("matrix-display-aarch64.tar.gz");
    let mut bytes = std::fs::read(&tarball).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&tarball, bytes).unwrap();
    let r = install(&out, &["--dry-run"]);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("checksum mismatch"));
}

#[test]
fn a_missing_release_is_explained() {
    if !have("curl") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let r = install(dir.path(), &["--dry-run"]);
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("download failed"));
}

#[test]
fn packaging_without_binaries_says_what_to_do() {
    let dir = tempfile::tempdir().unwrap();
    let r = package(dir.path(), dir.path());
    assert!(!r.status.success());
    assert!(String::from_utf8_lossy(&r.stderr).contains("cross-build.sh"));
}

#[test]
fn installer_refuses_unknown_options_and_prints_help() {
    let r = Command::new("bash").arg(root().join("install.sh")).arg("--nope").output().unwrap();
    assert!(!r.status.success());
    let help = Command::new("bash").arg(root().join("install.sh")).arg("--help").output().unwrap();
    assert!(help.status.success() && String::from_utf8_lossy(&help.stdout).contains("--from-source"));
}

#[test]
fn release_scripts_and_workflows_are_well_formed() {
    for f in ["install.sh", "tools/package-release.sh", "tools/cross-build.sh", "setup/03-install-service.sh"] {
        let out = Command::new("bash").arg("-n").arg(root().join(f)).output().unwrap();
        assert!(out.status.success(), "{f}: {}", String::from_utf8_lossy(&out.stderr));
    }
    let release = std::fs::read_to_string(root().join(".github/workflows/release.yml")).unwrap();
    assert!(release.contains("tools/cross-build.sh") && release.contains("tools/package-release.sh"));
    assert!(release.contains("gh release create") && release.contains("install.sh"));
}
