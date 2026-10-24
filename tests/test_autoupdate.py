import re
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = (ROOT / "setup/01-pi-setup.sh").read_text()


def snippet(target):
    m = re.search(r"write_file " + re.escape(target) + r" '(.*?)'\n", SCRIPT, re.S)
    assert m, target
    return m.group(1) + "\n"


@pytest.mark.skipif(not shutil.which("apt-config"), reason="needs apt-config")
def test_unattended_upgrade_config_parses_with_the_intended_policy(tmp_path):
    f = tmp_path / "52matrix"
    f.write_text(snippet("/etc/apt/apt.conf.d/52matrix-unattended-upgrades"))
    dump = subprocess.run(["apt-config", "-c", str(f), "dump"], capture_output=True, text=True, check=True).stdout
    assert 'Unattended-Upgrade::Origins-Pattern:: "origin=Debian,codename=${distro_codename}-security,label=Debian-Security";' in dump
    assert "origin=Raspberry Pi Foundation" in dump
    assert 'Unattended-Upgrade::Automatic-Reboot "true";' in dump
    assert 'Unattended-Upgrade::Automatic-Reboot-WithUsers "false";' in dump
    assert 'Unattended-Upgrade::Automatic-Reboot-Time "04:00";' in dump


@pytest.mark.skipif(not shutil.which("apt-config"), reason="needs apt-config")
def test_periodic_settings_enable_the_daily_run(tmp_path):
    f = tmp_path / "20auto"
    f.write_text(snippet("/etc/apt/apt.conf.d/20auto-upgrades"))
    dump = subprocess.run(["apt-config", "-c", str(f), "dump"], capture_output=True, text=True, check=True).stdout
    assert 'APT::Periodic::Update-Package-Lists "1";' in dump and 'APT::Periodic::Unattended-Upgrade "1";' in dump


@pytest.mark.skipif(not shutil.which("systemd-analyze"), reason="needs systemd-analyze")
def test_timer_calendar_is_valid_and_precedes_the_reboot_window():
    timer = snippet("/etc/systemd/system/apt-daily-upgrade.timer.d/matrix.conf")
    cal = re.search(r"^OnCalendar=(\*.+)$", timer, re.M).group(1)
    out = subprocess.run(["systemd-analyze", "calendar", cal], capture_output=True, text=True)
    assert out.returncode == 0 and "03:00:00" in out.stdout
    assert 'Automatic-Reboot-Time "04:00"' in SCRIPT          # reboot window follows the 03:00 upgrade


def test_pillow_is_not_held_or_blacklisted():
    # Frames use Pillow's public API (safe_image_copy), so Pillow may upgrade like any other package.
    conf = snippet("/etc/apt/apt.conf.d/52matrix-unattended-upgrades")
    assert "python3-pil" not in conf and "Package-Blacklist" not in conf
    assert "apt-mark hold" not in SCRIPT
