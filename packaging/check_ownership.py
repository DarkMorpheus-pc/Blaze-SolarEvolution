#!/usr/bin/env python3
"""Verify 100% overlay ownership coverage against packaging/ownership-manifest.yaml
and (when --rootfs is supplied) verify every first-party-rpm file is owned in rpmdb.sqlite.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

VALID_CLASSES = {"first-party-rpm", "fedora-config-override", "third-party-vendor", "excluded"}


def load_manifest(path: Path) -> list[dict[str, str]]:
    text = path.read_text(encoding="utf-8")
    try:
        import yaml  # type: ignore

        data = yaml.safe_load(text)
        rules = data.get("rules", []) if isinstance(data, dict) else []
        return [dict(r) for r in rules]
    except ImportError:
        pass

    rules: list[dict[str, str]] = []
    cur: dict[str, str] | None = None
    in_rules = False
    for raw in text.splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line == "rules:":
            in_rules = True
            continue
        if not in_rules:
            continue
        if line.startswith("- "):
            if cur:
                rules.append(cur)
            cur = {}
            line = line[2:].strip()
        if ":" in line and cur is not None:
            k, v = line.split(":", 1)
            cur[k.strip()] = v.strip().strip('"').strip("'")
    if cur:
        rules.append(cur)
    return rules


def match_rule(rel_path: str, rules: list[dict[str, str]]) -> dict[str, str] | None:
    best: dict[str, str] | None = None
    best_len = -1
    for r in rules:
        p = r.get("prefix", "").lstrip("/")
        if rel_path == p or rel_path.startswith(p.rstrip("/") + "/"):
            if len(p) > best_len:
                best = r
                best_len = len(p)
    return best


def main() -> int:
    ap = argparse.ArgumentParser(description="Verify BlazeOS overlay & rootfs RPM ownership.")
    ap.add_argument("--overlay", type=Path, default=Path(__file__).resolve().parents[1] / "blazeos_custom_apps")
    ap.add_argument("--manifest", type=Path, default=Path(__file__).resolve().parent / "ownership-manifest.yaml")
    ap.add_argument("--packages-json", type=Path, default=Path(__file__).resolve().parent / "packages.json")
    ap.add_argument("--rootfs", type=Path, default=None, help="Installed rootfs to check via rpm --root -qf")
    ap.add_argument("--allow-missing-optional", action="store_true")
    args = ap.parse_args()

    rules = load_manifest(args.manifest)
    if not rules:
        print(f"FATAL: Empty ruleset in {args.manifest}", file=sys.stderr)
        return 1

    for r in rules:
        cls = r.get("class")
        if cls not in VALID_CLASSES:
            print(f"FATAL: Invalid class '{cls}' for prefix '{r.get('prefix')}'", file=sys.stderr)
            return 1
        if cls == "first-party-rpm" and not r.get("owner_rpm"):
            print(f"FATAL: Missing owner_rpm for '{r.get('prefix')}'", file=sys.stderr)
            return 1

    # 1. Verify 100% coverage of overlay files
    all_files = sorted(
        str(p.relative_to(args.overlay))
        for p in args.overlay.rglob("*")
        if p.is_file() or p.is_symlink()
    )
    unclassified = []
    counts = {k: 0 for k in VALID_CLASSES}
    first_party_paths: list[tuple[str, str]] = []

    for rel in all_files:
        m = match_rule(rel, rules)
        if m is None:
            unclassified.append(rel)
            continue
        cls = m["class"]
        counts[cls] += 1
        if cls == "first-party-rpm":
            if ".env" in Path(rel).name or rel.endswith(".log") or rel.endswith(".map") or "/__pycache__/" in f"/{rel}/":
                continue
            first_party_paths.append((rel, m["owner_rpm"]))

    if unclassified:
        print(f"FATAL: {len(unclassified)} overlay files lack ownership classification in {args.manifest}:", file=sys.stderr)
        for u in unclassified[:25]:
            print(f"  UNCLASSIFIED: {u}", file=sys.stderr)
        return 1

    # 2. Verify every packages.json path is reflected as first-party-rpm in ownership-manifest.yaml
    pkgs = json.loads(args.packages_json.read_text(encoding="utf-8"))
    for p in pkgs:
        pname = p["name"]
        allowed_owners = {pname} | set(p.get("subpackages", []))
        for fp in p["files"]:
            rel = fp.lstrip("/")
            m = match_rule(rel, rules)
            if not m or m.get("class") != "first-party-rpm" or m.get("owner_rpm") not in allowed_owners:
                print(
                    f"FATAL: packages.json entry {pname}:{fp} mismatches manifest ({m})",
                    file=sys.stderr,
                )
                return 1

    print(
        f"[OWNERSHIP] Overlay {len(all_files)} files 100% classified: "
        + ", ".join(f"{k}={v}" for k, v in sorted(counts.items()))
    )

    # 3. Verify rootfs rpmdb ownership of every first-party-rpm file
    if args.rootfs is not None:
        rfs = args.rootfs.resolve()
        if not rfs.is_dir():
            print(f"FATAL: rootfs missing: {rfs}", file=sys.stderr)
            return 1
        query_targets = []
        missing_in_rootfs = []
        for rel, owner in first_party_paths:
            # One-shot OOBE files may be marked missingok if OOBE completed; still present in fresh ISO rootfs.
            p = rfs / rel
            if not p.exists() and not p.is_symlink():
                missing_in_rootfs.append((rel, owner))
            else:
                query_targets.append((rel, owner, f"/{rel}"))

        if missing_in_rootfs and not args.allow_missing_optional:
            print(f"FATAL: {len(missing_in_rootfs)} first-party files missing from {rfs}:", file=sys.stderr)
            for rel, owner in missing_in_rootfs[:20]:
                print(f"  MISSING ({owner}): /{rel}", file=sys.stderr)
            return 1

        # Batch query rpm -qf --qf '%{NAME}\n' (handle Fedora /usr/lib/sysimage/rpm across non-Fedora build hosts)
        rpm_cmd = ["rpm", "--root", str(rfs)]
        if (rfs / "usr/lib/sysimage/rpm/rpmdb.sqlite").exists():
            rpm_cmd.extend(["--dbpath", "/usr/lib/sysimage/rpm"])

        batch_sz = 300
        mismatches = []
        for i in range(0, len(query_targets), batch_sz):
            batch = query_targets[i : i + batch_sz]
            paths_only = [b[2] for b in batch]
            proc = subprocess.run(
                rpm_cmd + ["-qf", "--qf", "%{NAME}\\n"] + paths_only,
                capture_output=True,
                text=True,
            )
            lines = proc.stdout.splitlines()
            if proc.returncode != 0 or len(lines) != len(batch):
                # Fallback one-by-one to pinpoint exact orphan file
                for rel, owner, absp in batch:
                    sub = subprocess.run(
                        rpm_cmd + ["-qf", "--qf", "%{NAME}\\n", absp],
                        capture_output=True,
                        text=True,
                    )
                    if sub.returncode != 0:
                        mismatches.append((rel, owner, sub.stderr.strip() or "UNOWNED"))
                    else:
                        owners = [x.strip() for x in sub.stdout.splitlines() if x.strip()]
                        if owner not in owners:
                            mismatches.append((rel, owner, f"owned by {owners}"))
            else:
                for (rel, owner, _), got in zip(batch, lines):
                    if got.strip() != owner:
                        # Check multi-owner directory files cleanly
                        sub = subprocess.run(
                            rpm_cmd + ["-qf", "--qf", "%{NAME}\\n", f"/{rel}"],
                            capture_output=True,
                            text=True,
                        )
                        owners = [x.strip() for x in sub.stdout.splitlines() if x.strip()]
                        if owner not in owners:
                            mismatches.append((rel, owner, f"owned by {owners}"))

        if mismatches:
            print(f"FATAL: {len(mismatches)} rootfs files failed first-party RPM ownership:", file=sys.stderr)
            for rel, exp, got in mismatches[:25]:
                print(f"  /{rel}: expected={exp}, got={got}", file=sys.stderr)
            return 1
        print(f"[OWNERSHIP] Verified {len(query_targets)} first-party paths in {rfs} rpmdb cleanly.")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
