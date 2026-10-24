"""config_txt_update must be idempotent and only touch what it should (sandboxed copy)."""
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
COMMON = ROOT / "setup/lib/common.sh"
SAMPLE = """# header
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
"""
BLOCK = "dtoverlay=disable-bt\ndtparam=audio=off"
REGEX = "dtparam=audio=|camera_auto_detect=|dtoverlay=vc4-kms-v3d|max_framebuffers=|dtoverlay=disable-bt"


def run_update(path, dry=0, sample_has_all=True):
    script = (f'DRY_RUN={dry} BACKUP_DIR="{path.parent}/bk"; source "{COMMON}"; '
              f'config_txt_update "{path}" power "{BLOCK}" "{REGEX}"')
    return subprocess.run(["bash", "-c", script], capture_output=True, text=True, check=True)


def test_idempotent_and_scoped(tmp_path):
    f = tmp_path / "config.txt"
    f.write_text(SAMPLE)
    run_update(f)
    first = f.read_text()
    assert "#matrix-setup# dtparam=audio=on" in first
    assert "#matrix-setup# dtoverlay=vc4-kms-v3d" in first
    assert "arm_boost=1" in first and "dtoverlay=nospi10" in first    # untouched
    assert first.rstrip().endswith("# END matrix-setup:power")
    assert first.count("[all]") == 1 and first.count("# BEGIN matrix-setup:power") == 1
    run_update(f)
    assert f.read_text() == first                                        # second run: no change
    assert (tmp_path / "bk" / str(f).lstrip("/")).read_text() == SAMPLE  # original backed up


def test_dry_run_changes_nothing(tmp_path):
    f = tmp_path / "config.txt"
    f.write_text(SAMPLE)
    out = run_update(f, dry=1).stdout
    assert f.read_text() == SAMPLE and "+dtparam=audio=off" in out


def test_adds_all_section_when_missing(tmp_path):
    f = tmp_path / "config.txt"
    f.write_text("dtparam=audio=on\n[pi5]\nfoo=1\n")
    run_update(f)
    assert f.read_text().count("[all]") == 1


def test_real_config_roundtrip(tmp_path):
    real = Path("/boot/firmware/config.txt")
    if not real.exists():
        return
    f = tmp_path / "config.txt"
    shutil.copy(real, f)
    run_update(f); once = f.read_text(); run_update(f)
    assert f.read_text() == once


CMDLINE = "console=serial0,115200 console=tty1 root=PARTUUID=abc rootwait cfg80211.ieee80211_regdom=US"
TOKENS = '"isolcpus=domain,managed_irq,3" "nohz_full=3" "rcu_nocbs=3" "irqaffinity=0,1,2"'


def run_cmdline(path, tokens=TOKENS, dry=0):
    script = (f'DRY_RUN={dry} BACKUP_DIR="{path.parent}/bk"; source "{COMMON}"; '
              f'cmdline_ensure "{path}" {tokens}')
    return subprocess.run(["bash", "-c", script], capture_output=True, text=True, check=True)


def test_cmdline_adds_tokens_once_on_one_line(tmp_path):
    f = tmp_path / "cmdline.txt"
    f.write_text(CMDLINE)                      # real file has no trailing newline
    run_cmdline(f)
    first = f.read_text()
    assert first.count("\n") == 1 and first.endswith("\n")
    assert first.startswith(CMDLINE + " isolcpus=domain,managed_irq,3 nohz_full=3")
    run_cmdline(f)
    assert f.read_text() == first              # idempotent


def test_cmdline_respects_existing_value(tmp_path):
    f = tmp_path / "cmdline.txt"
    f.write_text(CMDLINE + " isolcpus=2")
    out = run_cmdline(f).stdout
    assert "isolcpus=2" in f.read_text() and "isolcpus=domain" not in f.read_text()
    assert "already sets isolcpus" in out
