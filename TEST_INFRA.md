# BlazeOS / SolarEvolution 5.05 — E2E Test Infrastructure (`TEST_INFRA.md`)

## 1. Test Philosophy
This test infrastructure implements **opaque-box, requirement-driven** end-to-end verification derived exclusively from:
- `.agents/teamwork/ORIGINAL_REQUEST.md` (Requirements `R1`–`R5` and Acceptance Criteria)
- `PROJECT.md` (`§ Architecture`, `§ Feature Inventory` Features #1–#17, `§ Interface Contracts`)
- Read-only Beta 1.3 reference baseline (`work_f45/` and `Blaze-SolarEvolution-5-x86_64(2026-11-11).iso`)

Key principles:
1. **Requirement-driven assertions**: Every test verifies an observable external contract (ISO checksum/implantisomd5, initrd/squashfs contents, ELF dynamic linking, signed RPM/GPG verification, CLI exit codes/JSON output, QEMU QMP display frames, QGA in-guest runtime state, and reference immutability).
2. **Zero dependency on missing host tools**: Built purely on Python 3 stdlib (`unittest`, `socket`, `subprocess`, `json`, `hashlib`, `base64`, `tempfile`, `pathlib`, `re`, `argparse`, `ast`, `gzip`, `shlex`) plus Pillow (`PIL`) and standard system/QEMU tools (`qemu-system-x86_64`, `ffmpeg`, `xorriso`, `unsquashfs`, `cpio`, `zstd`, `readelf`, `rpm`, `gpg`, `checkisomd5`, and `niri` for Tier 2 `niri validate -c` KDL syntax validation). Does **not** require `tesseract`, `gocr`, `ocrad`, `socat`, or `pytest`. All subprocess inspection tools run under `LC_ALL=C` / `LANG=C` for locale independence.
3. **Isolation & host safety**:
   - Never modifies `work_f45/`, `Blaze-SolarEvolution-5-x86_64(2026-11-11).iso`, or `f45_base/`.
   - Never touches or kills external QEMU processes (e.g. legacy `blaze-f45-stable-test` / `/tmp/blaze_test_qmp.sock`); uses PID-unique socket and temp directories (`/tmp/blaze_e2e_<pid>_*`).
   - Gates heavy ISO-extraction suites (`BLAZE_RUN_E2E=1`) and live QEMU VM boots (`BLAZE_RUN_QEMU_E2E=1`) so `python3 -m unittest discover -s /home/darkmorpheus/BlazeFedora/tests -v` remains fast and non-destructive by default, while `python3 -m tests.e2e.run_e2e` unlocks requested tiers cleanly (and propagates `--iso` / `--artifacts-dir` / `--firmware` dynamically).

---

## 2. Feature Inventory Mapping

| # | Feature (`PROJECT.md`) | Req (`ORIGINAL_REQUEST.md`) | Tier 1 | Tier 2 | Tier 3 | Tier 4 |
|---|---|---|---|---|---|---|
| 1 | Patched Initrd Plymouth (`Theme=blazeos`, `script.so`, `watermark.png`) | R1 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 2 | Rootfs Plymouth & Logos (`plymouthd.conf`, `script.so`, `blazeos-apply-logos`, `logo*.png`, `blazeos.script`) | R1 | `test_tier1_features.py` | — | — | `test_tier4_qemu_e2e.py` |
| 3 | GRUB2 & EFI Stub Parity (6 entries, `CDLABEL=Blaze-SE-5`, marker) | R1 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 4 | Fonts & SolarUI Icons & Wallpapers Parity | R1 | `test_tier1_features.py` | — | — | `test_tier4_qemu_e2e.py` |
| 5 | R1 Prior Fixes (`LICENSE`, `solarui/crates/`, `sentinel.rs` v0.3.1, dynamic `/usr/bin` tools incl. 14 tools, 8 RPMs, `BTRFS`) | R1 | `test_tier1_features.py` | `test_tier2_boundaries.py` | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 6 | Noctalia Shared Libs (`NEEDED` resolution) & `noctalia-assets` | R1 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 7 | Caelestia & Quickshell Plugins (`NEEDED` libs, `quickshell-fedora`, `plugins/`, `end4-pC`) | R1 | `test_tier1_features.py` | `test_tier2_boundaries.py` | `test_tier3_combinations.py` | — |
| 8 | FireHub & BlazeHouse Runtime (`chrome-sandbox` 4755, ELF deps, live launch) | R1, R3 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 9 | Custom Tools & Launchers Parity (`autostart`, 5 `.desktop`, `fastfetch`, `yad`, `alacritty`, no `usr/local/bin/__pycache__`) | R1, R3 | `test_tier1_features.py` | `test_tier2_boundaries.py` | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 10 | Packaging & DNF5 Repo Sync (8 signed RPMs, `RPM-GPG-KEY-blaze`, `blaze.repo`) | R1, R3 | `test_tier1_features.py` | `test_tier2_boundaries.py` | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 11 | Direct SolarUI Live Autologin (`livesys_session="solarui"`, `autologin.conf`, no SDDM greeter) | R2 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 12 | GNOME & Workstation Cleanup (`gnome-shell-extension-*` purged, GNOME kept for Anaconda) | R2 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 13 | Systemd Preset & Service Parity (`00-blazeos.preset`, `blaze-optimization`, clean `.wants`) | R2 | `test_tier1_features.py` | — | — | `test_tier4_qemu_e2e.py` |
| 14 | Anaconda 45.30-1 / WebUI Parity (`99-blazeos.conf` BTRFS, BlazeOS `"solarui","gnome"` selector, `pyanaconda` `BlazeFinalizeInstallationTask` / `CATEGORY_FINALIZATION`, `liveinst`) | R3 | `test_tier1_features.py` | — | `test_tier3_combinations.py` | `test_tier4_qemu_e2e.py` |
| 15 | ISO Rebuild & Ref Protection (`Blaze-SolarEvolution-5-x86_64.iso`, `.sha256`, `checkisomd5`, `work_f45/` SHA-256 unchanged) | R4 | `test_tier1_features.py` | `test_tier2_boundaries.py` | — | `test_tier4_qemu_e2e.py` |
| 16 | Opaque-Box E2E Test Suite (`TEST_INFRA.md`, `tests/e2e/`, `TEST_READY.md`) | R5 | `test_harness_unit.py` | — | — | — |
| 17 | Final E2E Pass & Tier 5 Hardening (100% pass across Tiers 1–4 + BIOS/UEFI) | R5 | All | All | All | All |

---

## 3. Test Runner Architecture

```
tests/e2e/
├── __init__.py
├── config.py                    # Dynamic env getters, immutable baseline hashes, overlay tool discovery, 8 RPMs
├── run_e2e.py                   # CLI entrypoint (--tier unit|1|2|3|4, --all, --dry-run, --firmware, --iso, --artifacts-dir; fails fast on missing ISO)
├── harness/
│   ├── qmp.py                   # AF_UNIX QMP JSON client (screendump, input-send-event, quit, proc exit watch)
│   ├── qga.py                   # AF_UNIX QGA JSON client (hardened guest-sync, short ping timeout, guest-exec, guest-exec-status)
│   ├── imaging.py               # ffmpeg/PIL PPM->PNG conversion + non-black, variance, frame-diff metrics
│   ├── qemu_vm.py               # Context-managed QEMU runner (BIOS & UEFI OVMF, stderr log, safe cleanup on failed start)
│   └── iso_inspect.py           # Read-only LC_ALL=C xorriso / unsquashfs (-no-xattrs) / zstd+cpio / readelf / rpm (--dbpath) / gpg helpers
├── test_harness_unit.py         # Always-run unit tests of harness modules (11 tests: mock QMP/QGA sockets, PIL, cleanup, CLI flags/missing ISO, Tier 4 pgrep/cleanup ordering & UI proc matching)
├── test_tier1_features.py       # Tier 1: Feature Coverage (12 tests covering Features #1-#15)
├── test_tier2_boundaries.py     # Tier 2: Boundary & Corner Cases (6 tests: invalid CLI args, sentinel fail-closed, tamper)
├── test_tier3_combinations.py   # Tier 3: Cross-Feature Pairwise Combinations (4 tests)
└── test_tier4_qemu_e2e.py       # Tier 4: Full QEMU E2E Boot (2 tests: Legacy BIOS & UEFI OVMF)
```

---

## 4. Tier Structure & Coverage Thresholds

- **Harness Self-Tests (`test_harness_unit.py`)**: Always executed by `unittest discover -s tests`. Covers mock QMP/QGA (`guest-sync` draining stale replies, malformed/partial JSON, and stale errors within bounded timeout + `_buf` clearing), PIL imaging, QEMU BIOS/UEFI command builder, `QemuVM` cleanup on failed startup, zstd offset/SHA-256 helpers, `run_e2e.py` `--iso`/`--artifacts-dir` propagation plus non-zero exit on missing ISO, and Tier 4 `guest_exec` `pgrep`/`pkill`/`systemctl stop`/cleanup ordering + `_has_installer_ui_proc` non-premature matching. Must pass 100% (11 test methods) on host without QEMU or ISO extraction.
- **Tier 1 — Feature Coverage (`test_tier1_features.py`, gate `BLAZE_RUN_E2E=1`)**:
  - Verifies ISO SHA-256 & `checkisomd5`, read-only reference immutability (`work_f45/` & `(2026-11-11).iso`), R1 prior fixes (path-boundary regex on `solarui/crates/`, `sentinel.rs` `0.3.1`, dynamic overlay `/usr/bin` tools), signed RPMs + bounded-timeout GPG `repomd.xml.asc`, GRUB2 & EFI stub, initrd & rootfs Plymouth `Theme=blazeos` + `script.so` + `blazeos-apply-logos` + `logo*.png`/`blazeos.script`, `work_f45/` font parity (`google-sans-flex`, `TTF`, `ttf-material-symbols-variable` `.ttf`) + icons + wallpapers, `livesys_session="solarui"`, GNOME extension purge via `--dbpath /usr/lib/sysimage/rpm` (`>1000` & `<1950` packages) + `/usr/bin/niri` executable overlay binary, `00-blazeos.preset` & `.wants/` parity (`graphical.target.wants/blazeos-nvidia-firstboot.service`), Feature 14 (`liveinst`, `anaconda-webui` BlazeOS `"solarui","gnome"` selector, `pyanaconda/modules/boss/installation.py` `BlazeFinalizeInstallationTask` + `45.30-1` `CATEGORY_*`/`CATEGORY_FINALIZATION`/`CurrentCategory`), scoped ELF `NEEDED` dependency resolution (`CRITICAL_SQUASHFS_ELFS` incl. `quickshell-fedora`, `caelestia`, Caelestia `.so` & Quickshell runtime plugin `.so` files), and absence of `usr/local/bin/__pycache__`.
  - **Threshold**: 100% pass required (12 test methods); fails loudly on missing ISO.
- **Tier 2 — Boundary & Corner Cases (`test_tier2_boundaries.py`, gate `BLAZE_RUN_E2E=1`)**:
  - Exercises invalid CLI arguments (`blaze-sentinel`, `blaze-snapshot`, `solar-shell switch`), isolated sandbox `blaze-sentinel check-once --json` tamper fail-closed behavior, `blaze-recovery` read-only modes, RPM payload tamper detection via `rpm -K`, GPG `repomd.xml` tamper rejection, and malformed `niri.kdl` rejection (`niri validate -c`).
  - **Threshold**: 100% pass required (6 test methods); zero host mutations.
- **Tier 3 — Cross-Feature Combinations (`test_tier3_combinations.py`, gate `BLAZE_RUN_E2E=1`)**:
  - Verifies pairwise end-to-end contracts across subsystems inside the built ISO + overlay:
    1. `GRUB -> Initrd/Plymouth -> SDDM -> livesys-solarui -> Niri -> solar-shell (noctalia/caelestia/end4-pC)`
    2. `Live Session -> FireHub -> blaze.repo / DNF5 / GPG`
    3. `Live Session -> liveinst (Anaconda WebUI BlazeOS "solarui","gnome" selector + 99-blazeos.conf BTRFS + polkit)`
    4. `Dynamic Overlay /usr/bin Tools + Launchers/Autostart/Assets`
  - **Threshold**: 100% pass required (4 test methods).
- **Tier 4 — Real-World QEMU E2E Scenarios (`test_tier4_qemu_e2e.py`, gate `BLAZE_RUN_QEMU_E2E=1`)**:
  - Boots target ISO in **both** Legacy BIOS and UEFI OVMF (`/usr/share/edk2/x64/OVMF_CODE.4m.fd`) using KVM + `-device virtio-tablet-pci` + QMP + QGA (`guest-sync` + short ping) + serial/stderr logs + PNG frame captures:
    1. UEFI OVMF POST splash -> GRUB menu transition & Plymouth boot splash frame captures (distinct non-black frames) + in-guest `journalctl -b -u 'plymouth*'` and `/proc/cmdline` validation.
    2. SDDM login bypass -> direct `liveuser` SolarUI Wayland session (`niri` + `solar-shell` + `noctalia`, `gdm` masked/inactive); deterministically awaits (`15s` window) & stops `solar-session-wrapper` auto-launched `liveinst` (`webui-cockpit-ws.service` stopped before and after `pkill`), verifies `/run/anaconda/liveinst.lock` release, and cleans stale `/run/anaconda/anaconda.conf` + `/run/anaconda/bus.address` before capturing clean `03_desktop_{firmware}.png`.
    3. In-guest parity checks (all dynamically discovered overlay `/usr/bin` tools, root GPL-3.0 `LICENSE` & in-guest license file, `blaze-sentinel` `0.3.1`, `blaze.repo` / `dnf5 --cacheonly repolist --enabled` / offline-resilient `dnf5 --cacheonly repoinfo blaze`, `99-blazeos.conf` `BTRFS`, assets/fonts across `/usr/share/fonts/{google-sans-flex,TTF,ttf-material-symbols-variable}`/icons, `FORBIDDEN_LIVE_RPMS` + `gnome-shell-extension-*`/`libreoffice*`/`libvirt*` absent).
    4. `firehub` launch inside live session via `runuser -u liveuser` (>8s survival, log clean, visual `frame_diff_ratio(desktop, firehub) > 0.01`, screenshot captured).
    5. `blaze-house` `ldd` & Wayland smoke-launch via `runuser -u liveuser` (`WEBKIT_DISABLE_DMABUF_RENDERER=1`, >4s survival, log clean).
    6. `liveinst` (Anaconda WebUI) launched inside `liveuser`'s local `logind`/Wayland session via `runuser -u liveuser` + `niri msg action spawn` (`subject.local` true for `20-livesys-solarui.rules`), verified via direct `vm.qga.guest_exec("/usr/bin/pgrep", ["-fa", "[a]naconda|[l]iveinst|[w]ebui-desktop"])` (zero `bash -lc` self-match), browser UI readiness poll (`35s` budget matching `firefox`/`slitherer`/`anaconda-webui/index.html`, ignoring `webui-desktop` wrapper) + `3s` settle + `12s` visual render poll (`frame_diff_ratio(desktop, anaconda) > 0.01`), `~/.local/share/solarui/session.log` + `/tmp/e2e_liveinst.log` failure diagnostics, BlazeOS `"solarui","gnome"` selector + BTRFS default verified, screenshot captured.
    7. Zero coredumps (`coredumpctl list --no-pager --no-legend`) & zero failed systemd units (`systemctl --failed --no-legend --no-pager`).
    8. Post-run read-only reference immutability verification.
  - **Threshold**: 100% pass across both Legacy BIOS and UEFI OVMF runs.
