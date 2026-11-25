# Test Readiness Declaration (`TEST_READY.md`)

## Test Runner
- **Framework**: Python 3 stdlib `unittest` + `socket` + `subprocess` + `json` + `PIL` (zero dependency on `pytest`, `tesseract`, or `socat`).
- **Existing Regression & Unit Suite (fast, host-safe)**:
  ```bash
  python3 -m unittest discover -s /home/darkmorpheus/BlazeFedora/tests -v
  ```
- **Rust Workspace Unit Tests**:
  ```bash
  cargo test --manifest-path /home/darkmorpheus/BlazeFedora/solarui/Cargo.toml --workspace --offline
  ```
- **Tiered E2E Suite Commands**:
  - Dry-run environment check:
    ```bash
    python3 -m tests.e2e.run_e2e --all --dry-run
    ```
  - Tier 1 (Feature Coverage — ISO/initrd/squashfs/RPM/ELF/reference immutability):
    ```bash
    python3 -m tests.e2e.run_e2e --tier 1 -v
    ```
  - Tier 2 (Boundary & Negative Cases — CLI validation, sandboxed Sentinel tamper, RPM/GPG tamper):
    ```bash
    python3 -m tests.e2e.run_e2e --tier 2 -v
    ```
  - Tier 3 (Cross-Feature Pairwise Combinations):
    ```bash
    python3 -m tests.e2e.run_e2e --tier 3 -v
    ```
  - Tier 4 (Full QEMU E2E Boot in Legacy BIOS + UEFI OVMF):
    ```bash
    python3 -m tests.e2e.run_e2e --tier 4 --firmware both -v
    ```
  - Tier 5 (White-Box Adversarial Behavioral, Harness & Contract Suite):
    ```bash
    python3 -m tests.e2e.run_e2e --tier 5 -v
    ```
  - Full Tiers 1–5 Suite:
    ```bash
    python3 -m tests.e2e.run_e2e --all --firmware both -v
    ```

## Coverage Summary
| Tier | Module | Scope | Gating | Current Verification |
|---|---|---|---|---|
| Existing Regressions | `tests/test_audit_regressions.py` | 21 overlay/script/CLI regression tests | Always enabled | Verified (21/21 PASS under `LC_ALL=tr_TR.UTF-8`) |
| Rust Workspace | `solarui/Cargo.toml` (`--workspace --offline`) | 67 Rust unit tests across 7 crates (`blaze-recovery` 3, `blaze-sentinel` 6, `solar-adapter` 1, `solar-common` 19, `solar-core` 6, `solar-niri` 7, `solar-shell` 25) | Always enabled | Verified (67/67 PASS offline) |
| Harness Self-Tests | `tests/e2e/test_harness_unit.py` | 21 unit tests (QMP mock, QGA mock + `guest-sync` partial/error drain & timeout + `_buf` clear, PIL frame diff, `is_anaconda_webui_rendered` & `is_firehub_rendered` positive/negative/textured-desktop/720x400-bounds region oracles, QEMU BIOS/UEFI cmdline + failed-start cleanup, ISO inspect, CLI `--iso`/`--artifacts-dir` propagation & non-zero exit on missing ISO, Tier 4 direct `guest_exec` `pgrep`/`pkill`/`systemctl stop`/post-`rm` mount cleanup ordering & `_has_installer_ui_proc` non-premature matching, Step 6/Step 7 4-gate oracle & `try...finally` `_probe_a.png` exception cleanup + deadline sleep cap, recursive nested `-c` AST+`shlex` `run_shell` self-matching `pkill`/`pgrep` guard, Tier 1 root-confined ELF `.so` symlink & arch resolution) | Always enabled | Verified (21/21 PASS under `LC_ALL=tr_TR.UTF-8`) |
| Tier 1 | `tests/e2e/test_tier1_features.py` | 12 test methods covering ISO SHA-256/`checkisomd5`, ref immutability, R1 fixes (path-boundary regex + dynamic overlay `/usr/bin` tools), signed RPMs + bounded-timeout GPG, GRUB/EFI, initrd & rootfs Plymouth + `blazeos-apply-logos`, `work_f45/` font parity (`google-sans-flex`, `TTF`, `ttf-material-symbols-variable` `.ttf`) + icons/wallpapers, `livesys=solarui` + `etc/pam.d/sddm-autologin` (`pam_selinux.so`, no `pam_succeed_if.so`), GNOME purge (`--dbpath /usr/lib/sysimage/rpm`, `>1000` pkgs) + `/usr/bin/niri` executable binary, `00-blazeos.preset` + `.wants` (`graphical.target.wants/blazeos-nvidia-firstboot.service`), Feature 14 (`liveinst`, `anaconda-webui` BlazeOS `"solarui","gnome"` selector, `pyanaconda` `BlazeFinalizeInstallationTask` / `45.30-1` constants), scoped ELF `NEEDED` (`CRITICAL_SQUASHFS_ELFS` + `quickshell-fedora`, `caelestia`, Caelestia & Quickshell plugins, `etc/alternatives` root-confined symlink resolution + `x86_64` `ELFCLASS64` separation of `usr/lib64` vs `usr/lib`), `etc/skel/.config/autostart/blazeos-welcome.desktop`, no `usr/local/bin/__pycache__` | `BLAZE_RUN_E2E=1` / `--tier 1` | Verified (12/12 PASS against rebuilt `Blaze-SolarEvolution-5-x86_64.iso` under `LC_ALL=tr_TR.UTF-8`) |
| Tier 2 | `tests/e2e/test_tier2_boundaries.py` | 6 test methods covering invalid CLI args, sandboxed `blaze-sentinel` tamper detection, `blaze-recovery`, `blaze-snapshot` rejection, `solar-shell`/`niri` invalid inputs, RPM/GPG/ISO tamper rejection | `BLAZE_RUN_E2E=1` / `--tier 2` | Verified (6/6 PASS under `LC_ALL=tr_TR.UTF-8`) |
| Tier 3 | `tests/e2e/test_tier3_combinations.py` | 4 pairwise integration tests covering Boot->Plymouth->SDDM->`livesys-solarui`->Niri->Shells, Live->FireHub->`blaze.repo`/DNF5, Live->`liveinst`->BTRFS+BlazeOS `"solarui","gnome"` selector, and Dynamic Overlay Tools+Launchers+Assets | `BLAZE_RUN_E2E=1` / `--tier 3` | Verified (4/4 PASS under `LC_ALL=tr_TR.UTF-8`) |
| Tier 4 | `tests/e2e/test_tier4_qemu_e2e.py` | 2 full QEMU E2E boot tests in BOTH Legacy BIOS & UEFI OVMF: OVMF->GRUB->Plymouth transitions + `journalctl -b`, SDDM bypass -> `liveuser` SolarUI + SELinux `Enforcing`/`unconfined_t` + `solarui-session.service`, deterministic `solar-session-wrapper` auto-launched `liveinst` cleanup (`15s` trigger window, `webui-cockpit-ws.service` stopped before/after `pkill`, `/run/anaconda/liveinst.lock` release, stale `/run/anaconda/{anaconda.conf,bus.address}` & `/run/install/sources` cleanup) before `03_desktop`, dynamic `/usr/bin` parity, `LICENSE`, `dnf5 --cacheonly repolist --enabled` + offline-resilient `dnf5 --cacheonly repoinfo blaze`, all 3 font dirs, `FORBIDDEN_LIVE_RPMS`, FireHub & `blaze-house` via `runuser -u liveuser` + `is_firehub_rendered` + `frame_diff_ratio`, `liveinst` spawned inside `liveuser` session via `runuser -u liveuser` + `niri msg action spawn` (`subject.local` true) + 4-gate Step 7 oracle (`50s` Gate 1 `/run/anaconda/backend_ready` + Cockpit HTTP 200 + `firefox`/`slitherer` check, `25s` Gate 2 Niri focused floating `>=900x550` unobstructed check, `25s` Gates 3–4 tightened `is_anaconda_webui_rendered` + `<0.02` 2-frame settle + `try...finally` `_probe_a.png` cleanup + capped sleep) + `session.log`/`e2e_liveinst.log` diagnostics + BlazeOS selector, zero coredumps/failed units, post-run ref immutability | `BLAZE_RUN_QEMU_E2E=1` / `--tier 4` | Awaiting live QEMU re-run (`--tier 4 --firmware both` / `--all --firmware both`) against Tier 5 rebuilt `Blaze-SolarEvolution-5-x86_64.iso` |
| Tier 5 | `tests/e2e/test_tier5_adversarial.py` | 30 white-box adversarial behavioral, harness, and contract tests (17 shell/system/locale/packaging/bug-regression tests in `Tier5ShellAndSystemAdversarialTests` + 13 `niri.kdl`/imaging/QGA/QMP/QemuVM/CLI/Rust-CLI tests in `Tier5AdversarialTests`) | `BLAZE_RUN_E2E=1` / `--tier 5` | Verified (30/30 PASS under `LC_ALL=tr_TR.UTF-8`) |

- **Suite Totals**: **75** E2E tests (`21` unit + `12` Tier 1 + `6` Tier 2 + `4` Tier 3 + `2` Tier 4 + `30` Tier 5) + `21` existing regressions + `67` Rust workspace tests = **163** total tests (`161/161` non-QEMU checks: `73/73` unit+Tiers 1–3+5 + `21/21` regressions + `67/67` Rust).

## Feature Checklist
- [x] Feature 1 — Patched Initrd Plymouth (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 2 — Rootfs Plymouth & Logos (`test_tier1_features.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 3 — GRUB2 & EFI Stub Parity (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 4 — Fonts & SolarUI Icons (`test_tier1_features.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 5 — R1 Prior Fixes Preservation (`test_tier1_features.py`, `test_tier2_boundaries.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 6 — Noctalia Shared Libs & Assets (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 7 — Caelestia & Quickshell Plugins (`test_tier1_features.py`, `test_tier2_boundaries.py`, `test_tier3_combinations.py`)
- [x] Feature 8 — FireHub & BlazeHouse Runtime (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 9 — Custom Tools & Launchers Parity (`test_tier1_features.py`, `test_tier2_boundaries.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 10 — Packaging & DNF5 Repo Sync (`test_tier1_features.py`, `test_tier2_boundaries.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 11 — Direct SolarUI Live Autologin (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 12 — GNOME & Workstation Cleanup (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 13 — Systemd Preset & Service Parity (`test_tier1_features.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 14 — Anaconda 45.30-1 / WebUI Parity (`test_tier1_features.py`, `test_tier3_combinations.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 15 — ISO Rebuild & Ref Protection (`test_tier1_features.py`, `test_tier2_boundaries.py`, `test_tier4_qemu_e2e.py`)
- [x] Feature 16 — Opaque-Box E2E Test Suite (`TEST_INFRA.md`, `tests/e2e/`, `TEST_READY.md`)
- [ ] Feature 17 — Final E2E Pass & Tier 5 Hardening (Tier 5 product/harness fixes applied, 30 Tier 5 tests integrated, ISO rebuilt; awaiting live QEMU Tier 4 gate on rebuilt ISO)
