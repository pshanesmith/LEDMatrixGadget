//! The Pi setup scripts, exercised on sandboxed copies. They target Raspberry Pi OS (GNU userland),
//! so the shell tests only run on Linux.

mod common;

use std::process::Command;

use common::root;

fn have(tool: &str) -> bool {
    Command::new("sh").args(["-c", &format!("command -v {tool}")]).output().is_ok_and(|o| o.status.success())
}

fn script(name: &str) -> String {
    std::fs::read_to_string(root().join("setup").join(name)).unwrap()
}

/// The body of `write_file TARGET '...'` in a setup script.
fn snippet(script: &str, target: &str) -> String {
    let start = script.find(&format!("write_file {target} '")).unwrap_or_else(|| panic!("{target}")) + "write_file ".len() + target.len() + 2;
    let end = start + script[start..].find("'\n").unwrap();
    format!("{}\n", &script[start..end])
}

#[cfg(target_os = "linux")]
mod boot {
    use std::path::Path;

    use super::*;

    fn bash(script: &str) -> std::process::Output {
        let out = Command::new("bash").args(["-c", script]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        out
    }

    const SAMPLE: &str = "# header
    dtparam=audio=on
    camera_auto_detect=1
    dtoverlay=vc4-kms-v3d
    max_framebuffers=2
    arm_boost=1
    [cm4]
    otg_mode=1
    [pi5]
    dtoverlay=nospi10
    [all]
    ";
    const BLOCK: &str = "dtoverlay=disable-bt\ndtparam=audio=off";
    const REGEX: &str = "dtparam=audio=|camera_auto_detect=|dtoverlay=vc4-kms-v3d|max_framebuffers=|dtoverlay=disable-bt";

    fn run_update(path: &Path, dry: u8) -> String {
        let common = root().join("setup/lib/common.sh");
        let script = format!(
            "DRY_RUN={dry} BACKUP_DIR=\"{}/bk\"; source \"{}\"; config_txt_update \"{}\" power \"{BLOCK}\" \"{REGEX}\"",
            path.parent().unwrap().display(), common.display(), path.display()
        );
        String::from_utf8(bash(&script).stdout).unwrap()
    }

    #[test]
    fn config_txt_update_is_idempotent_and_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("config.txt");
        std::fs::write(&f, SAMPLE).unwrap();
        run_update(&f, 0);
        let first = std::fs::read_to_string(&f).unwrap();
        assert!(first.contains("#matrix-setup# dtparam=audio=on"));
        assert!(first.contains("#matrix-setup# dtoverlay=vc4-kms-v3d"));
        assert!(first.contains("arm_boost=1") && first.contains("dtoverlay=nospi10")); // untouched
        assert!(first.trim_end().ends_with("# END matrix-setup:power"));
        assert_eq!(first.matches("[all]").count(), 1);
        assert_eq!(first.matches("# BEGIN matrix-setup:power").count(), 1);
        run_update(&f, 0);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), first); // second run: no change
        let backup = dir.path().join("bk").join(f.to_str().unwrap().trim_start_matches('/'));
        assert_eq!(std::fs::read_to_string(backup).unwrap(), SAMPLE); // original backed up
    }

    #[test]
    fn dry_run_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("config.txt");
        std::fs::write(&f, SAMPLE).unwrap();
        let out = run_update(&f, 1);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), SAMPLE);
        assert!(out.contains("+dtparam=audio=off"));
    }

    #[test]
    fn adds_all_section_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("config.txt");
        std::fs::write(&f, "dtparam=audio=on\n[pi5]\nfoo=1\n").unwrap();
        run_update(&f, 0);
        assert_eq!(std::fs::read_to_string(&f).unwrap().matches("[all]").count(), 1);
    }

    const CMDLINE: &str = "console=serial0,115200 console=tty1 root=PARTUUID=abc rootwait cfg80211.ieee80211_regdom=US";
    const TOKENS: &str = "\"isolcpus=domain,managed_irq,3\" \"nohz_full=3\" \"rcu_nocbs=3\" \"irqaffinity=0,1,2\"";

    fn run_cmdline(path: &Path) -> String {
        let common = root().join("setup/lib/common.sh");
        let script = format!(
            "DRY_RUN=0 BACKUP_DIR=\"{}/bk\"; source \"{}\"; cmdline_ensure \"{}\" {TOKENS}",
            path.parent().unwrap().display(), common.display(), path.display()
        );
        String::from_utf8(bash(&script).stdout).unwrap()
    }

    #[test]
    fn cmdline_adds_tokens_once_on_one_line() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("cmdline.txt");
        std::fs::write(&f, CMDLINE).unwrap(); // the real file has no trailing newline
        run_cmdline(&f);
        let first = std::fs::read_to_string(&f).unwrap();
        assert!(first.matches('\n').count() == 1 && first.ends_with('\n'));
        assert!(first.starts_with(&format!("{CMDLINE} isolcpus=domain,managed_irq,3 nohz_full=3")));
        run_cmdline(&f);
        assert_eq!(std::fs::read_to_string(&f).unwrap(), first); // idempotent
    }

    #[test]
    fn cmdline_respects_existing_value() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("cmdline.txt");
        std::fs::write(&f, format!("{CMDLINE} isolcpus=2")).unwrap();
        let out = run_cmdline(&f);
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("isolcpus=2") && !text.contains("isolcpus=domain"));
        assert!(out.contains("already sets isolcpus"));
    }
}

// --- automatic updates ------------------------------------------------------------------------

#[test]
fn unattended_upgrade_config_parses_with_the_intended_policy() {
    if !have("apt-config") {
        return;
    }
    let script = script("01-pi-setup.sh");
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("52matrix");
    std::fs::write(&f, snippet(&script, "/etc/apt/apt.conf.d/52matrix-unattended-upgrades")).unwrap();
    let dump = String::from_utf8(Command::new("apt-config").args(["-c"]).arg(&f).arg("dump").output().unwrap().stdout).unwrap();
    assert!(dump.contains("Unattended-Upgrade::Origins-Pattern:: \"origin=Debian,codename=${distro_codename}-security,label=Debian-Security\";"));
    assert!(dump.contains("origin=Raspberry Pi Foundation"));
    assert!(dump.contains("Unattended-Upgrade::Automatic-Reboot \"true\";"));
    assert!(dump.contains("Unattended-Upgrade::Automatic-Reboot-WithUsers \"false\";"));
    assert!(dump.contains("Unattended-Upgrade::Automatic-Reboot-Time \"04:00\";"));
}

#[test]
fn periodic_settings_enable_the_daily_run() {
    if !have("apt-config") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("20auto");
    std::fs::write(&f, snippet(&script("01-pi-setup.sh"), "/etc/apt/apt.conf.d/20auto-upgrades")).unwrap();
    let dump = String::from_utf8(Command::new("apt-config").args(["-c"]).arg(&f).arg("dump").output().unwrap().stdout).unwrap();
    assert!(dump.contains("APT::Periodic::Update-Package-Lists \"1\";") && dump.contains("APT::Periodic::Unattended-Upgrade \"1\";"));
}

#[test]
fn timer_calendar_is_valid_and_precedes_the_reboot_window() {
    if !have("systemd-analyze") {
        return;
    }
    let script = script("01-pi-setup.sh");
    let timer = snippet(&script, "/etc/systemd/system/apt-daily-upgrade.timer.d/matrix.conf");
    let cal = timer.lines().find_map(|l| l.strip_prefix("OnCalendar=").filter(|c| c.starts_with('*'))).unwrap();
    let out = Command::new("systemd-analyze").args(["calendar", cal]).output().unwrap();
    assert!(out.status.success() && String::from_utf8_lossy(&out.stdout).contains("03:00:00"));
    assert!(script.contains("Automatic-Reboot-Time \"04:00\"")); // reboot window follows the 03:00 upgrade
}

#[test]
fn nothing_is_held_back_from_upgrades() {
    let script = script("01-pi-setup.sh");
    let conf = snippet(&script, "/etc/apt/apt.conf.d/52matrix-unattended-upgrades");
    assert!(!conf.contains("Package-Blacklist"));
    assert!(!script.contains("apt-mark hold"));
}

#[test]
fn scripts_have_valid_bash_syntax() {
    for name in ["01-pi-setup.sh", "02-matrix-setup.sh", "03-install-service.sh", "lib/common.sh"] {
        let out = Command::new("bash").arg("-n").arg(root().join("setup").join(name)).output().unwrap();
        assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
    }
    for name in ["test-matrix.sh", "health-check.sh", "soak-report.sh"] {
        let out = Command::new("bash").arg("-n").arg(root().join("tools").join(name)).output().unwrap();
        assert!(out.status.success(), "{name}: {}", String::from_utf8_lossy(&out.stderr));
    }
}
