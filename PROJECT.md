# Project: BlazeOS / SolarEvolution 5.05 (Fedora 45 Workstation Stable 1.4 Parity & Release)

## Architecture
- **Base OS**: Fedora 45 Workstation Live 1.4 (`f45_base/Fedora-Workstation-Live-45-1.4.x86_64.iso`, unpacked to `work_f45_stable/`)
- **Reference Baseline (READ-ONLY)**: Healthy Beta 1.3 (`work_f45/` and `Blaze-SolarEvolution-5-x86_64(2026-11-11).iso`) — NEVER modify.
- **Custom Overlay & Sources**:
  - `solarui/`: 7-crate Rust workspace (`solar-common`, `solar-niri`, `solar-adapter`, `solar-core`, `solar-shell`, `solar-session`, `blaze-recovery`), `vendor/` (`end4-pC`, `noctalia`, `caelestia-*`), `data/noctalia-assets/`
  - `blazeos_custom_apps/`: Rootfs overlay containing 14 moved `/usr/bin/blaze*`/`solar-torture` tools, `opt/FireHub/`, `usr/bin/blaze-house`, Plymouth `blazeos` theme, wallpapers/assets, Anaconda `99-blazeos.conf` + WebUI overrides, systemd units & `00-blazeos.preset`, `/usr/share/dnf5/repos.d/blaze.repo` + GPG key
  - `packaging/`: 8 EdDSA/SHA512-signed RPMs + `repodata/` + `RPM-GPG-KEY-blaze` + `blaze.repo`
  - `build_f45_final.sh`: Rootless (`unshare -rm`) tmpfs staging + SELinux labeling + `mksquashfs` + `xorriso` + `implantisomd5` pipeline producing `/home/darkmorpheus/BlazeFedora/Blaze-SolarEvolution-5-x86_64.iso` and `.iso.sha256`
- **Data / Boot Flow**:
  1. UEFI (`OVMF`) / Legacy BIOS -> GRUB2 (`iso_mods/boot/grub2/grub.cfg` + `CDLABEL=Blaze-SE-5`) -> Linux kernel + patched `initrd` (Plymouth `Theme=blazeos`, `script.so`)
  2. Switch-root into `LiveOS/squashfs.img` -> `systemd` (`00-blazeos.preset`, Workstation GNOME/telemetry/virt trimmed) -> `livesys.service` (`livesys_session="solarui"`) -> `livesys-solarui` removes `00-blaze-oobe.conf`, configures `autologin.conf` (`liveuser` -> `solarui.desktop`), places `liveinst.desktop` on Desktop, sets polkit `20-livesys-solarui.rules`
  3. `sddm.service` auto-logins `liveuser` directly into `solar-session-wrapper` -> `solar-session` -> `niri` + `solar-core` + `solar-shell` (`noctalia` default, switchable to `caelestia`/`end4-pC`)
  4. User/E2E launches FireHub (`/usr/bin/firehub`), custom tools (`/usr/bin/*`), and Anaconda (`liveinst` -> `anaconda-webui` offering GNOME option + `99-blazeos.conf` `default_scheme = BTRFS`).

## Feature Inventory
Every feature from the Step 0 Survey and `ORIGINAL_REQUEST.md` is assigned to a milestone below:

| # | Feature | Description | Milestone | Source |
|---|---------|-------------|-----------|--------|
| 1 | Patched Initrd Plymouth | Reproducible initrd patch (`work_f45_stable/initrd_work/patched_initrd`) with `etc/plymouth/plymouthd.conf` (`Theme=blazeos`), `script.so`, `blazeos` theme, `spinner/watermark.png` | M1 | survey_1 |
| 2 | Rootfs Plymouth & Logos | `etc/plymouth/plymouthd.conf`, `usr/lib64/plymouth/script.so`, `spinner/watermark.png`, and `blazeos-apply-logos` branding parity | M1 | survey_1, survey_2 |
| 3 | GRUB2 & EFI Stub Parity | Verify `work_f45_stable/iso_mods/boot/grub2/grub.cfg` (6 entries incl. BERP/BERE, `0x5b67ceb6` marker, `CDLABEL=Blaze-SE-5`) & EFI stub match `work_f45/` | M1 | survey_1 |
| 4 | Fonts & SolarUI Icons | Restore `google-sans-flex`, `TTF`, `ttf-material-symbols-variable` fonts (matching `work_f45/`) and `usr/share/icons/hicolor/*/apps/solarui.{png,svg}` + wallpapers parity | M1 | survey_1, survey_2 |
| 5 | R1 Prior Fixes Preservation | Preserve root `LICENSE` (GPL-3.0), cleaned `solarui/crates/` paths, `sentinel.rs` v0.3.1 SHA-256 fail-closed canary, 14 tools in `blazeos_custom_apps/usr/bin/`, 8 signed RPMs + `blaze.repo`, `99-blazeos.conf` (`BTRFS`) | M1 | ORIGINAL_REQUEST, survey_2 |
| 6 | Noctalia Shared Libs & Assets | Resolve 5 missing NEEDED libs (`libqalculate.so.23`, `libical.so.4.0`, `libtomlplusplus.so.3`, `libjxl.so.0.12`, `libjxl_threads.so.0.12` + transitive deps) & sync full `noctalia-assets` | M1 | survey_2 |
| 7 | Caelestia & Quickshell Plugins | Resolve `libqalculate.so.23`, `libsensors.so.5`, `libaubio.so.5`, `libcava.so.1` & `usr/lib64/quickshell/plugins/` parity (plus `end4-pC` integrity) | M1 | survey_2 |
| 8 | FireHub & BlazeHouse Runtime | Verify `opt/FireHub/` (`chrome-sandbox` 4755, ELF deps) & `blaze-house` (`webkit2gtk-4.1`) launch cleanly in Live OS | M1 | survey_2 |
| 9 | Custom Tools & Launchers Parity | Fix `autostart/blazeos-welcome.desktop` (`/usr/bin/blazeos-welcome`), clean stale `usr/local/bin/__pycache__`, restore 5 missing `.desktop` launchers, restore `fastfetch`, `yad`, `alacritty` symlink, `niri-session` | M1 | survey_2 |
| 10 | Packaging & DNF5 Repo Sync | Verify 8 signed RPMs, `RPM-GPG-KEY-blaze`, `/usr/share/dnf5/repos.d/blaze.repo`, and align `packaging/verify-repo.sh` path | M1 | survey_2 |
| 11 | Direct SolarUI Live Autologin | Add `etc/sysconfig/livesys` (`livesys_session="solarui"`) so `livesys-solarui` configures `autologin.conf` (`liveuser` -> `solarui.desktop`), removes `00-blaze-oobe.conf`, sets up `liveinst.desktop` & polkit | M2 | survey_1 |
| 12 | GNOME & Workstation Cleanup | Fix `build_f45_final.sh:204` regex (`gnome-shell($|-extension-.*)`) to purge all 6 `gnome-shell-extension-*` RPMs while keeping GNOME available for Anaconda install | M2 | survey_1 |
| 13 | Systemd Preset & Service Parity | Apply `00-blazeos.preset`, remove `audit-rules`, broken `virt*`, `iscsi*`, `sssd-kcm`, `systemd-journalctl-metrics`, `systemd-journald-audit`, `systemd-report*`; enable `blaze-sentinel`, `blaze-boot-success`, `blazeos-plymouth-failsafe`, `blazeos-nvidia-firstboot`, `blaze-optimization` (`/usr/bin/blaze-optimize`) | M2 | survey_1 |
| 14 | Anaconda 45.30-1 / WebUI Parity | Verify `99-blazeos.conf` (`default_scheme = BTRFS`), `BlazeFinalizeInstallationTask` (`installation.py` vs `45.30-1` `CATEGORY_FINALIZATION`), `anaconda-webui` (`90-1`) GNOME desktop option | M2 | survey_1 |
| 15 | ISO Rebuild & Ref Protection | Terminate stale `blaze-f45-stable-test`, run `build_f45_final.sh` to build `Blaze-SolarEvolution-5-x86_64.iso` + `.iso.sha256` + `checkisomd5`, prove `work_f45/` & `(2026-11-11).iso` untouched | M2 | survey_1, survey_3 |
| 16 | Opaque-Box E2E Test Suite | Requirement-driven Tier 1–4 test suite (BIOS + UEFI QEMU via QMP+QGA+screenshots) covering all features, publishing `TEST_READY.md` | E2E_Track | survey_3 |
| 17 | Final E2E Pass & Tier 5 Hardening | Pass existing `tests/` + 100% of Tier 1–4 E2E suite in QEMU (Phase 1) and run Tier 5 adversarial white-box coverage hardening (Phase 2) | M3 | survey_3 |

## Milestones
| # | Name | Scope | Dependencies | Status |
|---|------|-------|--------------|--------|
| M1 | Initrd, Plymouth, SolarUI Libs, Assets & Tools Parity | Patch `work_f45_stable/` initrd, add rootfs Plymouth config/`script.so`/`watermark.png`, restore fonts/`solarui` icons, resolve Noctalia & Caelestia shared libs/plugins & `noctalia-assets`, fix autostart/desktop launchers & missing CLI tools (`fastfetch`, `yad`, etc.), verify R1 fixes + FireHub/BlazeHouse/packaging | none | DONE |
| E2E_Track | Opaque-Box E2E Testing Track | Design & implement `TEST_INFRA.md` and Tier 1–4 QEMU (BIOS + UEFI) & parity test suite under `tests/e2e/`, publish `TEST_READY.md` | none | DONE |
| M2 | Build Integration, Live Session, Cleanup, Anaconda & ISO Build | Configure `livesys_session="solarui"`, fix GNOME extension regex, apply `00-blazeos.preset` + `blaze-optimization.service`, verify Anaconda 45.30-1 compatibility, integrate M1 into `build_f45_final.sh`, kill stale QEMU VM, build `Blaze-SolarEvolution-5-x86_64.iso` + `.sha256` | M1 | DONE |
| M3 | Final Milestone: E2E Test Pass & Adversarial Coverage Hardening | Phase 1: Pass 100% of E2E Tiers 1–4 + existing `tests/` + `cargo test`; Phase 2: Tier 5 white-box adversarial coverage hardening & bug fixes (`Blaze-SolarEvolution-5-x86_64.iso` `8e9f00ac...`, `163/163` verified) | M2, E2E_Track | DONE |

## Interface Contracts
### M1 ↔ M2 (`build_f45_final.sh`, Initrd & Overlay)
- M1 produces `/home/darkmorpheus/BlazeFedora/work_f45_stable/initrd_work/patched_initrd` (via `/home/darkmorpheus/BlazeFedora/scripts/patch_f45_initrd.sh`), populates `blazeos_custom_apps/` with Plymouth config/`script.so`/`watermark.png`, fonts, `solarui` icons, Noctalia/Caelestia missing libs & `quickshell/plugins/`, full `noctalia-assets/`, fixed `.desktop` launchers & `etc/skel/` autostart, and missing CLI tools (`fastfetch`, `yad`, `alacritty`, `niri-session`).
- M2 updates `build_f45_final.sh` (`PATCHED_INITRD`, `gnome-shell($|-extension-.*)`, `00-blazeos.preset`, `blazeos-apply-logos`, `ldconfig -r`, SELinux labeling), configures `livesys_session="solarui"`, `blaze-optimization.service`, and Anaconda 45.30-1 parity, kills stale `blaze-f45-stable-test`, and builds `Blaze-SolarEvolution-5-x86_64.iso` + `.iso.sha256`.

### E2E_Track ↔ M3 (`TEST_READY.md`)
- E2E_Track derives tests purely from `ORIGINAL_REQUEST.md` and `PROJECT.md § Feature Inventory`, targeting `/home/darkmorpheus/BlazeFedora/Blaze-SolarEvolution-5-x86_64.iso` (plus read-only hash check on `work_f45/` and `(2026-11-11).iso`), using QEMU + KVM + OVMF (`/usr/share/edk2/x64/OVMF_CODE.4m.fd`) + QMP + QGA + `PIL`/`ffmpeg`.
- E2E_Track publishes `TEST_READY.md` at project root with exact runner commands and coverage summary.

## Code Layout (Exclusive Write Ownership)
- **NEVER MODIFY (READ-ONLY)**: `/home/darkmorpheus/BlazeFedora/work_f45/**`, `/home/darkmorpheus/BlazeFedora/Blaze-SolarEvolution-5-x86_64(2026-11-11).iso`, `/home/darkmorpheus/BlazeFedora/f45_base/**`
- **M1 Exclusive**: `work_f45_stable/initrd_work/`, `scripts/patch_f45_initrd.sh`, `blazeos_custom_apps/etc/plymouth/`, `blazeos_custom_apps/usr/lib64/`, `blazeos_custom_apps/usr/share/plymouth/`, `blazeos_custom_apps/usr/share/icons/hicolor/`, `blazeos_custom_apps/usr/share/fonts/`, `blazeos_custom_apps/usr/share/pixmaps/`, `blazeos_custom_apps/usr/share/glib-2.0/schemas/yad.gschema.xml`, `blazeos_custom_apps/usr/lib/systemd/user/`, `blazeos_custom_apps/usr/share/solarui/noctalia-assets/`, `blazeos_custom_apps/usr/share/applications/`, `blazeos_custom_apps/etc/skel/`, `blazeos_custom_apps/usr/local/bin/`, `blazeos_custom_apps/usr/bin/` (additive CLI tools/symlinks only — preserve 14 R1 tools), `packaging/verify-repo.sh`
- **M2 Exclusive**: `blazeos_custom_apps/etc/sysconfig/livesys`, `blazeos_custom_apps/etc/systemd/system/blaze-optimization.service`, `blazeos_custom_apps/etc/systemd/system-preset/00-blazeos.preset`, `blazeos_custom_apps/usr/share/cockpit/anaconda-webui/`, `blazeos_custom_apps/usr/lib64/python3.15/site-packages/pyanaconda/`, `build_f45_final.sh`, `work_f45_stable/LiveOS/squashfs.img`, `Blaze-SolarEvolution-5-x86_64.iso`, `Blaze-SolarEvolution-5-x86_64.iso.sha256`
- **E2E_Track Exclusive**: `TEST_INFRA.md`, `TEST_READY.md`, `tests/e2e/`
- **M3 Exclusive**: Fixes only after M2 completes, guided by E2E failures & Tier 5 Challenger findings.
