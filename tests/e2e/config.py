"""Canonical paths, immutable baseline hashes, and requirement-derived expectations."""
import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
OVERLAY_DIR = REPO_ROOT / "blazeos_custom_apps"
PACKAGING_REPO_DIR = REPO_ROOT / "packaging" / "out" / "repo"
SOLARUI_DIR = REPO_ROOT / "solarui"
REF_WORK_DIR = REPO_ROOT / "work_f45"
REF_ISO_PATH = REPO_ROOT / "Blaze-SolarEvolution-5-x86_64(2026-11-11).iso"
def get_target_iso_path() -> Path:
    return Path(
        os.environ.get(
            "BLAZE_E2E_ISO",
            str(REPO_ROOT / "Blaze-SolarEvolution-5-x86_64.iso"),
        )
    )


def get_target_iso_sha256_path() -> Path:
    return Path(f"{get_target_iso_path()}.sha256")


def get_artifacts_dir() -> Path:
    return Path(os.environ.get("BLAZE_E2E_ARTIFACTS", "/tmp/blaze_e2e_artifacts"))


def get_firmware_mode() -> str:
    return os.environ.get("BLAZE_QEMU_FIRMWARE", "both").lower()


TARGET_ISO_PATH = get_target_iso_path()
TARGET_ISO_SHA256_PATH = get_target_iso_sha256_path()
DEFAULT_ARTIFACTS_DIR = get_artifacts_dir()

OVMF_CODE_PATH = Path("/usr/share/edk2/x64/OVMF_CODE.4m.fd")
OVMF_VARS_PATH = Path("/usr/share/edk2/x64/OVMF_VARS.4m.fd")

# Read-only reference baseline SHA-256 hashes (surveyed 2026-11-19).
# These artifacts must NEVER be modified or overwritten.
IMMUTABLE_BASELINE_SHA256 = {
    REF_ISO_PATH: "ccbfd34f59cfb6ecb10a060a119b60f437f0e48e0a7557c98aa9ff49dabee7b4",
    REF_WORK_DIR / "iso_mods/boot/grub2/grub.cfg": "d2072f631369905714f4d99e6c7a4fa5437e8f7bc3066da09bcf61979e56b8b0",
    REF_WORK_DIR / "initrd_work/patched_initrd": "1610f62a29a77dc3d5ddfde90da37468f46816216458cb79d6b8ba032ecf74ad",
    REF_WORK_DIR / "LiveOS/squashfs.img": "b4f0158822d71923f568911dd5c92f01a276f3c4d0409d9b68c72279f0508b88",
}

# Reference MD5 of customized spinner watermark and logo.
REF_SPINNER_WATERMARK_MD5 = "a71cd9498af58db2c9f4a4e1e4f4053a"

# R1: 14 tools moved to blazeos_custom_apps/usr/bin/
CUSTOM_14_TOOLS = (
    "blaze-bootloader",
    "blaze-optimize",
    "blaze-snapshot",
    "blazeos-apply-install-choices",
    "blazeos-control",
    "blazeos-desktop-provision",
    "blazeos-limine-install",
    "blazeos-nvidia-setup",
    "blazeos-postinstall",
    "blazeos-remove-solarui",
    "blazeos-session-diagnose",
    "blazeos-waydroid-setup",
    "blazeos-welcome",
    "solar-torture",
)


def discover_overlay_usr_bin_tools() -> tuple[str, ...]:
    """Dynamically discover all executable files and symlinks in OVERLAY_DIR/usr/bin."""
    usr_bin = OVERLAY_DIR / "usr/bin"
    if not usr_bin.is_dir():
        return ()
    tools = []
    for entry in sorted(usr_bin.iterdir(), key=lambda p: p.name):
        if entry.name == "__pycache__":
            continue
        if entry.is_symlink() or (entry.is_file() and os.access(entry, os.X_OK)):
            tools.append(entry.name)
    return tuple(tools)


def get_niri_validate_binary() -> str | None:
    """Resolve BlazeOS patched niri binary before host niri (which lacks server-titlebar)."""
    import shutil
    for cand in (
        OVERLAY_DIR / "usr/bin/niri",
        REPO_ROOT / "niri-src/target/release/niri",
    ):
        if cand.is_file() and os.access(cand, os.X_OK):
            return str(cand)
    return shutil.which("niri")


# R1: 8 signed RPM packages in packaging/out/repo/
EXPECTED_8_RPMS = (
    "blaze-continuum-0.1.0-1.blaze.x86_64.rpm",
    "blaze-house-2.1.0-1.blaze.x86_64.rpm",
    "blaze-oobe-5.05-1.blaze.noarch.rpm",
    "blaze-recovery-5.05-2.blaze.x86_64.rpm",
    "blaze-release-5.05-3.blaze.noarch.rpm",
    "blaze-sentinel-0.3.1-1.blaze.x86_64.rpm",
    "blaze-tools-5.05-1.blaze.noarch.rpm",
    "solarui-5.05-1.blaze.x86_64.rpm",
)

# R2: GNOME / Workstation bloat packages that must be purged from live rootfs
FORBIDDEN_LIVE_RPMS = (
    "gdm",
    "gnome-shell",
    "gnome-shell-extension-apps-menu",
    "gnome-shell-extension-background-logo",
    "gnome-shell-extension-common",
    "gnome-shell-extension-launch-new-instance",
    "gnome-shell-extension-places-menu",
    "gnome-shell-extension-window-list",
    "gnome-tour",
    "gnome-initial-setup",
    "gnome-control-center",
    "gnome-classic-session",
    "gnome-session-wayland-session",
    "gnome-browser-connector",
    "gnome-system-monitor",
    "edk2-ovmf",
    "spice-server",
)

# R2: Unwanted systemd sockets/units that must NOT be enabled in .wants/
FORBIDDEN_SYSTEMD_WANTS_GLOBS = (
    "multi-user.target.wants/audit-rules.service",
    "multi-user.target.wants/virtqemud.service",
    "multi-user.target.wants/auditd.service",
    "multi-user.target.wants/sssd.service",
    "multi-user.target.wants/stratisd.service",
    "multi-user.target.wants/mcelog.service",
    "multi-user.target.wants/rsyslog.service",
    "multi-user.target.wants/ModemManager.service",
    "multi-user.target.wants/avahi-daemon.service",
    "multi-user.target.wants/systemd-homed.service",
    "multi-user.target.wants/mdmonitor.service",
    "network-online.target.wants/NetworkManager-wait-online.service",
    "timers.target.wants/dnf-makecache.timer",
    "sysinit.target.wants/iscsi-onboot.service",
    "sysinit.target.wants/iscsi-starter.service",
    "sockets.target.wants/virt*.socket",
    "sockets.target.wants/iscsid.socket",
    "sockets.target.wants/iscsiuio.socket",
    "sockets.target.wants/sssd-kcm.socket",
    "sockets.target.wants/systemd-journalctl-metrics.socket",
    "sockets.target.wants/systemd-journald-audit.socket",
    "sockets.target.wants/systemd-report*.socket",
)

# R2 / M2: Required enabled systemd units
REQUIRED_ENABLED_UNITS = (
    "etc/systemd/system/display-manager.service",
    "etc/systemd/system/graphical.target.wants/sddm.service",
    "etc/systemd/system/graphical.target.wants/blaze-boot-success.service",
    "etc/systemd/system/multi-user.target.wants/display-manager.service",
    "etc/systemd/system/multi-user.target.wants/blaze-sentinel.service",
    "etc/systemd/system/multi-user.target.wants/blazeos-plymouth-failsafe.service",
    "etc/systemd/system/graphical.target.wants/blazeos-nvidia-firstboot.service",
    "etc/systemd/system/multi-user.target.wants/blaze-optimization.service",
    "etc/systemd/system/multi-user.target.wants/blazeos-identity-guard.service",
    "etc/systemd/system/multi-user.target.wants/livesys.service",
)

# Critical ELFs inside squashfs whose NEEDED libraries must all resolve
CRITICAL_SQUASHFS_ELFS = (
    "usr/bin/noctalia",
    "usr/bin/solar-shell",
    "usr/bin/solar-session",
    "usr/bin/solar-core",
    "usr/bin/blaze-sentinel",
    "usr/bin/blaze-recovery",
    "usr/bin/blaze-house",
    "opt/FireHub/firehub",
    "usr/bin/quickshell.bin",
    "usr/bin/quickshell-fedora",
    "usr/lib64/quickshell/qml/Caelestia/lib/libcaelestia-core.so",
    "usr/lib64/quickshell/qml/Caelestia/lib/libcaelestia-services.so",
    "usr/lib64/plymouth/script.so",
)
