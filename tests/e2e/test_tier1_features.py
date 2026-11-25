"""Tier 1: Requirement-driven feature coverage against ISO, initrd, squashfs, RPMs, and ELFs."""
import ast
import gzip
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile
import unittest

from tests.e2e.config import (
    CRITICAL_SQUASHFS_ELFS,
    CUSTOM_14_TOOLS,
    EXPECTED_8_RPMS,
    FORBIDDEN_LIVE_RPMS,
    FORBIDDEN_SYSTEMD_WANTS_GLOBS,
    IMMUTABLE_BASELINE_SHA256,
    OVERLAY_DIR,
    PACKAGING_REPO_DIR,
    REF_SPINNER_WATERMARK_MD5,
    REF_WORK_DIR,
    REPO_ROOT,
    REQUIRED_ENABLED_UNITS,
    SOLARUI_DIR,
    discover_overlay_usr_bin_tools,
    get_target_iso_path,
    get_target_iso_sha256_path,
)
from tests.e2e.harness.iso_inspect import (
    extract_initrd_zstd_cpio_list,
    extract_initrd_zstd_files,
    md5_file,
    readelf_needed,
    rpm_query_root,
    rpm_verify_signatures,
    run_checkisomd5,
    sha256_file,
    unsquashfs_cat,
    unsquashfs_extract_paths,
    unsquashfs_list,
    verify_iso_sha256_file,
    xorriso_extract,
    xorriso_list,
    xorriso_pvd_info,
)

RUN_E2E = os.environ.get("BLAZE_RUN_E2E") == "1"

# Optional Qt designer / external DB plugins that link host-optional libs not bundled in LiveOS.
OPTIONAL_QUICKSHELL_PLUGINS = frozenset({
    "libPySidePlugin.so",
    "libqsqlibase.so",
    "libqsqlodbc.so",
    "libqsqlpsql.so",
})

ELFCLASS32 = 1
ELFCLASS64 = 2
EM_386 = 0x03
EM_X86_64 = 0x3E
X86_64_ELF_ARCH = (ELFCLASS64, EM_X86_64)
X86_32_ELF_ARCH = (ELFCLASS32, EM_386)


def _resolve_in_root(
    entry: Path,
    root: Path,
    *,
    expect_dir: bool = False,
    max_hops: int = 40,
) -> Path | None:
    """Resolve `entry` strictly inside `root` without dereferencing against the host."""
    root_real = root.resolve()
    try:
        rel = entry.relative_to(root)
    except ValueError:
        try:
            rel = entry.relative_to(root_real)
        except ValueError:
            return None

    pending = [part for part in rel.parts if part not in ("", ".")]
    resolved_parts: list[str] = []
    hops = 0

    while pending:
        part = pending.pop(0)
        if part == "..":
            if not resolved_parts:
                return None  # Relative symlink escaped above root
            resolved_parts.pop()
            continue

        resolved_parts.append(part)
        cur = root_real.joinpath(*resolved_parts)
        try:
            st = os.lstat(cur)
        except OSError:
            return None

        if stat.S_ISLNK(st.st_mode):
            hops += 1
            if hops > max_hops:
                return None
            try:
                target = os.readlink(cur)
            except OSError:
                return None
            tgt_parts = [p for p in target.split("/") if p not in ("", ".")]
            if target.startswith("/"):
                resolved_parts = []
            else:
                resolved_parts.pop()
            pending = tgt_parts + pending

    final = root_real.joinpath(*resolved_parts)
    try:
        st = os.lstat(final)
    except OSError:
        return None
    if expect_dir:
        return final if stat.S_ISDIR(st.st_mode) else None
    return final if stat.S_ISREG(st.st_mode) else None


def _read_elf_arch(path: Path) -> tuple[int, int] | None:
    """Return `(ei_class, e_machine)` from an ELF header, or None if not a valid ELF."""
    try:
        with open(path, "rb") as f:
            hdr = f.read(20)
    except OSError:
        return None
    if len(hdr) < 20 or hdr[:4] != b"\x7fELF":
        return None
    ei_class = hdr[4]
    ei_data = hdr[5]
    if ei_data == 1:
        e_machine = int.from_bytes(hdr[18:20], "little")
    elif ei_data == 2:
        e_machine = int.from_bytes(hdr[18:20], "big")
    else:
        return None
    return (ei_class, e_machine)


def _collect_direct_so_names(
    dirs: list[Path],
    *,
    ext_root: Path,
    expected_arch: tuple[int, int] = X86_64_ELF_ARCH,
) -> set[str]:
    """Collect valid ELF `.so` names directly inside `dirs`, confined strictly to `ext_root`."""
    libs: set[str] = set()
    for base in dirs:
        resolved_base = _resolve_in_root(base, ext_root, expect_dir=True)
        if resolved_base is None:
            continue
        for entry in resolved_base.iterdir():
            if ".so" not in entry.name:
                continue
            resolved = _resolve_in_root(entry, ext_root, expect_dir=False)
            if resolved is None:
                continue
            if _read_elf_arch(resolved) == expected_arch:
                libs.add(entry.name)
    return libs


@unittest.skipUnless(
    RUN_E2E,
    "Set BLAZE_RUN_E2E=1 or run `python3 -m tests.e2e.run_e2e --tier 1`",
)
class Tier1FeatureCoverageTests(unittest.TestCase):
    """Covers Features #1-#15 in PROJECT.md against built ISO and repo artifacts."""

    tmpdir_obj: tempfile.TemporaryDirectory | None = None
    cache_dir: Path
    iso_path: Path
    iso_sha256_path: Path
    initrd_path: Path
    squashfs_path: Path
    squashfs_tree: list[str]
    squashfs_paths: set[str]
    squashfs_modes: dict[str, str]

    @classmethod
    def setUpClass(cls) -> None:
        cls.iso_path = get_target_iso_path()
        cls.iso_sha256_path = get_target_iso_sha256_path()
        if not cls.iso_path.is_file():
            raise AssertionError(f"Target ISO not found: {cls.iso_path}")
        cls.tmpdir_obj = tempfile.TemporaryDirectory(prefix="blaze_t1_iso_")
        cls.addClassCleanup(cls.tmpdir_obj.cleanup)
        cls.cache_dir = Path(cls.tmpdir_obj.name)
        cls.initrd_path = xorriso_extract(
            cls.iso_path, "/boot/x86_64/loader/initrd", cls.cache_dir / "initrd"
        )
        cls.squashfs_path = xorriso_extract(
            cls.iso_path, "/LiveOS/squashfs.img", cls.cache_dir / "squashfs.img"
        )
        cls.squashfs_tree = unsquashfs_list(cls.squashfs_path)
        cls.squashfs_paths = set()
        cls.squashfs_modes = {}
        marker = "squashfs-root/"
        for line in cls.squashfs_tree:
            idx = line.find(marker)
            if idx != -1:
                mode = line.split(None, 1)[0]
                rel = line[idx + len(marker) :]
                if mode.startswith("l") and " -> " in rel:
                    rel = rel.split(" -> ", 1)[0]
                cls.squashfs_paths.add(rel)
                cls.squashfs_modes[rel] = mode

    @classmethod
    def tearDownClass(cls) -> None:
        if cls.tmpdir_obj is not None:
            cls.tmpdir_obj.cleanup()
            cls.tmpdir_obj = None

    def test_f15_iso_checksum_and_implantisomd5(self):
        self.assertTrue(self.iso_path.is_file(), f"Missing ISO: {self.iso_path}")
        self.assertTrue(self.iso_sha256_path.is_file(), f"Missing sha256: {self.iso_sha256_path}")
        ok, detail = verify_iso_sha256_file(self.iso_path, self.iso_sha256_path)
        self.assertTrue(ok, f"ISO SHA-256 mismatch: {detail}")
        res = run_checkisomd5(self.iso_path)
        self.assertEqual(res.returncode, 0, f"checkisomd5 failed: {res.stdout}\n{res.stderr}")

    def test_f15_readonly_reference_immutability(self):
        for path, expected_sha in IMMUTABLE_BASELINE_SHA256.items():
            self.assertTrue(path.is_file(), f"Missing reference baseline file: {path}")
            actual = sha256_file(path)
            self.assertEqual(
                actual,
                expected_sha,
                f"Read-only reference baseline modified! {path}: {actual} != {expected_sha}",
            )

    def test_f05_r1_fixes_preserved(self):
        license_path = REPO_ROOT / "LICENSE"
        self.assertTrue(license_path.is_file(), "Root LICENSE file missing")
        license_text = license_path.read_text(encoding="utf-8", errors="replace")
        self.assertIn("GNU GENERAL PUBLIC LICENSE", license_text)
        self.assertIn("Version 3", license_text)

        crates_dir = SOLARUI_DIR / "crates"
        forbidden_path_re = re.compile(r"(?<![\w./-])(/home/darkmorpheus|/home/SolarUI|/root/)")
        for rs_file in crates_dir.rglob("*.rs"):
            content = rs_file.read_text(encoding="utf-8", errors="replace")
            m = forbidden_path_re.search(content)
            self.assertIsNone(
                m,
                f"Hardcoded path {m.group(0) if m else ''} in {rs_file}",
            )

        sentinel_rs = (crates_dir / "blaze-recovery/src/sentinel.rs").read_text(encoding="utf-8")
        sentinel_core_rs = (crates_dir / "blaze-sentinel-core/src/engine.rs").read_text(encoding="utf-8")
        self.assertIn("SENTINEL_VERSION", sentinel_rs)
        self.assertIn('SENTINEL_VERSION: &str = "0.3.1"', sentinel_core_rs)
        self.assertIn("is_canary_intact", sentinel_rs)
        self.assertIn("pub fn is_canary_intact", sentinel_core_rs)

        overlay_tools = discover_overlay_usr_bin_tools()
        self.assertTrue(
            set(CUSTOM_14_TOOLS).issubset(set(overlay_tools)),
            f"Missing CUSTOM_14_TOOLS in overlay: {set(CUSTOM_14_TOOLS) - set(overlay_tools)}",
        )
        for tool in overlay_tools:
            rel = f"usr/bin/{tool}"
            self.assertIn(rel, self.squashfs_paths, f"Overlay tool missing in ISO squashfs: {rel}")
            mode = self.squashfs_modes.get(rel, "")
            self.assertTrue(
                mode.startswith("l") or "x" in mode[:10],
                f"Overlay tool not executable/symlink in squashfs: {rel} ({mode})",
            )

        conf_bytes = unsquashfs_cat(self.squashfs_path, "etc/anaconda/conf.d/99-blazeos.conf").decode(
            "utf-8"
        )
        self.assertRegex(conf_bytes, r"(?m)^default_scheme\s*=\s*BTRFS\s*$")
        self.assertIn("efi_dir = fedora", conf_bytes)

        repo_bytes = unsquashfs_cat(self.squashfs_path, "usr/share/dnf5/repos.d/blaze.repo").decode(
            "utf-8"
        )
        self.assertIn("gpgcheck=1", repo_bytes)
        self.assertIn("repo_gpgcheck=1", repo_bytes)
        self.assertIn("RPM-GPG-KEY-blaze", repo_bytes)

    def test_f10_signed_rpms_and_gpg_metadata(self):
        gpg_key = PACKAGING_REPO_DIR / "RPM-GPG-KEY-blaze"
        self.assertTrue(gpg_key.is_file(), f"Missing GPG key: {gpg_key}")
        rpm_files = [PACKAGING_REPO_DIR / name for name in EXPECTED_8_RPMS]
        for rpm_file in rpm_files:
            self.assertTrue(rpm_file.is_file(), f"Missing signed RPM: {rpm_file}")

        verdicts = rpm_verify_signatures(rpm_files, gpg_key)
        for rpm_path, verdict in verdicts.items():
            self.assertTrue(
                verdict.startswith("rc=0") and "digests signatures OK" in verdict,
                f"RPM signature check failed for {rpm_path}: {verdict}",
            )

        with tempfile.TemporaryDirectory(prefix="blaze_gpg_") as tmp:
            gpghome = Path(tmp) / "gnupg"
            gpghome.mkdir(mode=0o700)
            env = {**os.environ, "GNUPGHOME": str(gpghome), "LC_ALL": "C", "LANG": "C"}
            subprocess.run(
                ["gpg", "--import", str(gpg_key)],
                env=env,
                check=True,
                capture_output=True,
                timeout=30,
            )
            res = subprocess.run(
                [
                    "gpg",
                    "--verify",
                    str(PACKAGING_REPO_DIR / "repodata/repomd.xml.asc"),
                    str(PACKAGING_REPO_DIR / "repodata/repomd.xml"),
                ],
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertEqual(res.returncode, 0, f"repomd.xml GPG verify failed: {res.stderr}")

        squash_key = unsquashfs_cat(self.squashfs_path, "etc/pki/rpm-gpg/RPM-GPG-KEY-blaze")
        self.assertEqual(squash_key, gpg_key.read_bytes(), "Squashfs GPG key differs from packaging key")

    def test_f03_grub2_and_efi_stub_parity(self):
        pvd = xorriso_pvd_info(self.iso_path)
        self.assertIn("Blaze-SE-5", pvd)

        iso_files = set(xorriso_list(self.iso_path, "/"))
        self.assertIn("/boot/grub2/grub.cfg", iso_files)
        self.assertIn("/EFI/BOOT/BOOTX64.EFI", iso_files)

        grub_local = xorriso_extract(self.iso_path, "/boot/grub2/grub.cfg", self.cache_dir / "grub.cfg")
        grub_text = grub_local.read_text(encoding="utf-8")
        for expected in (
            "Start Blaze SolarEvolution 5",
            "Blaze Emergency Recovery Protocol (BERP)",
            "BlazeOS Recovery Environment (BERE Graphical)",
            "Test this media & start Blaze SolarEvolution 5",
            "Start Blaze SolarEvolution 5 (NVIDIA Mode)",
            "Troubleshooting",
            "CDLABEL=Blaze-SE-5",
            "rd.live.image",
        ):
            self.assertIn(expected, grub_text)

        m = re.search(r"search\s+--file\s+--set=root\s+(/boot/\S+)", grub_text)
        self.assertIsNotNone(m, "GRUB search marker line missing")
        assert m is not None
        self.assertIn(m.group(1), iso_files, f"GRUB search marker {m.group(1)} missing in ISO")

        efi_stub = unsquashfs_cat(self.squashfs_path, "usr/share/blazeos/grub-efi-stub.cfg").decode("utf-8")
        self.assertIn("grub2/grub.cfg", efi_stub)

    def test_f01_patched_initrd_plymouth_and_script_so(self):
        self.assertEqual(
            self.initrd_path.stat().st_size % 4,
            0,
            f"Initrd size {self.initrd_path.stat().st_size} is not 4-byte aligned for trailing cpio archives",
        )
        members = extract_initrd_zstd_cpio_list(self.initrd_path)
        for req in (
            "etc/plymouth/plymouthd.conf",
            "usr/lib64/plymouth/script.so",
            "usr/share/plymouth/themes/blazeos/blazeos.plymouth",
            "usr/share/plymouth/themes/blazeos/blazeos.script",
            "usr/share/plymouth/themes/blazeos/logo.png",
            "usr/share/plymouth/themes/spinner/watermark.png",
            "usr/lib/dracut/hooks",
            "usr/lib/modprobe.d/blazeos-usb-quirks.conf",
            "etc/udev/rules.d/99-blaze-ventoy-compat.rules",
            "usr/bin/dmsquash-live-root",
        ):
            self.assertIn(req, members, f"Initrd missing required artifact: {req}")

        ext_dir = self.cache_dir / "initrd_ext"
        extract_initrd_zstd_files(
            self.initrd_path,
            [
                "etc/plymouth/plymouthd.conf",
                "usr/share/plymouth/themes/spinner/watermark.png",
                "usr/lib/modprobe.d/blazeos-usb-quirks.conf",
                "etc/udev/rules.d/99-blaze-ventoy-compat.rules",
                "usr/bin/dmsquash-live-root",
            ],
            ext_dir,
        )
        ply_conf = (ext_dir / "etc/plymouth/plymouthd.conf").read_text(encoding="utf-8")
        self.assertRegex(ply_conf, r"(?m)^Theme=blazeos\s*$")
        self.assertEqual(
            md5_file(ext_dir / "usr/share/plymouth/themes/spinner/watermark.png"),
            REF_SPINNER_WATERMARK_MD5,
        )
        self.assertIn(
            "346d:5678:u",
            (ext_dir / "usr/lib/modprobe.d/blazeos-usb-quirks.conf").read_text(encoding="utf-8"),
        )
        self.assertIn(
            'SYMLINK+="ventoy mapper/ventoy"',
            (ext_dir / "etc/udev/rules.d/99-blaze-ventoy-compat.rules").read_text(encoding="utf-8"),
        )
        dmsquash = (ext_dir / "usr/bin/dmsquash-live-root").read_text(encoding="utf-8")
        self.assertIn("cancel_wait_for_dev /dev/ventoy", dmsquash)
        self.assertIn("blkid -s TYPE -o value", dmsquash)
        self.assertIn("usr/lib/modprobe.d/blazeos-usb-quirks.conf", self.squashfs_paths)

    def test_f02_f04_rootfs_plymouth_logos_fonts_icons_and_wallpapers(self):
        ply_conf = unsquashfs_cat(self.squashfs_path, "etc/plymouth/plymouthd.conf").decode("utf-8")
        self.assertRegex(ply_conf, r"(?m)^Theme=blazeos\s*$")
        for ply_asset in (
            "usr/lib64/plymouth/script.so",
            "usr/share/plymouth/themes/blazeos/blazeos.plymouth",
            "usr/share/plymouth/themes/blazeos/blazeos.script",
            "usr/share/plymouth/themes/blazeos/logo.png",
            "usr/share/plymouth/themes/blazeos/logo-glow.png",
        ):
            self.assertIn(ply_asset, self.squashfs_paths, f"Missing Plymouth asset in squashfs: {ply_asset}")

        apply_logos = "usr/libexec/blazeos-apply-logos"
        self.assertIn(apply_logos, self.squashfs_paths, "Missing usr/libexec/blazeos-apply-logos")
        self.assertIn("x", self.squashfs_modes.get(apply_logos, "")[:10])

        for icon in (
            "usr/share/icons/hicolor/48x48/apps/solarui.png",
            "usr/share/icons/hicolor/256x256/apps/solarui.png",
            "usr/share/icons/hicolor/scalable/apps/solarui.svg",
        ):
            self.assertIn(icon, self.squashfs_paths, f"Missing SolarUI hicolor icon in squashfs: {icon}")

        for font_dir, expected_tokens in (
            ("usr/share/fonts/google-sans-flex/", ("GoogleSansFlex",)),
            ("usr/share/fonts/TTF/", ("CaskaydiaCoveNerdFont", "Rubik")),
            ("usr/share/fonts/ttf-material-symbols-variable/", ("MaterialSymbols",)),
        ):
            ttf_paths = [
                p for p in self.squashfs_paths if p.startswith(font_dir) and p.endswith(".ttf")
            ]
            self.assertTrue(ttf_paths, f"Missing .ttf font files in squashfs under {font_dir}")
            for tok in expected_tokens:
                self.assertTrue(
                    any(tok in p for p in ttf_paths),
                    f"Missing {tok} .ttf font in squashfs under {font_dir}",
                )

        ref_walls = [p.name for p in (REF_WORK_DIR / "rootfs/usr/share/blazeos/wallpapers").iterdir()]
        for wname in ref_walls:
            self.assertIn(
                f"usr/share/blazeos/wallpapers/{wname}",
                self.squashfs_paths,
                f"Missing wallpaper parity file: {wname}",
            )

        ext_wm = self.cache_dir / "rootfs_wm"
        unsquashfs_extract_paths(
            self.squashfs_path, ext_wm, ["usr/share/plymouth/themes/spinner/watermark.png"]
        )
        self.assertEqual(
            md5_file(ext_wm / "usr/share/plymouth/themes/spinner/watermark.png"),
            REF_SPINNER_WATERMARK_MD5,
        )

    def test_f11_direct_solarui_livesys_and_sddm_config(self):
        livesys = unsquashfs_cat(self.squashfs_path, "etc/sysconfig/livesys").decode("utf-8")
        self.assertIn('livesys_session="solarui"', livesys)
        self.assertIn("usr/libexec/livesys/sessions.d/livesys-solarui", self.squashfs_paths)
        self.assertIn("usr/share/wayland-sessions/solarui.desktop", self.squashfs_paths)

        ext_sd = self.cache_dir / "sddm_links"
        unsquashfs_extract_paths(
            self.squashfs_path,
            ext_sd,
            [
                "etc/systemd/system/display-manager.service",
                "etc/systemd/system/gdm.service",
                "etc/systemd/system/graphical.target.wants/sddm.service",
            ],
        )
        dm_target = os.readlink(ext_sd / "etc/systemd/system/display-manager.service")
        self.assertTrue(dm_target.endswith("sddm.service"), f"display-manager points to {dm_target}")
        self.assertEqual(os.readlink(ext_sd / "etc/systemd/system/gdm.service"), "/dev/null")
        self.assertTrue(
            (ext_sd / "etc/systemd/system/graphical.target.wants/sddm.service").is_symlink()
        )

        sddm_pam = unsquashfs_cat(self.squashfs_path, "etc/pam.d/sddm-autologin").decode("utf-8")
        session_lines = [
            " ".join(line.split())
            for line in sddm_pam.splitlines()
            if line.strip()
            and not line.strip().startswith("#")
            and line.strip().lstrip("-").startswith("session")
        ]
        self.assertTrue(
            any("pam_selinux.so close" in line for line in session_lines),
            f"Missing `session ... pam_selinux.so close` in etc/pam.d/sddm-autologin:\n{sddm_pam}",
        )
        self.assertTrue(
            any("pam_selinux.so open" in line for line in session_lines),
            f"Missing `session ... pam_selinux.so open` in etc/pam.d/sddm-autologin:\n{sddm_pam}",
        )
        self.assertFalse(
            any("pam_succeed_if.so" in line for line in session_lines),
            f"PAM session bypass (`pam_succeed_if.so`) must not skip pam_selinux.so in etc/pam.d/sddm-autologin:\n{sddm_pam}",
        )
        self.assertNotIn("pam_oo7.so", sddm_pam)

        wayland_sess = unsquashfs_cat(self.squashfs_path, "etc/sddm/wayland-session").decode("utf-8")
        self.assertNotIn('exec "$@"\n', wayland_sess)
        self.assertNotIn('exec "$@"', wayland_sess)
        self.assertIn("exec $@", wayland_sess)
        self.assertIn("usr/libexec/blaze-oobe-session", self.squashfs_paths)
        oobe_desktop = unsquashfs_cat(
            self.squashfs_path, "usr/share/wayland-sessions/blaze-oobe.desktop"
        ).decode("utf-8")
        self.assertIn("Exec=/usr/libexec/blaze-oobe-session", oobe_desktop)

    def test_f12_gnome_extension_purge_and_package_parity(self):
        rpmdb_dir = self.cache_dir / "rpmdb_root"
        unsquashfs_extract_paths(
            self.squashfs_path,
            rpmdb_dir,
            ["usr/lib/sysimage/rpm", "var/lib/rpm"],
        )
        installed = rpm_query_root(rpmdb_dir)
        installed_set = set(installed)

        self.assertGreater(
            len(installed),
            1000,
            f"RPMDB query returned unexpectedly few packages ({len(installed)}) — empty DB?",
        )
        self.assertLess(len(installed), 1950, f"Package count unexpectedly high: {len(installed)}")
        for pkg in FORBIDDEN_LIVE_RPMS:
            self.assertNotIn(pkg, installed_set, f"Forbidden Workstation/GNOME RPM still installed: {pkg}")
        for pkg in installed:
            self.assertFalse(
                pkg.startswith("gnome-shell-extension-"),
                f"GNOME Shell extension RPM not purged: {pkg}",
            )
            self.assertFalse(
                pkg.startswith("libreoffice"),
                f"LibreOffice RPM not purged: {pkg}",
            )
            self.assertFalse(
                pkg.startswith("libvirt"),
                f"Libvirt RPM not purged: {pkg}",
            )
        for required_pkg in ("qemu-guest-agent", "htop", "sddm", "mate-polkit"):
            self.assertIn(required_pkg, installed_set, f"Required RPM missing: {required_pkg}")
        self.assertIn("usr/bin/niri", self.squashfs_paths, "Missing overlay binary: usr/bin/niri")
        niri_mode = self.squashfs_modes.get("usr/bin/niri", "")
        self.assertTrue(
            niri_mode.startswith("-") and "x" in niri_mode[:10],
            f"usr/bin/niri is not an executable regular file in squashfs: {niri_mode!r}",
        )

    def test_f13_systemd_unit_and_preset_parity(self):
        preset_rel = "etc/systemd/system-preset/00-blazeos.preset"
        self.assertIn(preset_rel, self.squashfs_paths, f"Missing systemd preset: {preset_rel}")
        preset_txt = unsquashfs_cat(self.squashfs_path, preset_rel).decode("utf-8")
        self.assertIn("enable sddm.service", preset_txt)
        self.assertIn("enable blaze-sentinel.service", preset_txt)
        self.assertIn("disable gdm.service", preset_txt)

        ext_sys = self.cache_dir / "systemd_etc"
        unsquashfs_extract_paths(self.squashfs_path, ext_sys, ["etc/systemd/system"])
        sys_dir = ext_sys / "etc/systemd/system"

        for rel_glob in FORBIDDEN_SYSTEMD_WANTS_GLOBS:
            parent_rel, pattern = rel_glob.split("/", 1)
            matches = list((sys_dir / parent_rel).glob(pattern)) if (sys_dir / parent_rel).is_dir() else []
            self.assertEqual(matches, [], f"Unwanted systemd .wants still present: {matches}")

        for req_rel in REQUIRED_ENABLED_UNITS:
            p = self.cache_dir / "systemd_etc" / req_rel
            self.assertTrue(p.exists() or p.is_symlink(), f"Required systemd unit symlink missing: {req_rel}")

        opt_svc = unsquashfs_cat(self.squashfs_path, "etc/systemd/system/blaze-optimization.service").decode(
            "utf-8"
        )
        self.assertIn("/usr/bin/blaze-optimize", opt_svc)

    def test_f14_anaconda_webui_and_pyanaconda_parity(self):
        self.assertIn("usr/bin/liveinst", self.squashfs_paths, "Missing usr/bin/liveinst")
        self.assertIn("x", self.squashfs_modes.get("usr/bin/liveinst", "")[:10])

        conf_txt = unsquashfs_cat(self.squashfs_path, "etc/anaconda/conf.d/99-blazeos.conf").decode(
            "utf-8"
        )
        self.assertRegex(conf_txt, r"(?m)^default_scheme\s*=\s*BTRFS\s*$")

        index_js = unsquashfs_cat(
            self.squashfs_path, "usr/share/cockpit/anaconda-webui/index.js"
        ).decode("utf-8", errors="replace")
        index_gz = gzip.decompress(
            unsquashfs_cat(self.squashfs_path, "usr/share/cockpit/anaconda-webui/index.js.gz")
        ).decode("utf-8", errors="replace")
        self.assertEqual(index_js, index_gz, "anaconda-webui index.js.gz is out of sync with index.js")
        self.assertIn('"solarui","gnome"', index_js)
        self.assertIn("desktopNames", index_js)
        self.assertIn('gnome:"GNOME"', index_js)
        self.assertIn("anaconda_desktop_choice", index_js)

        inst_rel = "usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py"
        const_rel = "usr/lib64/python3.15/site-packages/pyanaconda/core/constants.py"
        iface_rel = (
            "usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/"
            "install_manager/installation_category_interface.py"
        )
        inst_src = unsquashfs_cat(self.squashfs_path, inst_rel).decode("utf-8")
        const_src = unsquashfs_cat(self.squashfs_path, const_rel).decode("utf-8")
        iface_src = unsquashfs_cat(self.squashfs_path, iface_rel).decode("utf-8")

        compile(inst_src, inst_rel, "exec")
        self.assertIn("class BlazeFinalizeInstallationTask", inst_src)
        self.assertIn("BlazeFinalizeInstallationTask(conf.target.system_root).run", inst_src)
        self.assertIn("/usr/bin/blazeos-desktop-provision", inst_src)
        self.assertNotIn("/usr/local/bin/blazeos-desktop-provision", inst_src)
        self.assertIn("CategoryReportTaskInterface(self)", inst_src)

        self.assertIn("CATEGORY_FINALIZATION", const_src)
        self.assertIn("def CurrentCategory", iface_src)

        # Verify every CATEGORY_* constant imported by installation.py exists in 45.30-1 constants.py
        tree = ast.parse(inst_src)
        imported_cats: set[str] = set()
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.module == "pyanaconda.core.constants":
                for alias in node.names:
                    if alias.name.startswith("CATEGORY_"):
                        imported_cats.add(alias.name)
        self.assertTrue(imported_cats, "installation.py imports no CATEGORY_* constants")
        for cat_sym in sorted(imported_cats):
            self.assertRegex(
                const_src,
                rf"(?m)^{re.escape(cat_sym)}\s*=",
                f"{cat_sym} missing from Anaconda constants.py",
            )

    def test_f06_f07_f08_f09_elf_needed_and_assets_parity(self):
        ext_root = self.cache_dir / "elf_root"
        extract_targets = [
            "usr/lib64",
            "usr/lib",
            "etc/alternatives",
            "opt/FireHub",
            "usr/bin/caelestia",
            *CRITICAL_SQUASHFS_ELFS,
        ]
        unsquashfs_extract_paths(self.squashfs_path, ext_root, extract_targets)

        sys_libs_64 = _collect_direct_so_names(
            [ext_root / "usr/lib64"], ext_root=ext_root, expected_arch=X86_64_ELF_ARCH
        )
        sys_libs_32 = _collect_direct_so_names(
            [ext_root / "usr/lib"], ext_root=ext_root, expected_arch=X86_32_ELF_ARCH
        )
        qs_libs = sys_libs_64 | _collect_direct_so_names(
            [
                ext_root / "usr/lib64/quickshell",
                ext_root / "usr/lib64/quickshell/qml/Caelestia/lib",
            ],
            ext_root=ext_root,
            expected_arch=X86_64_ELF_ARCH,
        )
        firehub_libs = sys_libs_64 | _collect_direct_so_names(
            [ext_root / "opt/FireHub"], ext_root=ext_root, expected_arch=X86_64_ELF_ARCH
        )
        plymouth_libs = sys_libs_64 | _collect_direct_so_names(
            [ext_root / "usr/lib64/plymouth"], ext_root=ext_root, expected_arch=X86_64_ELF_ARCH
        )

        for elf_rel in CRITICAL_SQUASHFS_ELFS:
            elf_path = ext_root / elf_rel
            self.assertTrue(elf_path.is_file(), f"Critical ELF missing in squashfs: {elf_rel}")
            elf_arch = _read_elf_arch(elf_path)
            self.assertEqual(
                elf_arch,
                X86_64_ELF_ARCH,
                f"Unexpected ELF architecture for {elf_rel}: {elf_arch}",
            )
            needed = readelf_needed(elf_path)
            self.assertGreater(
                len(needed),
                0,
                f"readelf returned 0 NEEDED libs for dynamically linked ELF {elf_rel}",
            )
            if elf_rel.startswith("opt/FireHub/"):
                pool = firehub_libs
            elif elf_rel.startswith("usr/lib64/plymouth/"):
                pool = plymouth_libs
            elif any(k in elf_rel for k in ("quickshell", "Caelestia", "noctalia")):
                pool = qs_libs
            else:
                pool = sys_libs_64 if elf_arch == X86_64_ELF_ARCH else sys_libs_32
            missing = [lib for lib in needed if lib not in pool]
            self.assertEqual(
                missing,
                [],
                f"Unresolved NEEDED shared libraries for {elf_rel}: {missing}",
            )

        # Verify usr/bin/caelestia Python launcher and all Caelestia + Quickshell runtime plugin .so files
        caelestia_bin = ext_root / "usr/bin/caelestia"
        self.assertTrue(caelestia_bin.is_file(), "Missing usr/bin/caelestia")
        compile(caelestia_bin.read_text(encoding="utf-8"), "usr/bin/caelestia", "exec")

        caelestia_sos = sorted((ext_root / "usr/lib64/quickshell/qml/Caelestia").rglob("*.so"))
        self.assertGreater(len(caelestia_sos), 0, "No Caelestia .so plugins found")
        for so_path in caelestia_sos:
            self.assertEqual(_read_elf_arch(so_path), X86_64_ELF_ARCH, f"Non-x86_64 plugin: {so_path}")
            needed = readelf_needed(so_path)
            self.assertGreater(len(needed), 0, f"0 NEEDED libs for {so_path}")
            missing = [lib for lib in needed if lib not in qs_libs]
            self.assertEqual(missing, [], f"Unresolved NEEDED libs for {so_path.name}: {missing}")

        plugins_dir = ext_root / "usr/lib64/quickshell/plugins"
        self.assertTrue((plugins_dir / "platforms").is_dir(), "Missing quickshell platforms plugins")
        plugin_sos = sorted(plugins_dir.rglob("*.so"))
        self.assertGreater(len(plugin_sos), 0, "No quickshell plugin .so files found")
        for so_path in plugin_sos:
            if so_path.name in OPTIONAL_QUICKSHELL_PLUGINS:
                continue
            self.assertEqual(_read_elf_arch(so_path), X86_64_ELF_ARCH, f"Non-x86_64 plugin: {so_path}")
            needed = readelf_needed(so_path)
            self.assertGreater(len(needed), 0, f"0 NEEDED libs for {so_path}")
            missing = [lib for lib in needed if lib not in qs_libs]
            self.assertEqual(
                missing,
                [],
                f"Unresolved NEEDED libs for quickshell plugin {so_path.relative_to(plugins_dir)}: {missing}",
            )

        # FireHub chrome-sandbox SUID check
        sandbox_lines = [l for l in self.squashfs_tree if l.endswith("opt/FireHub/chrome-sandbox")]
        self.assertTrue(sandbox_lines, "opt/FireHub/chrome-sandbox missing")
        self.assertTrue(
            sandbox_lines[0].startswith("-rws"),
            f"chrome-sandbox lacks SUID 4755: {sandbox_lines[0]}",
        )

        # Noctalia assets, quickshell plugins, launchers, and CLI tools parity
        for req_path in (
            "usr/share/solarui/noctalia-assets/noctalia.svg",
            "usr/share/solarui/noctalia-assets/emoji.json",
            "usr/lib64/quickshell/plugins/platforms",
            "usr/bin/fastfetch",
            "usr/bin/yad",
            "usr/bin/alacritty",
            "usr/bin/niri-session",
            "usr/share/applications/blazeos-control.desktop",
            "usr/share/applications/blazeos-welcome.desktop",
            "usr/share/applications/blazeos-waydroid-setup.desktop",
            "usr/share/applications/solar-settings.desktop",
            "usr/share/applications/solar-welcome.desktop",
        ):
            self.assertTrue(
                any(p == req_path or p.startswith(req_path + "/") for p in self.squashfs_paths),
                f"Missing runtime parity artifact in squashfs: {req_path}",
            )

        # Stale usr/local/bin/__pycache__ must be cleaned from squashfs
        self.assertFalse(
            any(
                p == "usr/local/bin/__pycache__" or p.startswith("usr/local/bin/__pycache__/")
                for p in self.squashfs_paths
            ),
            "Stale usr/local/bin/__pycache__ found in squashfs",
        )

        autostart_welcome = unsquashfs_cat(
            self.squashfs_path, "etc/skel/.config/autostart/blazeos-welcome.desktop"
        ).decode("utf-8")
        self.assertIn("Exec=/usr/bin/blazeos-welcome", autostart_welcome)
        self.assertNotIn("/usr/local/bin/blazeos-welcome", autostart_welcome)

    def test_f16_deep_identity_persistence_and_rpm_ownership(self):
        rpmdb_dir = self.cache_dir / "rpmdb_identity"
        unsquashfs_extract_paths(
            self.squashfs_path,
            rpmdb_dir,
            ["usr/lib/sysimage/rpm", "var/lib/rpm"],
        )
        installed_set = set(rpm_query_root(rpmdb_dir))
        self.assertIn("blaze-release", installed_set, "blaze-release RPM not installed in squashfs")

        for rel, expected in (
            ("etc/os-release", 'PRETTY_NAME="Blaze SolarEvolution 5.05.1"'),
            ("usr/lib/os-release", 'PRETTY_NAME="Blaze SolarEvolution 5.05.1"'),
            ("etc/redhat-release", "Blaze SolarEvolution release 5.05.1 (SolarEvolution)"),
            ("etc/system-release", "Blaze SolarEvolution release 5.05.1 (SolarEvolution)"),
            ("etc/fedora-release", "Blaze SolarEvolution release 5.05.1 (SolarEvolution)"),
            (".buildstamp", "Product=Blaze SolarEvolution"),
            ("etc/default/grub", 'GRUB_DISTRIBUTOR="Blaze SolarEvolution"'),
            ("etc/dnf/vars/releasever", "45"),
            ("usr/share/dnf5/vars.d/releasever", "45"),
            ("etc/dnf/libdnf5.conf.d/10-blaze-identity.conf", "excludepkgs=fedora-release*"),
            ("etc/dnf/protected.d/blaze-release.conf", "blaze-release"),
            ("etc/anaconda/profile.d/blazeos-solarevolution.conf", "profile_id = blazeos-solarevolution"),
        ):
            txt = unsquashfs_cat(self.squashfs_path, rel).decode("utf-8", errors="replace")
            self.assertIn(expected, txt, f"{rel} missing expected token {expected!r}")

        for rel in (
            "etc/kernel/install.d/05-blaze-identity.install",
            "etc/kernel/install.d/99-zz-blaze-identity.install",
            "usr/bin/blazeos-sync-identity",
        ):
            self.assertIn(rel, self.squashfs_paths, f"Missing identity hook in squashfs: {rel}")
            self.assertIn("x", self.squashfs_modes.get(rel, "")[:10], f"Not executable in squashfs: {rel}")

        index_js = unsquashfs_cat(
            self.squashfs_path, "usr/share/cockpit/anaconda-webui/index.js"
        ).decode("utf-8", errors="replace")
        self.assertIn('n["os-name"].v.includes("Blaze")', index_js)
        self.assertIn('m["os-name"].v.includes("Blaze")', index_js)
        self.assertIn("bztBrand(", index_js)

        ext_id = self.cache_dir / "initrd_identity"
        extract_initrd_zstd_files(
            self.initrd_path,
            [
                "usr/lib/initrd-release",
                "usr/lib/fedora-release",
                ".buildstamp",
            ],
            ext_id,
        )
        initrd_rel = (ext_id / "usr/lib/initrd-release").read_text(encoding="utf-8")
        self.assertIn("ID=blazeos", initrd_rel)
        self.assertIn("Fedora", initrd_rel)
        self.assertIn('PRETTY_NAME="Blaze SolarEvolution 5.05.1', initrd_rel)
        self.assertEqual(
            (ext_id / "usr/lib/fedora-release").read_text(encoding="utf-8").strip(),
            "Blaze SolarEvolution release 5.05.1 (SolarEvolution)",
        )
        self.assertIn("Product=Blaze SolarEvolution", (ext_id / ".buildstamp").read_text(encoding="utf-8"))

    def test_f17_github_channel_and_bere_vault_parity(self):
        ch_conf = unsquashfs_cat(
            self.squashfs_path, "etc/blazeos/update-channel.conf"
        ).decode("utf-8", errors="replace")
        self.assertIn("DarkMorpheus-pc/Blaze-SolarEvolution", ch_conf)
        self.assertIn("repo/channel.json", ch_conf)

        index_js = unsquashfs_cat(
            self.squashfs_path, "usr/share/cockpit/anaconda-webui/index.js"
        ).decode("utf-8", errors="replace")
        self.assertIn("anaconda_bere_vault_mode", index_js)

        utils_py = unsquashfs_cat(
            self.squashfs_path,
            "usr/lib64/python3.15/site-packages/pyanaconda/modules/storage/partitioning/automatic/utils.py",
        ).decode("utf-8", errors="replace")
        self.assertIn("anaconda_bere_vault_mode", utils_py)
        self.assertIn('Size("4400MiB")', utils_py)

        prov_sh = unsquashfs_cat(
            self.squashfs_path, "usr/libexec/blazeos-provision-bere-vault"
        ).decode("utf-8", errors="replace")
        self.assertIn("full_os_embedded", prov_sh)
        self.assertIn("blockdev --setro", prov_sh)

        cfgstub = unsquashfs_cat(
            self.squashfs_path, "usr/bin/gen_grub_cfgstub"
        ).decode("utf-8", errors="replace")
        self.assertIn("BLAZE_RECOVERY_VAULT.json", cfgstub)

        bere_gui = unsquashfs_cat(
            self.squashfs_path, "usr/bin/berp-recovery-gui"
        ).decode("utf-8", errors="replace")
        self.assertIn("vault_verify_and_boot", bere_gui)
        self.assertIn("vault://BLAZE_REC", bere_gui)

    def test_f18_akrep_v1_packaging_profiles_and_polkit_parity(self):
        rpmdb_dir = self.cache_dir / "rpmdb_akrep"
        unsquashfs_extract_paths(
            self.squashfs_path,
            rpmdb_dir,
            ["usr/lib/sysimage/rpm", "var/lib/rpm"],
        )
        installed_set = set(rpm_query_root(rpmdb_dir))
        for pkg in ("blaze-akrep", "blaze-akrep-daemon", "blaze-akrep-ui"):
            self.assertIn(pkg, installed_set, f"{pkg} RPM missing from squashfs rpmdb")

        for rel in (
            "usr/bin/akrepd",
            "usr/bin/akrep-privd",
            "usr/bin/akrep-ui",
            "usr/lib/sysusers.d/blaze-akrep.conf",
            "usr/lib/tmpfiles.d/blaze-akrep.conf",
            "usr/lib/systemd/system/akrepd.service",
            "usr/lib/systemd/system/akrep-privd.service",
            "usr/lib/systemd/system-preset/90-blaze-akrep.preset",
            "usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.conf",
            "usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.Privileged.conf",
            "usr/share/dbus-1/system-services/tr.com.blazeos.Akrep1.Privileged.service",
            "usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy",
            "usr/share/applications/tr.com.blazeos.Akrep.desktop",
            "usr/share/akrep/profiles/balanced.toml",
            "usr/share/akrep/profiles/developer.toml",
            "usr/share/akrep/profiles/gaming.toml",
            "usr/share/akrep/profiles/public-network.toml",
        ):
            self.assertIn(rel, self.squashfs_paths, f"Missing Akrep v1.0 file in squashfs: {rel}")

        polkit_xml = unsquashfs_cat(
            self.squashfs_path, "usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy"
        ).decode("utf-8", errors="replace")
        self.assertIn("auth_admin_keep", polkit_xml)
        for act in (
            "tr.com.blazeos.Akrep1.selinux.diagnose",
            "tr.com.blazeos.Akrep1.selinux.manage",
            "tr.com.blazeos.Akrep1.firewall.manage",
            "tr.com.blazeos.Akrep1.flatpak.system-manage",
            "tr.com.blazeos.Akrep1.quarantine.manage",
            "tr.com.blazeos.Akrep1.profile.apply",
            "tr.com.blazeos.Akrep1.packages.install-optional",
        ):
            self.assertIn(act, polkit_xml)


if __name__ == "__main__":
    unittest.main()
