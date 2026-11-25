"""Tier 2: Boundary, negative, and tamper-detection tests (isolated sandboxes only)."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

from tests.e2e.config import OVERLAY_DIR, PACKAGING_REPO_DIR, get_niri_validate_binary, get_target_iso_path
from tests.e2e.harness.iso_inspect import rpm_verify_signatures, verify_iso_sha256_file

RUN_E2E = os.environ.get("BLAZE_RUN_E2E") == "1"


@unittest.skipUnless(
    RUN_E2E,
    "Set BLAZE_RUN_E2E=1 or run `python3 -m tests.e2e.run_e2e --tier 2`",
)
class Tier2BoundaryAndNegativeTests(unittest.TestCase):
    """Verifies graceful error handling, invalid args, and tamper detection."""

    def test_blaze_sentinel_invalid_args_and_sandbox_tamper_detection(self):
        sentinel = str(OVERLAY_DIR / "usr/bin/blaze-sentinel")
        with tempfile.TemporaryDirectory(prefix="blaze_t2_sent_") as tmp:
            env = {**os.environ, "HOME": tmp, "TMPDIR": tmp, "XDG_RUNTIME_DIR": tmp}

            bad = subprocess.run(
                [sentinel, "invalid-subcommand-xyz"],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(bad.returncode, 2)
            self.assertIn("unknown command", bad.stderr)

            ver = subprocess.run(
                [sentinel, "--version"], env=env, capture_output=True, text=True, timeout=10
            )
            self.assertEqual(ver.returncode, 0)
            self.assertIn("0.3.1", ver.stdout)

            st = subprocess.run(
                [sentinel, "status", "--json"], env=env, capture_output=True, text=True, timeout=10
            )
            self.assertEqual(st.returncode, 0)
            payload = json.loads(st.stdout)
            self.assertEqual(payload.get("total"), 7)

            sim = subprocess.run(
                [sentinel, "--simulate-attack"], env=env, capture_output=True, text=True, timeout=10
            )
            self.assertEqual(sim.returncode, 0, sim.stderr)
            self.assertIn("Canary tamper detected successfully", sim.stdout)

            # Real sandboxed check-once -> tamper -> re-check
            c1 = subprocess.run(
                [sentinel, "check-once", "--json"], env=env, capture_output=True, text=True, timeout=10
            )
            self.assertEqual(c1.returncode, 0, c1.stdout + c1.stderr)
            canary = Path(tmp) / ".config/.blaze_canary"
            self.assertTrue(canary.is_file())
            canary.write_text("TAMPERED_CONTENT_E2E", encoding="utf-8")

            c2 = subprocess.run(
                [sentinel, "check-once", "--json"], env=env, capture_output=True, text=True, timeout=10
            )
            self.assertEqual(c2.returncode, 1)
            j2 = json.loads(c2.stdout)
            self.assertEqual(j2.get("status"), "alert")
            self.assertTrue(any("RANSOMWARE" in a for a in j2.get("alerts", [])))

    def test_blaze_recovery_readonly_flags(self):
        recovery = str(OVERLAY_DIR / "usr/bin/blaze-recovery")
        for flag in ("--help", "--version", "--status", "--list-snapshots", "--verify"):
            res = subprocess.run([recovery, flag], capture_output=True, text=True, timeout=10)
            self.assertEqual(res.returncode, 0, f"{flag} failed: {res.stderr}")
        ver = subprocess.run([recovery, "--version"], capture_output=True, text=True, timeout=10)
        self.assertIn("5.05", ver.stdout)

    def test_blaze_snapshot_invalid_inputs_rejected(self):
        snap = str(OVERLAY_DIR / "usr/bin/blaze-snapshot")
        cases = (
            ["unknown-cmd-xyz"],
            ["restore"],
            ["restore", "../etc/passwd"],
            ["delete", "bad/name"],
        )
        for args in cases:
            res = subprocess.run([snap, *args], capture_output=True, text=True, timeout=10)
            self.assertNotEqual(res.returncode, 0, f"Expected failure for {args}, got rc=0")

    def test_solar_shell_invalid_engine_and_invalid_kdl(self):
        solar_shell = str(OVERLAY_DIR / "usr/bin/solar-shell")
        with tempfile.TemporaryDirectory(prefix="blaze_t2_sh_") as tmp:
            env = {**os.environ, "HOME": tmp, "XDG_RUNTIME_DIR": tmp}
            res = subprocess.run(
                [solar_shell, "switch", "nonexistent-desktop-engine"],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertNotEqual(res.returncode, 0)
            self.assertIn("noctalia, caelestia, end4-pc", res.stdout + res.stderr)

            bad_kdl = Path(tmp) / "bad.kdl"
            bad_kdl.write_text("this-is-not { valid kdl syntax !!!", encoding="utf-8")
            niri_bin = get_niri_validate_binary() or "niri"
            niri_res = subprocess.run(
                [niri_bin, "validate", "-c", str(bad_kdl)],
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertNotEqual(niri_res.returncode, 0)

    def test_rpm_and_repomd_tamper_rejected(self):
        gpg_key = PACKAGING_REPO_DIR / "RPM-GPG-KEY-blaze"
        orig_rpm = PACKAGING_REPO_DIR / "blaze-sentinel-0.3.1-2.blaze.x86_64.rpm"
        orig_repomd = PACKAGING_REPO_DIR / "repodata/repomd.xml"
        orig_asc = PACKAGING_REPO_DIR / "repodata/repomd.xml.asc"

        with tempfile.TemporaryDirectory(prefix="blaze_t2_tamper_") as tmp:
            t = Path(tmp)
            tampered_rpm = t / "tampered.rpm"
            data = bytearray(orig_rpm.read_bytes())
            data[len(data) // 2] ^= 0xFF
            tampered_rpm.write_bytes(bytes(data))
            verdicts = rpm_verify_signatures([tampered_rpm], gpg_key)
            verdict = verdicts[str(tampered_rpm)]
            self.assertFalse(verdict.startswith("rc=0"), f"Tampered RPM unexpectedly passed: {verdict}")

            tampered_xml = t / "repomd.xml"
            tampered_xml.write_bytes(orig_repomd.read_bytes() + b"\n<!-- tampered -->\n")
            gpghome = t / "gnupg"
            gpghome.mkdir(mode=0o700)
            env = {**os.environ, "GNUPGHOME": str(gpghome), "LC_ALL": "C", "LANG": "C"}
            subprocess.run(["gpg", "--import", str(gpg_key)], env=env, check=True, capture_output=True, timeout=30)
            gpg_res = subprocess.run(
                ["gpg", "--verify", str(orig_asc), str(tampered_xml)],
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
            )
            self.assertNotEqual(gpg_res.returncode, 0, "Tampered repomd.xml unexpectedly passed GPG")

    def test_iso_sha256_mismatch_detected(self):
        iso_path = get_target_iso_path()
        if not iso_path.is_file():
            self.fail(f"Target ISO missing: {iso_path}")
        with tempfile.TemporaryDirectory(prefix="blaze_t2_iso_") as tmp:
            bad_sha = Path(tmp) / "bad.sha256"
            bad_sha.write_text(("0" * 64) + f"  {iso_path.name}\n", encoding="utf-8")
            ok, _ = verify_iso_sha256_file(iso_path, bad_sha)
            self.assertFalse(ok)

    def test_identity_sync_rejects_invalid_args_and_recovers_forced_drift(self):
        sync_bin = str(OVERLAY_DIR / "usr/bin/blazeos-sync-identity")
        bad = subprocess.run([sync_bin, "--nonexistent-flag"], capture_output=True, text=True, timeout=10)
        self.assertEqual(bad.returncode, 2)

        with tempfile.TemporaryDirectory(prefix="blaze_t2_id_") as tmp:
            root = Path(tmp)
            shutil.copytree(OVERLAY_DIR / "usr/share/blazeos/identity", root / "usr/share/blazeos/identity")
            for sub in ("etc/default", "etc/dnf/vars", "etc/dnf/libdnf5.conf.d", "etc/dnf/protected.d", "etc/anaconda/profile.d", "usr/lib", "boot/loader/entries"):
                (root / sub).mkdir(parents=True, exist_ok=True)
            (root / "usr/lib/os-release").write_text('NAME="Fedora Linux"\nPRETTY_NAME="Fedora Linux 45"\n', encoding="utf-8")
            (root / "usr/lib/fedora-release").write_text("Fedora release 45 (Forty Five)\n", encoding="utf-8")
            (root / "etc/default/grub").write_text('GRUB_DISTRIBUTOR="Fedora"\n', encoding="utf-8")
            (root / "boot/loader/entries/test-7.3.4.conf").write_text(
                "title Fedora Linux (7.3.4-300.fc45.x86_64) 45 (Workstation Edition)\n"
                "version 7.3.4-300.fc45.x86_64\n"
                "grub_class fedora\n",
                encoding="utf-8",
            )
            res = subprocess.run([sync_bin, f"--sysroot={root}"], capture_output=True, text=True, timeout=10)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertIn('PRETTY_NAME="Blaze SolarEvolution 5.05.1"', (root / "etc/os-release").read_text(encoding="utf-8"))
            self.assertEqual((root / "etc/redhat-release").read_text(encoding="utf-8").strip(), "Blaze SolarEvolution release 5.05.1 (SolarEvolution)")
            self.assertEqual((root / "etc/dnf/vars/releasever").read_text(encoding="utf-8").strip(), "45")
            bls = (root / "boot/loader/entries/test-7.3.4.conf").read_text(encoding="utf-8")
            self.assertIn("title Blaze SolarEvolution (7.3.4-300.fc45.x86_64) 5.05.1", bls)
            self.assertIn("grub_class blazeos", bls)


if __name__ == "__main__":
    unittest.main()
