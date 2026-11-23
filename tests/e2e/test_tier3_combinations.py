"""Tier 3: Cross-feature pairwise integration tests across ISO and overlay contracts."""
import gzip
import os
from pathlib import Path
import tempfile
import unittest

from tests.e2e.config import (
    CUSTOM_14_TOOLS,
    discover_overlay_usr_bin_tools,
    get_target_iso_path,
)
from tests.e2e.harness.iso_inspect import (
    extract_initrd_zstd_cpio_list,
    unsquashfs_cat,
    unsquashfs_list,
    xorriso_extract,
)

RUN_E2E = os.environ.get("BLAZE_RUN_E2E") == "1"


@unittest.skipUnless(
    RUN_E2E,
    "Set BLAZE_RUN_E2E=1 or run `python3 -m tests.e2e.run_e2e --tier 3`",
)
class Tier3CrossFeatureTests(unittest.TestCase):
    """Verifies pairwise integration contracts across subsystems inside the built ISO."""

    tmpdir_obj: tempfile.TemporaryDirectory | None = None
    cache_dir: Path
    iso_path: Path
    initrd_path: Path
    squashfs_path: Path
    squashfs_paths: set[str]

    @classmethod
    def setUpClass(cls) -> None:
        cls.iso_path = get_target_iso_path()
        if not cls.iso_path.is_file():
            raise AssertionError(f"Target ISO not found: {cls.iso_path}")
        cls.tmpdir_obj = tempfile.TemporaryDirectory(prefix="blaze_t3_iso_")
        cls.addClassCleanup(cls.tmpdir_obj.cleanup)
        cls.cache_dir = Path(cls.tmpdir_obj.name)
        cls.initrd_path = xorriso_extract(
            cls.iso_path, "/boot/x86_64/loader/initrd", cls.cache_dir / "initrd"
        )
        cls.squashfs_path = xorriso_extract(
            cls.iso_path, "/LiveOS/squashfs.img", cls.cache_dir / "squashfs.img"
        )
        cls.squashfs_paths = set()
        marker = "squashfs-root/"
        for line in unsquashfs_list(cls.squashfs_path):
            idx = line.find(marker)
            if idx != -1:
                mode = line.split(None, 1)[0]
                rel = line[idx + len(marker) :]
                if mode.startswith("l") and " -> " in rel:
                    rel = rel.split(" -> ", 1)[0]
                cls.squashfs_paths.add(rel)

    @classmethod
    def tearDownClass(cls) -> None:
        if cls.tmpdir_obj is not None:
            cls.tmpdir_obj.cleanup()
            cls.tmpdir_obj = None

    def test_pair_boot_plymouth_sddm_livesys_solarui_shells(self):
        grub = xorriso_extract(
            self.iso_path, "/boot/grub2/grub.cfg", self.cache_dir / "grub.cfg"
        ).read_text(encoding="utf-8")
        self.assertIn("rhgb", grub)
        self.assertIn("rd.live.image", grub)
        self.assertIn("CDLABEL=Blaze-SE-5", grub)

        initrd_members = extract_initrd_zstd_cpio_list(self.initrd_path)
        self.assertIn("etc/plymouth/plymouthd.conf", initrd_members)
        self.assertIn("usr/lib64/plymouth/script.so", initrd_members)

        livesys = unsquashfs_cat(self.squashfs_path, "etc/sysconfig/livesys").decode("utf-8")
        self.assertIn('livesys_session="solarui"', livesys)
        livesys_solarui = unsquashfs_cat(
            self.squashfs_path, "usr/libexec/livesys/sessions.d/livesys-solarui"
        ).decode("utf-8")
        self.assertIn("Session=solarui.desktop", livesys_solarui)
        self.assertIn("User=liveuser", livesys_solarui)

        solarui_desktop = unsquashfs_cat(
            self.squashfs_path, "usr/share/wayland-sessions/solarui.desktop"
        ).decode("utf-8")
        self.assertIn("Exec=/usr/bin/solar-session-wrapper", solarui_desktop)
        self.assertIn("Icon=solarui", solarui_desktop)

        # Verify all 3 SolarUI shell engines have configs/assets in squashfs
        for path in (
            "usr/bin/niri",
            "usr/bin/solar-session",
            "usr/bin/solar-core",
            "usr/bin/solar-shell",
            "usr/bin/noctalia",
            "usr/share/solarui/noctalia-assets/noctalia.svg",
            "usr/bin/caelestia",
            "usr/bin/quickshell.bin",
            "usr/bin/quickshell-fedora",
            "etc/skel/.config/noctalia/settings.toml",
            "etc/skel/.config/caelestia/shell.json",
            "usr/share/quickshell/end4-pC",
        ):
            self.assertTrue(
                any(p == path or p.startswith(path + "/") for p in self.squashfs_paths),
                f"Boot-to-shell chain missing {path}",
            )

    def test_pair_live_session_firehub_and_dnf5_blaze_repo(self):
        firehub_wrapper = unsquashfs_cat(self.squashfs_path, "usr/bin/firehub").decode("utf-8")
        self.assertIn("/opt/FireHub/firehub", firehub_wrapper)
        self.assertIn("opt/FireHub/resources/app.asar", self.squashfs_paths)
        self.assertIn("usr/bin/dnf5", self.squashfs_paths)
        self.assertIn("usr/bin/flatpak", self.squashfs_paths)
        self.assertIn("etc/flatpak/remotes.d/flathub.flatpakrepo", self.squashfs_paths)

        repo_txt = unsquashfs_cat(self.squashfs_path, "usr/share/dnf5/repos.d/blaze.repo").decode(
            "utf-8"
        )
        self.assertIn("enabled=1", repo_txt)
        self.assertIn("gpgcheck=1", repo_txt)
        self.assertIn("repo_gpgcheck=1", repo_txt)
        self.assertIn("etc/pki/rpm-gpg/RPM-GPG-KEY-blaze", self.squashfs_paths)

    def test_pair_live_session_anaconda_btrfs_gnome_and_polkit(self):
        livesys_solarui = unsquashfs_cat(
            self.squashfs_path, "usr/libexec/livesys/sessions.d/livesys-solarui"
        ).decode("utf-8")
        self.assertIn("20-livesys-solarui.rules", livesys_solarui)
        self.assertIn("liveinst.desktop", livesys_solarui)

        ana_conf = unsquashfs_cat(self.squashfs_path, "etc/anaconda/conf.d/99-blazeos.conf").decode(
            "utf-8"
        )
        self.assertIn("default_scheme = BTRFS", ana_conf)
        self.assertIn("btrfs_compression = zstd:1", ana_conf)

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

        provision = unsquashfs_cat(self.squashfs_path, "usr/bin/blazeos-desktop-provision").decode(
            "utf-8"
        )
        self.assertIn("gnome:full)", provision)
        self.assertIn("@workstation-product-environment", provision)

    def test_pair_custom_tools_launchers_autostart_and_assets(self):
        overlay_tools = discover_overlay_usr_bin_tools()
        self.assertTrue(set(CUSTOM_14_TOOLS).issubset(set(overlay_tools)))
        for tool in overlay_tools:
            self.assertIn(f"usr/bin/{tool}", self.squashfs_paths)

        autostart = unsquashfs_cat(
            self.squashfs_path, "etc/skel/.config/autostart/blazeos-welcome.desktop"
        ).decode("utf-8")
        self.assertIn("Exec=/usr/bin/blazeos-welcome", autostart)

        for desktop_file, expected_exec in (
            ("usr/share/applications/blazeos-control.desktop", "blazeos-control"),
            ("usr/share/applications/blazeos-welcome.desktop", "blazeos-welcome"),
            ("usr/share/applications/blazeos-waydroid-setup.desktop", "blazeos-waydroid-setup"),
            ("usr/share/applications/solar-settings.desktop", "solar-shell settings"),
            ("usr/share/applications/solar-welcome.desktop", "solar-shell welcome"),
        ):
            content = unsquashfs_cat(self.squashfs_path, desktop_file).decode("utf-8")
            self.assertIn(expected_exec, content, f"{desktop_file} missing {expected_exec}")

        for dep in ("usr/bin/fastfetch", "usr/bin/yad", "usr/lib/kitty/bin/kitty", "usr/bin/blaze-house"):
            self.assertIn(dep, self.squashfs_paths, f"Missing tool dependency {dep}")


if __name__ == "__main__":
    unittest.main()
