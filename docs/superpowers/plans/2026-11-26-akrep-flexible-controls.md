# Akrep Esnek Koruma Kontrolleri ve Uyumluluk Yardımcıları Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add timed (`15m`/`1h`/`UntilReboot`) and explicit persistent (`UntilChanged`) manual relaxation controls for SELinux (`Enforcing` <-> `Permissive`), Firewalld, and `systemd-oomd`, plus real Fedora gaming/Android-ROM compatibility helpers (`nofile 65536/524288`, container directory `container_file_t` labeling + `restorecon`, `android-udev-rules`, `ncurses-compat-libs`, Flatpak `devices=input` + disk presets, Firewalld LAN/dev templates, and Secure Boot/NVIDIA MOK status) to BlazeOS Akrep Security Center while preserving `akrepd` read-only isolation, `akrep-privd` 0 MiB idle exit, Polkit preview/rollback transactions, and the Graphite + Blaze Ember UI spec.

**Architecture:** Extend `blaze-sentinel-core` (`AkrepSnapshot` with backward-compatible `#[serde(default)]` fields) and `blaze-akrep-common` (6 typed `ActionRequest` variants + strict validators). Implement an `OverrideStore` and `OverrideOps` trait inside `blaze-akrep-privd` backed by PID 1 transient `systemd-run` timers (`akrep-revert-<subsys>.timer`) invoking `akrep-privd --revert-override <subsys> --generation <uuid>` plus a world-readable cache (`/var/cache/akrep/active_overrides.json`, `0644`) read by unprivileged `akrepd` alongside 15s TTL-cached read-only compatibility/MOK probes. Surface all controls cleanly inside Akrep's existing 10 GTK4 pages (`nav_count == 10`) using the approved unslop design tokens.

**Tech Stack:** Rust (`tokio`, `zbus`, `serde`, `gtk4`), systemd transient timers (`systemd-run`), Polkit XML, Fedora 45 RPM/DNF5 (`build-rpms.sh`), QEMU/KVM (`tests.e2e.run_e2e` + live UEFI visual check).

**Spec:** [2026-11-26-akrep-flexible-controls-design.md](../specs/2026-11-26-akrep-flexible-controls-design.md)

## Global Constraints

- `AKREP_VERSION = "1.0.0"`, `AKREP_SCHEMA_VERSION = 2`, `nav_count = 10` (no new pages, no schema break).
- `SELINUX=disabled`, `audit2allow`, `semodule`, global Flatpak overrides (`global`), blanket `filesystem=host`/`/`/`/etc`, `devices=all`, and sandbox-escape D-Bus policies remain strictly forbidden.
- `blaze-akrep-daemon` (`akrepd`) remains 100% read-only (`User=akrep`, `ProtectSystem=strict`, zero `gtk4` deps, zero mutator methods, `<250ms` snapshot budget, never wakes `akrep-privd` during browsing).
- `blaze-akrep-privd` keeps `--idle-timeout-secs 60` (`0 MiB RSS` at idle) and requires `CreatePlan` -> `present_plan_preview_dialog` -> `CommitPlan` + Polkit (`auth_admin_keep`) for all privileged changes.
- UI follows [2026-11-26-blaze-ui-unslop-design.md](../specs/2026-11-26-blaze-ui-unslop-design.md): Graphite `#0f0f10/#141415/#19191b/#232325`, Blaze ember `#c83b22/#e0573c`, 4px grid, 8px status dots, 16px `*-symbolic` icons, Turkish sentence case, zero `(Polkit)` button text, zero emojis, zero `/home/<user>` developer paths, zero clipping at `1060x720`.
- Offline Cargo (`--locked --offline`), stripped release binaries (`--remap-path-prefix=${HOME}=/build`), EdDSA-signed RPMs, and mandatory QEMU Legacy BIOS + UEFI OVMF (`run_e2e --all --firmware both`) + visual screenshot verification (`AGENTS.md`).

## Review Focus

1. **Stale `Timed`/`UntilReboot` entry after reboot:** `active_overrides.json` holds an old `boot_id` -> `akrepd` ignores it and reports real runtime posture (pinned in Task 4).
2. **Stale timer firing after user switches `Timed` -> `UntilChanged`:** `--revert-override <subsys> --generation <old-uuid>` observes generation mismatch and exits cleanly as a no-op without reverting `UntilChanged` (pinned in Task 2).
3. **Switching `UntilChanged` -> `Timed`/`UntilReboot`:** Immediately reconciles `/etc/selinux/config` to `SELINUX=enforcing` and re-enables `firewalld`/`systemd-oomd` units so reboot or timer expiry restores protection (pinned in Task 2).
4. **AOSP 500k+ file tree mutations before rollback:** `SelinuxLabelContainerDir` fingerprints only the canonical root directory context so child-file changes during builds never cause `ExternalDriftDetected`, and rollback executes `restorecon -h [-R]` without following symlinks (pinned in Task 2 & Task 3).
5. **`nofile` hard limit floor & `/home/<user>` root protection:** `DevLimitsSetNofile` UI preset uses `(65536, 524288)` so Fedora's `524288` hard limit never drops, and `SelinuxLabelContainerDir` rejects `/home/<user>` root, `/etc`, `/`, symlinks, and `..` (pinned in Task 1 & Task 2).

---

### Task 1: Shared Models, Validation Red Lines & Polkit Action

**Files:**
- Modify: `solarui/crates/blaze-sentinel-core/src/models.rs`
- Modify: `solarui/crates/blaze-akrep-common/src/models.rs`
- Modify: `solarui/crates/blaze-akrep-common/src/lib.rs` (if re-exports needed)
- Modify: `blazeos_custom_apps/usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy`

**Interfaces:**
- Produces in `blaze_sentinel_core`:
  - `pub struct ActiveOverrideEntry { pub subsystem: String, pub duration_kind: String, pub minutes: Option<u32>, pub started_at: String, pub expires_at: Option<String>, pub remaining_secs: Option<i64>, pub boot_id: String }`
  - `pub struct SystemCompatStatus { pub oomd_active: bool, pub oomd_enabled: bool, pub nofile_enabled: bool, pub nofile_soft: u32, pub nofile_hard: u32, pub android_udev_installed: bool, pub ncurses_compat_installed: bool }`
  - `pub struct DriverMokSummary { pub secure_boot_enabled: Option<bool>, pub mok_enrolled_count: Option<usize>, pub nvidia_module_loaded: bool, pub akmod_nvidia_installed: bool, pub unsigned_risk: bool, pub guidance_tr: String, pub guidance_en: String }`
  - `AkrepSnapshot` additions (`#[serde(default)]`): `pub active_overrides: Vec<ActiveOverrideEntry>`, `pub system_compat: Option<SystemCompatStatus>`, `pub driver_mok: Option<DriverMokSummary>`
- Produces in `blaze_akrep_common`:
  - `pub enum OverrideSubsystem { Selinux, Firewalld, Oomd }` (with `as_str(&self) -> &'static str`, `from_str_id(s: &str) -> Option<Self>`)
  - `pub enum OverrideDuration { Timed { minutes: u32 }, UntilReboot, UntilChanged }`
  - `pub enum SelinuxDesiredMode { Enforcing, Permissive }`
  - `pub enum CompatPackageGroup { AndroidUdevRules, AndroidLegacyLibs }` (with `rpm_packages(&self) -> &'static [&'static str]`)
  - `ActionRequest` variants: `SelinuxSetMode`, `FirewallSetState`, `OomdSetState`, `DevLimitsSetNofile`, `SelinuxLabelContainerDir`, `InstallCompatibilityPackage`
  - `pub const POLKIT_SYSTEM_COMPAT_MANAGE: &str = "tr.com.blazeos.Akrep1.system.compat-manage";`
  - `pub fn validate_container_dir_path(path: &str) -> Result<String, AkrepError>`

- [ ] **Step 1: Write failing unit tests in `blaze-akrep-common/src/models.rs` and `blaze-sentinel-core/src/models.rs`**
  - `test_new_flexible_action_requests_validation_and_polkit`: asserts `SelinuxSetMode`, `FirewallSetState`, `OomdSetState`, `DevLimitsSetNofile` `(65536, 524288)`, `SelinuxLabelContainerDir` (`"/home/alice/android"`), and `InstallCompatibilityPackage` pass `validate_red_lines()`, return exact Polkit actions (`selinux.manage`, `firewall.manage`, `system.compat-manage`, `packages.install-optional`) and risk levels (`Low`/`Medium`/`High` for `UntilChanged`).
  - `test_flexible_action_red_lines_rejected`: asserts `Timed { minutes: 0 }`, `Timed { minutes: 1441 }`, `DevLimitsSetNofile { enabled: true, soft_limit: 500, hard_limit: 65536 }`, `soft > hard`, `hard > 1_048_576`, and `SelinuxLabelContainerDir` on `"/"`, `"/home"`, `"/home/alice"`, `"/etc/foo"`, `"/home/alice/../etc/shadow"`, `"/mnt"`, `"/run/media/alice"` are all rejected with `InvalidInput` or `ForbiddenOperation`.
  - `test_akrep_snapshot_deserializes_legacy_json_without_new_fields`: verifies backward-compatible `#[serde(default)]` deserialization of schema v2 JSON lacking `active_overrides`/`system_compat`/`driver_mok`.

- [ ] **Step 2: Run tests to verify failure**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-sentinel-core -p blaze-akrep-common`
  - Expected: FAIL (undefined types/variants).

- [ ] **Step 3: Implement shared structs, enums, validators, Polkit mappings, and Polkit XML action**
  - Add `ActiveOverrideEntry`, `SystemCompatStatus`, `DriverMokSummary` and `#[serde(default)]` fields on `AkrepSnapshot` in `solarui/crates/blaze-sentinel-core/src/models.rs` (re-export in `lib.rs`).
  - Add enums, 6 `ActionRequest` variants, `validate_container_dir_path`, `validate_red_lines()`, `polkit_action()`, and `risk_level()` arms in `solarui/crates/blaze-akrep-common/src/models.rs` (keeping `SELINUX=disabled`, `audit2allow`, `semodule`, global Flatpak, `filesystem=host`, `devices=all` strictly blocked).
  - Add `tr.com.blazeos.Akrep1.system.compat-manage` (`auth_admin` / `auth_admin` / `auth_admin_keep`) to `blazeos_custom_apps/usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy`.

- [ ] **Step 4: Run tests & clippy**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-sentinel-core -p blaze-akrep-common && cargo clippy --manifest-path solarui/Cargo.toml --locked --offline -p blaze-sentinel-core -p blaze-akrep-common --all-targets -- -D warnings`
  - Expected: PASS (0 warnings).

- [ ] **Step 5: Commit**
  - `git add solarui/crates/blaze-sentinel-core/ solarui/crates/blaze-akrep-common/ blazeos_custom_apps/usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy && git commit -m "feat(akrep-common): add flexible override and compatibility action models and Polkit policy"`

---

### Task 2: Override Store, Transient Systemd Timer Manager, Compatibility Adapter & CLI Revert

**Files:**
- Create: `solarui/crates/blaze-akrep-privd/src/adapters/overrides.rs`
- Modify: `solarui/crates/blaze-akrep-privd/src/adapters/mod.rs`
- Modify: `solarui/crates/blaze-akrep-privd/src/main.rs`

**Interfaces:**
- Consumes: Task 1 types (`OverrideSubsystem`, `OverrideDuration`, `SelinuxDesiredMode`, `CompatPackageGroup`, `validate_container_dir_path`) and existing `AuditLogger` / `atomic_write`.
- Produces in `blaze_akrep_privd::adapters::overrides`:
  - `pub const DEFAULT_PUBLIC_OVERRIDES_PATH: &str = "/var/cache/akrep/active_overrides.json";`
  - `pub trait OverrideOps: Send + Sync` (abstracting `setenforce`, `/etc/selinux/config` read/write, `systemctl is-active/is-enabled/start/stop/enable/disable/daemon-reload`, `systemd-run` timer schedule/cancel, `nofile` drop-in read/write/remove, `chcon`/`restorecon`/`getfilecon`, `dnf5 install` + `udevadm`, `read_boot_id`)
  - `pub struct LinuxOverrideOps` (production implementation; never uses `shell=True`; atomic temp+fsync+rename for `/etc/selinux/config`, drop-ins, and JSON stores; `chcon --no-dereference [-R] -t container_file_t` with 60s timeout)
  - `pub struct OverrideAdapter` with methods:
    - `preview_selinux_mode`, `apply_selinux_mode`, `rollback_selinux_mode`
    - `preview_firewall_state`, `apply_firewall_state`, `rollback_firewall_state`
    - `preview_oomd_state`, `apply_oomd_state`, `rollback_oomd_state`
    - `preview_nofile_limits`, `apply_nofile_limits`, `rollback_nofile_limits`
    - `preview_container_dir_label`, `apply_container_dir_label`, `rollback_container_dir_label`
    - `preview_compat_package`, `apply_compat_package`
    - `force_restore_for_profile(&self, restore_selinux: bool, restore_firewalld: bool) -> Result<ProfileOverridePreState, AkrepError>` & `rollback_profile_override_restore(&self, pre: &ProfileOverridePreState) -> Result<(), AkrepError>`
    - `handle_timer_revert(&self, subsystem: OverrideSubsystem, generation_id: &str) -> Result<bool, AkrepError>` (returns `false` no-op on generation mismatch)
  - CLI in `main.rs`: `akrep-privd --revert-override <selinux|firewalld|oomd> --generation <uuid> [--state-dir <PATH>]`

- [ ] **Step 1: Write failing unit tests in `adapters/overrides.rs` using a `FakeOverrideOps` mock + `tempfile::TempDir`**
  - `test_selinux_mode_timed_schedules_timer_writes_cache_and_reverts`: asserts `Permissive + Timed(15)` calls `setenforce(false)`, preserves `/etc/selinux/config` `SELINUX=enforcing`, schedules `akrep-revert-selinux` timer with `900s`, writes `/var/lib/akrep/overrides.json` (`0600`) and public cache (`0644`), and `handle_timer_revert` with matching generation restores `Enforcing` and clears cache.
  - `test_timer_revert_ignores_stale_generation_after_until_changed`: asserts switching `Timed(15)` -> `UntilChanged` writes `SELINUX=permissive` (preserving comments & `SELINUXTYPE=targeted`, NEVER `SELINUX=disabled`), cancels old timer, and calling `handle_timer_revert` with the old generation UUID returns `Ok(false)` and leaves SELinux `Permissive` intact!
  - `test_switching_until_changed_back_to_timed_reconciles_persistent_config`: asserts switching SELinux `UntilChanged` -> `Timed(15)` (and Firewalld `UntilChanged` -> `UntilReboot`) immediately rewrites `/etc/selinux/config` back to `SELINUX=enforcing` / calls `systemctl enable firewalld.service` while keeping runtime paused!
  - `test_nofile_limits_apply_and_rollback_preserves_524288_hard_ceiling`: asserts drop-ins contain `* soft nofile 65536`, `* hard nofile 524288`, `DefaultLimitNOFILE=65536:524288`, `daemon-reload` runs, and rollback removes both files cleanly.
  - `test_container_dir_label_roots_only_fingerprint_and_symlink_rejection`: asserts `chcon --no-dereference -R -t container_file_t` runs, symlink targets fail closed, `post_fingerprints` has exactly 1 entry (canonical root dir context, NOT children), and rollback calls `restorecon -h -R` cleanly even after new files are added inside the directory.
  - `test_oomd_pause_resume_and_compat_package_install`: asserts `systemd-oomd` timed/until-changed/resume + rollback and `AndroidUdevRules` (`dnf5` + `udevadm reload/trigger`) / `AndroidLegacyLibs` apply cleanly.

- [ ] **Step 2: Run tests to verify failure**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-privd overrides`
  - Expected: FAIL (module not found).

- [ ] **Step 3: Implement `adapters/overrides.rs`, export in `adapters/mod.rs`, and wire `--revert-override` in `main.rs`**
  - Implement atomic `/etc/selinux/config` line updater (preserving all comments, blank lines, and `SELINUXTYPE=`, strictly only ever writing `SELINUX=enforcing` or `SELINUX=permissive`).
  - Wire `--revert-override <subsys> --generation <uuid>` in `main.rs` so it executes synchronously without grabbing the D-Bus bus name, logs `override_auto_reverted` to `/var/lib/akrep/audit.log`, and exits 0.

- [ ] **Step 4: Run tests & clippy**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-privd overrides && cargo clippy --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-privd --lib --bins -- -D warnings`
  - Expected: PASS (0 warnings on lib/bins).

- [ ] **Step 5: Commit**
  - `git add solarui/crates/blaze-akrep-privd/src/adapters/overrides.rs solarui/crates/blaze-akrep-privd/src/adapters/mod.rs solarui/crates/blaze-akrep-privd/src/main.rs && git commit -m "feat(akrep-privd): implement override store, transient timer auto-revert and compatibility adapter"`

---

### Task 3: Wire Transaction Engine, Composite Backend & Profile Auto-Restore

**Files:**
- Modify: `solarui/crates/blaze-akrep-privd/src/adapters/mod.rs` (`CompositeMutationBackend`)
- Modify: `solarui/crates/blaze-akrep-privd/src/adapters/profiles.rs`
- Modify: `solarui/crates/blaze-akrep-privd/src/transaction_engine.rs`
- Modify: `solarui/crates/blaze-akrep-privd/src/adapters/device_security.rs`

**Interfaces:**
- Consumes: Task 1 `ActionRequest` variants, Task 2 `OverrideAdapter`.
- Produces:
  - Full `CreatePlan` / `CommitPlan` / `RollbackTransaction` handling for all 6 new `ActionRequest` variants in `CompositeMutationBackend` and `TransactionEngine`.
  - `ProfileAdapter` auto-restores SELinux `Enforcing` (if currently `Permissive`, still failing closed if `Disabled`) and starts/enables `firewalld` (if currently stopped) + clears Akrep override timers as part of profile preview/apply/rollback.
  - `DeviceSecurityAdapter` deep audit populates MOK/NVIDIA summary (`mokutil --sb-state`, `mokutil --list-enrolled`, `/sys/module/nvidia`).

- [ ] **Step 1: Write failing integration tests in `blaze-akrep-privd/tests/flexible_controls_tx.rs`**
  - `test_tx_selinux_mode_preview_commit_and_rollback`: end-to-end `TransactionEngine` `create_plan` -> `commit_plan` -> `rollback_transaction` for `SelinuxSetMode { Permissive, UntilChanged }` restoring `Enforcing`.
  - `test_tx_raw_json_still_blocks_audit2allow_semodule_disabled_and_free_text_setenforce`: asserts `ActionRequest::SelinuxSetBoolean { name: "setenforce 0" }` and `"selinux=disabled"` are STILL blocked by `TransactionEngine`, while typed `ActionRequest::SelinuxSetMode` succeeds!
  - `test_tx_apply_profile_auto_restores_permissive_selinux_and_paused_firewalld`: sets SELinux `Permissive` and `firewalld` stopped via `OverrideAdapter`, then calls `preview_profile("balanced")` -> `commit_plan` -> asserts SELinux is back to `Enforcing`, `firewalld` is running, `overrides.json` is cleared, and rollback works cleanly; also asserts `SELinux Disabled` still returns `BackendUnavailable`.
  - `test_tx_container_dir_rollback_succeeds_after_child_file_changes`: applies `SelinuxLabelContainerDir` on a temp dir, writes/modifies child files inside it, then runs `rollback_transaction` -> asserts `TransactionState::RolledBack` (NO `ExternalDriftDetected`).

- [ ] **Step 2: Run test to verify failure**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-privd --test flexible_controls_tx`
  - Expected: FAIL.

- [ ] **Step 3: Wire `CompositeMutationBackend`, `TransactionEngine`, `ProfileAdapter`, and `DeviceSecurityAdapter`**
  - Add `overrides: Arc<OverrideAdapter>` to `CompositeMutationBackend` and pass it into `ProfileAdapter`.
  - Dispatch the 6 new `ActionRequest` variants in `CompositeMutationBackend::{preview, apply, rollback}`.
  - Refine `transaction_engine.rs:1272` raw JSON scanner so `"audit2allow"`, `"semodule"`, `"selinux=disabled"` stay unconditionally rejected everywhere and `"setenforce"` stays rejected in all non-`selinux_set_mode` payloads.
  - Update `ProfileAdapter::ensure_prerequisites_and_resolve_zone`, `preview_profile`, `apply_profile`, and `rollback_profile` to include automatic SELinux `Enforcing` + `firewalld` restore diffs when relaxed (while still rejecting `SelinuxRuntimeMode::Disabled`).
  - Append MOK/NVIDIA summary metrics inside `DeviceSecurityAdapter`.

- [ ] **Step 4: Run all `blaze-akrep-privd` tests**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-privd`
  - Expected: PASS (all existing Sprint 0–4 tests + new `flexible_controls_tx` tests green).

- [ ] **Step 5: Commit**
  - `git add solarui/crates/blaze-akrep-privd/ && git commit -m "feat(akrep-privd): wire flexible override transactions, profile auto-restore and MOK audit"`

---

### Task 4: Read-Only `akrepd` Collectors & Snapshot Reconciliation

**Files:**
- Create: `solarui/crates/blaze-akrep-daemon/src/collectors/compat.rs`
- Modify: `solarui/crates/blaze-akrep-daemon/src/collectors/mod.rs`

**Interfaces:**
- Consumes: Task 1 `blaze_sentinel_core::{ActiveOverrideEntry, SystemCompatStatus, DriverMokSummary, AkrepSnapshot}` and `/var/cache/akrep/active_overrides.json`.
- Produces in `blaze_akrep_daemon::collectors::compat`:
  - `pub fn collect_active_overrides(cache_path: &Path, current_boot_id: &str, now: DateTime<Utc>) -> Vec<ActiveOverrideEntry>`
  - `pub fn collect_system_compat_cached() -> SystemCompatStatus` (reads `systemd-oomd` via `systemctl is-active`/`is-enabled`, parses `/etc/security/limits.d/90-akrep-dev-nofile.conf`, 15s TTL-cached `rpm -q android-udev-rules ncurses-compat-libs`)
  - `pub fn collect_driver_mok_cached() -> DriverMokSummary` (reads Secure Boot EFI var, `/sys/module/nvidia`, 15s TTL-cached `rpm -q akmod-nvidia` + `mokutil --list-enrolled` summary)
  - `pub fn reconcile_snapshot_with_overrides(...)` (annotates/deduplicates findings & section severities: `Timed`/`UntilReboot` -> `Attention` with remaining-time note; SELinux `UntilChanged` -> `Attention`; Firewalld `UntilChanged` -> `Critical`; also includes `tr.com.blazeos.Akrep1.system.compat-manage` in `default_polkit_actions()`)

- [ ] **Step 1: Write failing unit tests in `collectors/compat.rs`**
  - `test_collect_active_overrides_drops_stale_boot_id_and_expired_timed`: writes `active_overrides.json` with a `Timed` entry from a past timestamp, an `UntilReboot` entry with mismatched `boot_id`, and an `UntilChanged` entry with mismatched `boot_id` -> asserts only `UntilChanged` survives!
  - `test_collect_active_overrides_computes_remaining_secs_for_live_timed`: writes a live `Timed(15)` entry with matching `boot_id` -> asserts `remaining_secs > 0`.
  - `test_reconcile_downgrades_timed_firewall_and_selinux_findings_to_attention_and_keeps_until_changed_firewall_critical`: verifies severity mapping and finding deduplication.

- [ ] **Step 2: Run tests to verify failure**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-daemon`
  - Expected: FAIL.

- [ ] **Step 3: Implement `collectors/compat.rs` and integrate into `collectors/mod.rs`**
  - Run blocking reads inside the existing `spawn_blocking` block in `collect_snapshot` with `OnceLock<Mutex<...>>` 15s TTL cache for `rpm`/`mokutil` subprocesses so `collect_snapshot` remains fast (`<250ms`).
  - Keep `blaze-akrep-daemon/src/dbus.rs` strictly read-only (5 existing methods untouched).

- [ ] **Step 4: Run daemon tests & clippy**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-daemon && cargo clippy --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-daemon --all-targets -- -D warnings`
  - Expected: PASS (0 warnings).

- [ ] **Step 5: Commit**
  - `git add solarui/crates/blaze-akrep-daemon/ && git commit -m "feat(akrepd): collect active overrides, system compatibility and MOK summary read-only"`

---

### Task 5: Akrep GTK4 UI Controls & Unslop-Compliant Presentation (`blaze-akrep-ui`)

**Files:**
- Modify: `solarui/crates/blaze-akrep-ui/src/state.rs`
- Modify: `solarui/crates/blaze-akrep-ui/src/pages/overview.rs`
- Modify: `solarui/crates/blaze-akrep-ui/src/pages/system_protection.rs`
- Modify: `solarui/crates/blaze-akrep-ui/src/pages/firewall.rs`
- Modify: `solarui/crates/blaze-akrep-ui/src/pages/applications.rs`
- Modify: `solarui/crates/blaze-akrep-ui/src/pages/device_security.rs`
- Modify: `solarui/crates/blaze-akrep-ui/src/components/plan_dialog.rs` (ensure clear human-readable diff labels for the 6 new actions + high-risk callout when `UntilChanged` is previewed)

**Interfaces:**
- Consumes: Task 1 `ActionRequest` variants & `AkrepSnapshot.{active_overrides, system_compat, driver_mok}` via existing `UiCommand::CreatePlan`.
- Produces:
  - Draft state fields in `AppUiState`: `selinux_duration_idx`, `firewall_duration_idx`, `oomd_duration_idx`, `container_dir_path`, `container_dir_recursive`.
  - Clean segmented duration selector helper (`15 dk`, `1 saat`, `Yeniden başlatmaya kadar`, `Ben tekrar değiştirene kadar`; OOMD: `1 saat`, `4 saat`, `Yeniden başlatmaya kadar`, `Ben tekrar değiştirene kadar`).
  - **Overview (`overview.rs`):** Quiet amber **Etkin gevşetmeler** card right below the hero block whenever `!snap.active_overrides.is_empty()` with per-subsystem countdown/mode and `Etkinleştir` button (goes through `CreatePlan` preview modal).
  - **System Protection (`system_protection.rs`):**
    - SELinux mode card: `Enforcing` -> duration selector + secondary `Permissive moduna geç`; `Permissive` -> active mode/countdown + off-white primary `Enforcing moduna dön` + duration selector + `Süreyi güncelle`.
    - New **Geliştirici ve derleme uyumluluğu** card: `systemd-oomd` pause/resume + duration selector, `nofile 65536 / 524288` toggle (`Etkinleştir` / `Varsayılana dön`), container/ROM folder label input + recursive checkbox + `Etiket uygula` + `Varsayılan etikete dön`, and Android packages (`android-udev-rules`, `ncurses-compat-libs`) status dots + `Kur`.
  - **Firewall (`firewall.rs`):**
    - Firewalld pause/resume + duration selector (`Duraklat` / `Etkinleştir` / `Süreyi güncelle`).
    - Additional quick presets in FlowBox: `Samba (yerel ağ)`, `Web geliştirme (3000/tcp)`, `Web geliştirme (8080/tcp)`, `Jellyfin (8096/tcp)`.
  - **Applications (`applications.rs`):**
    - Add `Oyun kumandası (input)` (`("Context.devices", "input")` / `"!input"`) toggle per Flatpak app.
    - Add quick disk folder buttons (`/mnt`, `/run/media`) next to custom folder grant row.
  - **Device Security (`device_security.rs`):**
    - Render immediate read-only **Secure Boot ve sürücü uyumluluğu (MOK)** card from `snap.driver_mok` (without requiring Polkit first!), plus keep full deep-audit details when `Derin denetim` is clicked.

- [ ] **Step 1: Write failing UI unit tests in `blaze-akrep-ui/src/main.rs` (or module tests)**
  - `test_duration_choice_mapping_and_labels`: tests all 4 duration choices (`15 dk`, `1 saat`, `Yeniden başlatmaya kadar`, `Ben tekrar değiştirene kadar`) map cleanly to `OverrideDuration` variants and never contain `(Polkit)`, `!`, or emojis.
  - `test_flatpak_gamepad_input_toggle_delta`: tests `compute_capability_override_delta("Context.devices", "input", "!input", false/true)` produces valid non-forbidden `FlatpakSetOverride` pairs (`validate_flatpak_override_changes` returns `Ok(())`).
  - Existing `test_ten_navigation_sections_and_card_routing` continues asserting `nav_count == 10`.

- [ ] **Step 2: Run tests to verify failure**
  - Run: `cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-ui`
  - Expected: FAIL (new helper/tests not defined yet).

- [ ] **Step 3: Implement UI additions strictly matching the Graphite + Blaze Ember unslop spec**
  - Update `state.rs`, `overview.rs`, `system_protection.rs`, `firewall.rs`, `applications.rs`, `device_security.rs`, and `plan_dialog.rs`.
  - Ensure all labels wrap (`WrapMode::WordChar`), `FlowBox` / horizontal rows fit cleanly at `1060x720` with zero right-edge clipping, `changes-prevent-symbolic` + `Yönetici parolası istenir` captions replace any Polkit jargon, and zero emojis or `/home/` paths exist.

- [ ] **Step 4: Run tests, clippy, CSS parse check & `--smoke-test`**
  - Run: `cargo clippy --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-ui --all-targets -- -D warnings && cargo test --manifest-path solarui/Cargo.toml --locked --offline -p blaze-akrep-ui && ./solarui/target/debug/akrep-ui --smoke-test`
  - Expected: PASS (0 warnings, `nav_count == 10`, `version == "1.0.0"`).

- [ ] **Step 5: Commit**
  - `git add solarui/crates/blaze-akrep-ui/ && git commit -m "feat(akrep-ui): add flexible SELinux/Firewalld/OOMD controls, dev/gaming compat helpers and MOK card"`

---

### Task 6: Audit Regressions, Stripped Release Binaries, Signed RPMs, Live ISO & Mandatory QEMU Verification

**Files:**
- Modify: `tests/test_audit_regressions.py`
- Refresh (tracked release artifacts): `blazeos_custom_apps/usr/bin/{akrepd,akrep-privd,akrep-ui}`, `Blaze-SolarEvolution-5.05.1-RC-x86_64.iso.sha256`, `Blaze-SolarEvolution-5-x86_64.iso.sha256`

**Interfaces:**
- Consumes: Tasks 1–5 committed cleanly.
- Produces: Verified EdDSA-signed RPMs, rebuilt Live ISO (`checkisomd5: PASS`), `79/79` BIOS+UEFI QEMU E2E pass, and live QEMU functional + visual screenshot verification of Akrep's new flexible controls.

- [ ] **Step 1: Update `tests/test_audit_regressions.py` to assert the new Polkit action (`tr.com.blazeos.Akrep1.system.compat-manage`) and flexible override invariants (zero `SELINUX=disabled`, `akrepd` strictly read-only)**
  - Run: `python3 -B -m unittest tests.test_audit_regressions -v` (will run fully after stripped binaries are built in Step 2).

- [ ] **Step 2: Build stripped release binaries with `--remap-path-prefix=${HOME}=/build`, verify 0 `/home/` paths, run full test suite, build & verify signed RPMs, and build Live ISO**
  - Run:
    ```bash
    RUSTFLAGS="--remap-path-prefix=${HOME}=/build" cargo build --manifest-path solarui/Cargo.toml --release --locked --offline -p blaze-akrep-daemon -p blaze-akrep-privd -p blaze-akrep-ui
    strip --strip-unneeded solarui/target/release/{akrepd,akrep-privd,akrep-ui}
    install -m 0755 solarui/target/release/akrepd blazeos_custom_apps/usr/bin/akrepd
    install -m 0755 solarui/target/release/akrep-privd blazeos_custom_apps/usr/bin/akrep-privd
    install -m 0755 solarui/target/release/akrep-ui blazeos_custom_apps/usr/bin/akrep-ui
    for b in akrepd akrep-privd akrep-ui; do [ "$(strings blazeos_custom_apps/usr/bin/$b | grep -c "$HOME" || true)" -eq 0 ]; done
    python3 -B -m unittest tests.test_audit_regressions -v
    python3 packaging/check_ownership.py
    ./packaging/build-rpms.sh
    ./packaging/verify-repo.sh
    SKIP_RPM_BUILD=1 ./build_f45_final.sh
    ```
  - Expected: 0 dev-path hits, `36/36` audit regressions OK, `100%` ownership, all 12 RPMs signed & verified, `checkisomd5: PASS`, relative `.iso.sha256`.

- [ ] **Step 3: Run mandatory QEMU Legacy BIOS + UEFI OVMF E2E**
  - Run: `LC_ALL=tr_TR.UTF-8 python3 -m tests.e2e.run_e2e --all --firmware both`
  - Expected: `79/79 OK` + visually inspect BIOS & UEFI Plymouth, SolarUI desktop, FireHub, and Anaconda WebUI screenshots.

- [ ] **Step 4: Run live QEMU UEFI functional & visual verification of Akrep's new flexible controls**
  - Boot the rebuilt Live ISO in QEMU UEFI (`QemuVM`), test via D-Bus/QGA + `akrep-ui`:
    1. SELinux `Enforcing` -> `Permissive (Timed 15m)` -> verify `getenforce == Permissive`, `active_overrides.json`, active systemd timer `akrep-revert-selinux.timer`, Overview `Etkin gevşetmeler` card screenshot, and `Enforcing` restore.
    2. Firewalld pause (`Timed 15m`) -> verify stopped + timer + resume; test `akrep-privd --revert-override` stale-generation no-op.
    3. `systemd-oomd` pause/resume + `nofile` drop-in (`65536/524288`) enable/rollback + container directory `chcon`/`restorecon` in `/home/liveuser/android_test`.
    4. Capture and visually inspect QEMU screenshots of `Overview` (with active override banner), `System Protection` (SELinux mode + Dev/ROM compat card), `Firewall` (pause control + new LAN/dev presets), `Applications` (gamepad input + quick folder buttons), and `Device Security` (MOK/driver summary). Fix any visual or functional defect and re-verify!

- [ ] **Step 5: Commit release artifacts and clean temporary `/tmp` VM files**
  - `git add tests/test_audit_regressions.py blazeos_custom_apps/usr/bin/{akrepd,akrep-privd,akrep-ui} Blaze-SolarEvolution-5.05.1-RC-x86_64.iso.sha256 Blaze-SolarEvolution-5-x86_64.iso.sha256 && git commit -m "feat(release): package Akrep flexible controls, update tests and ISO checksums"`
  - Clean `/tmp/blaze_e2e_artifacts`, keep curated screenshots in the conversation artifact directory, update walkthrough, and ask user before `git push origin main`.
