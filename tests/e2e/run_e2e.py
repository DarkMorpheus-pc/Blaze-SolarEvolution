"""CLI runner for the BlazeOS / SolarEvolution 5.05.1 E2E test suite (Tiers 1-4)."""
import argparse
import os
from pathlib import Path
import sys
import unittest

TIER_MODULES = {
    "unit": "tests.e2e.test_harness_unit",
    "1": "tests.e2e.test_tier1_features",
    "2": "tests.e2e.test_tier2_boundaries",
    "3": "tests.e2e.test_tier3_combinations",
    "4": "tests.e2e.test_tier4_qemu_e2e",
    "5": "tests.e2e.test_tier5_adversarial",
}


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="python3 -m tests.e2e.run_e2e",
        description="Run BlazeOS / SolarEvolution 5.05.1 opaque-box E2E tests (Tiers 1-5).",
    )
    parser.add_argument(
        "--tier",
        choices=["unit", "1", "2", "3", "4", "5"],
        action="append",
        help="Run specific tier(s). Can be passed multiple times.",
    )
    parser.add_argument(
        "--all",
        action="store_true",
        help="Run all tiers (unit + 1 + 2 + 3 + 4 + 5).",
    )
    parser.add_argument(
        "--firmware",
        choices=["bios", "uefi", "both"],
        default="both",
        help="Firmware mode for Tier 4 QEMU E2E tests (default: both).",
    )
    parser.add_argument(
        "--iso",
        type=Path,
        default=None,
        help="Path to target ISO image (overrides BLAZE_E2E_ISO).",
    )
    parser.add_argument(
        "--artifacts-dir",
        type=Path,
        default=None,
        help="Directory for QEMU screenshots and serial logs (overrides BLAZE_E2E_ARTIFACTS).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Validate environment/arguments and list selected test modules without executing.",
    )
    parser.add_argument("-v", "--verbose", action="store_true", help="Verbose unittest output.")
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    if args.all:
        selected = ["unit", "1", "2", "3", "4", "5"]
    elif args.tier:
        selected = list(dict.fromkeys(args.tier))
    else:
        selected = ["unit", "1", "2", "3", "5"]

    if args.iso is not None:
        os.environ["BLAZE_E2E_ISO"] = str(args.iso)
    if args.artifacts_dir is not None:
        os.environ["BLAZE_E2E_ARTIFACTS"] = str(args.artifacts_dir)
    os.environ["BLAZE_QEMU_FIRMWARE"] = args.firmware

    from tests.e2e.config import OVMF_CODE_PATH, get_artifacts_dir, get_target_iso_path

    iso_path = get_target_iso_path()
    artifacts_dir = get_artifacts_dir()
    os.environ["BLAZE_E2E_ISO"] = str(iso_path)
    os.environ["BLAZE_E2E_ARTIFACTS"] = str(artifacts_dir)

    if any(t in ("1", "2", "3", "5") for t in selected):
        os.environ["BLAZE_RUN_E2E"] = "1"
    else:
        os.environ.pop("BLAZE_RUN_E2E", None)
    if "4" in selected:
        os.environ["BLAZE_RUN_QEMU_E2E"] = "1"
    else:
        os.environ.pop("BLAZE_RUN_QEMU_E2E", None)

    if args.dry_run:
        print("DRY RUN: BlazeOS E2E Test Configuration")
        print(f"  ISO:           {iso_path} (exists={iso_path.is_file()})")
        print(f"  OVMF:          {OVMF_CODE_PATH} (exists={OVMF_CODE_PATH.is_file()})")
        print(f"  KVM:           /dev/kvm (exists={Path('/dev/kvm').exists()})")
        print(f"  Firmware:      {args.firmware}")
        print(f"  Artifacts dir: {artifacts_dir}")
        print(f"  Tiers:         {', '.join(selected)}")
        for t in selected:
            print(f"    - tier {t}: {TIER_MODULES[t]}")
        return 0

    if any(t in ("1", "2", "3", "4") for t in selected) and not iso_path.is_file():
        print(f"ERROR: Target ISO not found: {iso_path}", file=sys.stderr)
        return 2

    loader = unittest.TestLoader()
    suite = unittest.TestSuite()
    for t in selected:
        suite.addTests(loader.loadTestsFromName(TIER_MODULES[t]))

    verbosity = 2 if args.verbose else 1
    runner = unittest.TextTestRunner(verbosity=verbosity)
    result = runner.run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
