use blaze_akrep_common::{
    ActionRequest, AkrepError, AppOrigin, BusKind, FilesystemAccessMode,
    FlatpakInstallationSnapshot, FlatpakScope, MockFlatpakRuntimeProbe, MockPortalPrivacyBackend,
    PermissionSourceLayer, PermissionStoreEntry, PortalDiscoveryReport, PortalPrivacyAction,
    PortalPrivacyBackend, PrivacySessionController, PrivacyStreamKind, RemoteDesktopDeviceType,
    ScreenCastCursorMode, ScreenCastSourceType, TransactionState, UserFlatpakManager,
    IFACE_PORTAL_CAMERA, IFACE_PORTAL_GLOBAL_SHORTCUTS, IFACE_PORTAL_REMOTE_DESKTOP,
    IFACE_PORTAL_SCREENCAST, IFACE_PORTAL_USB, POLKIT_FIREWALL_MANAGE,
    POLKIT_FLATPAK_SYSTEM_MANAGE,
};
use blaze_akrep_privd::{
    CompositeMutationBackend, FirewallAdapter, IdleTracker, MockAuthorizer, MockFirewallBackend,
    MockSelinuxOps, SelinuxAdapter, SystemFlatpakAdapter, TransactionEngine,
};
use blaze_sentinel_core::{AkrepSeverity, Availability};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

struct Sprint2Harness {
    _root_tmp: TempDir,
    system_flatpak_dir: PathBuf,
    user_flatpak_dir: PathBuf,
    system_apps_dir: PathBuf,
    user_apps_dir: PathBuf,
    user_tx_dir: PathBuf,
    runtime_probe: Arc<MockFlatpakRuntimeProbe>,
    auth: Arc<MockAuthorizer>,
    engine: TransactionEngine,
    user_mgr: UserFlatpakManager,
}

fn setup_sprint2_harness() -> Sprint2Harness {
    let root_tmp = TempDir::new().expect("tempdir");
    let base = root_tmp.path();

    let privd_state_dir = base.join("var_lib_akrep");
    let system_flatpak_dir = base.join("var_lib_flatpak");
    let user_flatpak_dir = base.join("xdg_data_flatpak");
    let system_apps_dir = base.join("usr_share_applications");
    let user_apps_dir = base.join("xdg_data_applications");
    let user_tx_dir = base.join("xdg_data_akrep_tx");

    for dir in [
        &system_flatpak_dir,
        &user_flatpak_dir,
        &system_apps_dir,
        &user_apps_dir,
    ] {
        fs::create_dir_all(dir).expect("mkdir");
    }

    let runtime_probe = Arc::new(MockFlatpakRuntimeProbe::new());
    let auth = Arc::new(MockAuthorizer::new());
    auth.allow_all_for_bus(":1.30", 1000);

    let selinux_adapter = Arc::new(SelinuxAdapter::new(Arc::new(
        MockSelinuxOps::new_fedora_enforcing(),
    )));
    let firewall_adapter = Arc::new(FirewallAdapter::new(Arc::new(
        MockFirewallBackend::new_fedora_workstation(),
    )));
    let system_flatpak_adapter = Arc::new(SystemFlatpakAdapter::with_roots(
        system_flatpak_dir.clone(),
        system_apps_dir.clone(),
        runtime_probe.clone(),
    ));
    let composite = Arc::new(CompositeMutationBackend::with_flatpak(
        selinux_adapter,
        firewall_adapter,
        system_flatpak_adapter,
    ));

    let engine =
        TransactionEngine::new(privd_state_dir, auth.clone(), composite).expect("engine init");

    let user_mgr = UserFlatpakManager::with_paths(
        user_flatpak_dir.clone(),
        system_flatpak_dir.clone(),
        vec![user_apps_dir.clone(), system_apps_dir.clone()],
        user_tx_dir.clone(),
        runtime_probe.clone(),
    );

    Sprint2Harness {
        _root_tmp: root_tmp,
        system_flatpak_dir,
        user_flatpak_dir,
        system_apps_dir,
        user_apps_dir,
        user_tx_dir,
        runtime_probe,
        auth,
        engine,
        user_mgr,
    }
}

#[allow(clippy::too_many_arguments)]
fn install_mock_flatpak_app(
    flatpak_root: &Path,
    app_id: &str,
    arch: &str,
    branch: &str,
    commit_hex: &str,
    display_name: &str,
    version: &str,
    metadata_ini: &str,
    use_gvariant_deploy: bool,
) {
    let commit_dir = flatpak_root
        .join("app")
        .join(app_id)
        .join(arch)
        .join(branch)
        .join(commit_hex);
    fs::create_dir_all(&commit_dir).expect("create commit dir");
    fs::write(commit_dir.join("metadata"), metadata_ini).expect("write metadata");

    if use_gvariant_deploy {
        let mut raw_deploy = Vec::new();
        raw_deploy.extend_from_slice(b"flathub\0");
        raw_deploy.extend_from_slice(b"appdata-name\0\0");
        raw_deploy.extend_from_slice(display_name.as_bytes());
        raw_deploy.push(0);
        raw_deploy.extend_from_slice(b"appdata-version\0");
        raw_deploy.extend_from_slice(version.as_bytes());
        raw_deploy.push(0);
        fs::write(commit_dir.join("deploy"), raw_deploy).expect("write binary deploy");
    } else {
        let deploy_txt = format!(
            "display_name={}\nversion={}\nbranch={}\n",
            display_name, version, branch
        );
        fs::write(commit_dir.join("deploy"), deploy_txt).expect("write text deploy");
    }

    let export_apps = commit_dir.join("export/share/applications");
    fs::create_dir_all(&export_apps).expect("create export apps");
    let desktop_content = format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec=/usr/bin/flatpak run {}\nX-Flatpak={}\n",
        display_name, app_id, app_id
    );
    fs::write(
        export_apps.join(format!("{}.desktop", app_id)),
        desktop_content,
    )
    .expect("write exported desktop");

    let branch_dir = flatpak_root
        .join("app")
        .join(app_id)
        .join(arch)
        .join(branch);
    let active_link = branch_dir.join("active");
    if active_link.exists() {
        let _ = fs::remove_file(&active_link);
    }
    symlink(commit_hex, &active_link).expect("symlink active");

    let app_dir = flatpak_root.join("app").join(app_id);
    let current_link = app_dir.join("current");
    if current_link.exists() {
        let _ = fs::remove_file(&current_link);
    }
    symlink(format!("{}/{}", arch, branch), &current_link).expect("symlink current");
}

// ============================================================================
// Application Security Acceptance Tests (APP-01 .. APP-10)
// ============================================================================

#[tokio::test]
async fn app_01_inventory_lists_user_and_system_flatpaks() {
    let h = setup_sprint2_harness();

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.mozilla.firefox",
        "x86_64",
        "stable",
        "1111111111111111111111111111111111111111111111111111111111111111",
        "Firefox Web Browser",
        "134.0.1",
        "[Application]\nname=org.mozilla.firefox\nruntime=org.freedesktop.Platform/x86_64/24.08\n\n[Context]\nshared=ipc;network;\nsockets=wayland;pulseaudio;\ndevices=dri;\n",
        true,
    );
    install_mock_flatpak_app(
        &h.user_flatpak_dir,
        "com.obsproject.Studio",
        "x86_64",
        "stable",
        "2222222222222222222222222222222222222222222222222222222222222222",
        "OBS Studio",
        "31.0.0",
        "[Application]\nname=com.obsproject.Studio\nruntime=org.kde.Platform/x86_64/6.8\n\n[Context]\nshared=ipc;network;\nsockets=wayland;pulseaudio;\ndevices=dri;\n",
        false,
    );

    // Unified unprivileged session inventory sees both User and System Flatpak installations
    let inv = h.user_mgr.inspect_unified().expect("unified inventory");
    assert_eq!(inv.system_apps.len(), 1);
    assert_eq!(inv.user_apps.len(), 1);

    let ff = &inv.system_apps[0];
    assert_eq!(ff.app_id, "org.mozilla.firefox");
    assert_eq!(ff.display_name, "Firefox Web Browser");
    assert_eq!(ff.version, "134.0.1");
    assert_eq!(ff.arch, "x86_64");
    assert_eq!(ff.branch, "stable");
    assert_eq!(ff.runtime, "org.freedesktop.Platform/x86_64/24.08");
    assert_eq!(ff.scope, FlatpakScope::System);
    assert!(ff.origin.sandbox_supported());

    let obs = &inv.user_apps[0];
    assert_eq!(obs.app_id, "com.obsproject.Studio");
    assert_eq!(obs.display_name, "OBS Studio");
    assert_eq!(obs.version, "31.0.0");
    assert_eq!(obs.scope, FlatpakScope::User);
    assert!(obs.origin.sandbox_supported());

    // System D-Bus inspection endpoint on akrep-privd returns the System installation snapshot
    let sys_json = h
        .engine
        .inspect_system_flatpaks(":1.30")
        .await
        .expect("inspect_system_flatpaks");
    let sys_snap: FlatpakInstallationSnapshot = serde_json::from_str(&sys_json).unwrap();
    assert_eq!(sys_snap.scope, FlatpakScope::System);
    assert_eq!(sys_snap.apps.len(), 1);
    assert_eq!(sys_snap.apps[0].app_id, "org.mozilla.firefox");
}

#[tokio::test]
async fn app_02_manifest_permissions_parsed_accurately() {
    let h = setup_sprint2_harness();

    let rich_manifest = "\
[Application]
name=org.example.MediaSuite
runtime=org.freedesktop.Platform/x86_64/24.08

[Context]
shared=network;ipc;
sockets=wayland;x11;pulseaudio;
devices=all;usb;dri;
features=bluetooth;
filesystems=host;~/Projects:rw;xdg-download:ro;

[Session Bus Policy]
org.freedesktop.Notifications=talk
org.freedesktop.secrets=talk

[System Bus Policy]
org.freedesktop.UPower=talk
";

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.example.MediaSuite",
        "x86_64",
        "stable",
        "3333333333333333333333333333333333333333333333333333333333333333",
        "Media Suite",
        "2.4.0",
        rich_manifest,
        false,
    );

    // Global override (read-only layer) revokes x11; per-app override revokes host, devices=all, and bluetooth
    let ov_dir = h.system_flatpak_dir.join("overrides");
    fs::create_dir_all(&ov_dir).unwrap();
    fs::write(ov_dir.join("global"), "[Context]\nsockets=!x11;\n").unwrap();
    fs::write(
        ov_dir.join("org.example.MediaSuite"),
        "[Context]\ndevices=!all;\nfeatures=!bluetooth;\nfilesystems=!host;\n\n[Session Bus Policy]\norg.freedesktop.secrets=none\n",
    )
    .unwrap();

    let inv = h.user_mgr.inspect_unified().unwrap();
    let app = inv
        .find_flatpak("org.example.MediaSuite", FlatpakScope::System)
        .expect("MediaSuite present");

    // 1. Raw manifest permissions reflect all manifest declarations including broad warnings
    let mp = &app.manifest_permissions;
    assert!(mp.network.granted);
    assert!(mp.microphone.granted);
    assert!(mp.camera.direct_devices_all);
    assert!(mp.camera.warning);
    assert!(!mp.camera.portal_mediated);
    assert!(mp.usb.granted);
    assert!(mp.bluetooth.granted);
    assert!(mp.gpu.granted);
    assert!(mp.wayland.granted);
    assert!(mp.x11.granted);
    assert!(mp.has_filesystem_access("host"));
    assert!(mp.filesystem_grant("host").unwrap().is_broad_warning);
    assert_eq!(
        mp.filesystem_grant("~/Projects").unwrap().mode,
        FilesystemAccessMode::ReadWrite
    );
    assert_eq!(
        mp.filesystem_grant("xdg-download").unwrap().mode,
        FilesystemAccessMode::ReadOnly
    );
    let sec_bus = mp
        .bus_policy(BusKind::Session, "org.freedesktop.secrets")
        .unwrap();
    assert_eq!(sec_bus.policy, "talk");
    assert!(sec_bus.is_sensitive_warning);
    assert_eq!(
        mp.bus_policy(BusKind::System, "org.freedesktop.UPower")
            .unwrap()
            .policy,
        "talk"
    );

    // 2. Effective permissions accurately reflect Manifest -> GlobalOverride -> AppOverride precedence
    let ep = &app.effective_permissions;
    assert!(ep.network.granted);
    assert!(ep.microphone.granted);
    assert!(!ep.x11.granted);
    assert_eq!(ep.x11.source, Some(PermissionSourceLayer::GlobalOverride));
    assert!(!ep.camera.direct_devices_all);
    assert!(ep.camera.portal_mediated);
    assert!(ep.camera.revoked_all);
    assert_eq!(ep.camera.source, Some(PermissionSourceLayer::AppOverride));
    assert!(ep.usb.granted);
    assert!(ep.gpu.granted);
    assert!(!ep.bluetooth.granted);
    assert!(!ep.has_filesystem_access("host"));
    assert_eq!(
        ep.filesystem_grant("host").unwrap().mode,
        FilesystemAccessMode::Revoked
    );
    assert!(ep.has_filesystem_access("~/Projects"));
    assert!(ep.has_filesystem_access("xdg-download"));
    assert_eq!(
        ep.bus_policy(BusKind::Session, "org.freedesktop.secrets")
            .unwrap()
            .policy,
        "none"
    );
}

#[tokio::test]
async fn app_03_user_vs_system_scope_distinction() {
    let h = setup_sprint2_harness();

    let manifest = "\
[Application]
name=org.gimp.GIMP
runtime=org.gnome.Platform/x86_64/47

[Context]
shared=ipc;
sockets=wayland;
devices=dri;
";

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.gimp.GIMP",
        "x86_64",
        "stable",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "GNU Image Manipulation Program",
        "3.0.0",
        manifest,
        false,
    );
    install_mock_flatpak_app(
        &h.user_flatpak_dir,
        "org.gimp.GIMP",
        "x86_64",
        "beta",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "GNU Image Manipulation Program (User)",
        "3.0.1",
        manifest,
        false,
    );

    // 1. Both scopes appear independently in unified inventory
    let inv_before = h.user_mgr.inspect_unified().unwrap();
    assert_eq!(inv_before.system_apps.len(), 1);
    assert_eq!(inv_before.user_apps.len(), 1);
    assert_eq!(inv_before.system_apps[0].scope, FlatpakScope::System);
    assert_eq!(inv_before.user_apps[0].scope, FlatpakScope::User);

    // 2. User-scope override rejected on akrep-privd (ProtectHome invariant)
    let priv_user_err = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.gimp.GIMP".to_string(),
                scope: FlatpakScope::User,
                additions: vec![("Context.filesystems".to_string(), "~/Photos:rw".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(priv_user_err, AkrepError::ForbiddenOperation(_)));
    assert!(priv_user_err
        .message()
        .contains("User-scope Flatpak overrides must execute in user session"));

    // 3. User-scope override via UserFlatpakManager mutates ONLY user_flatpak_dir
    let user_tx = h
        .user_mgr
        .apply_user_override(
            "org.gimp.GIMP",
            &[("Context.filesystems".to_string(), "~/Photos:rw".to_string())],
            &[],
        )
        .expect("apply user override");
    assert_eq!(user_tx.scope, FlatpakScope::User);
    assert_eq!(user_tx.state, TransactionState::Committed);
    assert!(h.user_flatpak_dir.join("overrides/org.gimp.GIMP").is_file());
    assert!(!h
        .system_flatpak_dir
        .join("overrides/org.gimp.GIMP")
        .exists());
    assert!(h
        .user_tx_dir
        .join(format!("{}.json", user_tx.tx_id))
        .is_file());

    let inv_after_user = h.user_mgr.inspect_unified().unwrap();
    assert!(inv_after_user
        .find_flatpak("org.gimp.GIMP", FlatpakScope::User)
        .unwrap()
        .effective_permissions
        .has_filesystem_access("~/Photos"));
    assert!(!inv_after_user
        .find_flatpak("org.gimp.GIMP", FlatpakScope::System)
        .unwrap()
        .effective_permissions
        .has_filesystem_access("~/Photos"));

    // 4. System-scope override via akrep-privd mutates ONLY system_flatpak_dir
    let sys_plan = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.gimp.GIMP".to_string(),
                scope: FlatpakScope::System,
                additions: vec![(
                    "Context.filesystems".to_string(),
                    "xdg-pictures:ro".to_string(),
                )],
                removals: vec![],
            },
        )
        .await
        .expect("create system plan");
    let sys_rec = h
        .engine
        .commit_plan(":1.30", &sys_plan.plan_id, &sys_plan.plan_sha256)
        .await
        .expect("commit system plan");
    assert_eq!(sys_rec.state, TransactionState::Committed);

    let inv_after_both = h.user_mgr.inspect_unified().unwrap();
    let sys_gimp = inv_after_both
        .find_flatpak("org.gimp.GIMP", FlatpakScope::System)
        .unwrap();
    let usr_gimp = inv_after_both
        .find_flatpak("org.gimp.GIMP", FlatpakScope::User)
        .unwrap();
    assert!(sys_gimp
        .effective_permissions
        .has_filesystem_access("xdg-pictures"));
    assert!(!sys_gimp
        .effective_permissions
        .has_filesystem_access("~/Photos"));
    assert!(usr_gimp
        .effective_permissions
        .has_filesystem_access("~/Photos"));
    assert!(!usr_gimp
        .effective_permissions
        .has_filesystem_access("xdg-pictures"));
}

#[tokio::test]
async fn app_04_grant_narrow_folder_affects_only_target_app() {
    let h = setup_sprint2_harness();

    let base_manifest = "[Application]\nname=org.kde.krita\nruntime=org.kde.Platform/x86_64/6.8\n\n[Context]\nsockets=wayland;\n";
    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.kde.krita",
        "x86_64",
        "stable",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "Krita",
        "5.2.6",
        base_manifest,
        false,
    );
    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.inkscape.Inkscape",
        "x86_64",
        "stable",
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
        "Inkscape",
        "1.4",
        "[Application]\nname=org.inkscape.Inkscape\nruntime=org.gnome.Platform/x86_64/47\n\n[Context]\nsockets=wayland;\n",
        false,
    );

    // Mark Krita as currently running via flatpak ps probe
    h.runtime_probe.set_running("org.kde.krita", true);

    // Verify red-lines block blanket grants before granting narrow folder
    for forbidden_tok in ["host", "host-os", "host-etc", "home", "~", "/", "/home"] {
        let bad = h
            .engine
            .create_plan(
                ":1.30",
                ActionRequest::FlatpakSetOverride {
                    app_id: "org.kde.krita".to_string(),
                    scope: FlatpakScope::System,
                    additions: vec![("Context.filesystems".to_string(), forbidden_tok.to_string())],
                    removals: vec![],
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(bad, AkrepError::ForbiddenOperation(_)));
    }
    let bad_dev = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.kde.krita".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.devices".to_string(), "all".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(bad_dev, AkrepError::ForbiddenOperation(_)));

    // Grant narrow folder ~/Artwork:rw ONLY to org.kde.krita
    let plan = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.kde.krita".to_string(),
                scope: FlatpakScope::System,
                additions: vec![(
                    "Context.filesystems".to_string(),
                    "~/Artwork:rw".to_string(),
                )],
                removals: vec![],
            },
        )
        .await
        .expect("plan narrow folder");

    assert!(
        !plan.warnings_en.is_empty(),
        "running app must emit restart warning"
    );
    assert!(!plan.warnings_tr.is_empty());

    let rec = h
        .engine
        .commit_plan(":1.30", &plan.plan_id, &plan.plan_sha256)
        .await
        .expect("commit narrow folder");
    assert_eq!(rec.state, TransactionState::Committed);
    assert_eq!(
        rec.target_state
            .get("restart_required")
            .and_then(|v| v.as_bool()),
        Some(true)
    );

    let ov_file = h.system_flatpak_dir.join("overrides/org.kde.krita");
    assert!(ov_file.is_file());
    let mode = fs::metadata(&ov_file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "system Flatpak override file must be 0644");
    assert!(!h
        .system_flatpak_dir
        .join("overrides/org.inkscape.Inkscape")
        .exists());
    assert!(!h.system_flatpak_dir.join("overrides/global").exists());

    let inv = h.user_mgr.inspect_unified().unwrap();
    let krita = inv
        .find_flatpak("org.kde.krita", FlatpakScope::System)
        .unwrap();
    let inkscape = inv
        .find_flatpak("org.inkscape.Inkscape", FlatpakScope::System)
        .unwrap();
    assert!(krita.running);
    assert!(krita
        .effective_permissions
        .has_filesystem_access("~/Artwork"));
    assert!(!inkscape
        .effective_permissions
        .has_filesystem_access("~/Artwork"));
}

#[tokio::test]
async fn app_05_revoke_folder_removes_access() {
    let h = setup_sprint2_harness();

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.videolan.VLC",
        "x86_64",
        "stable",
        "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
        "VLC",
        "3.0.21",
        "[Application]\nname=org.videolan.VLC\nruntime=org.kde.Platform/x86_64/5.15-23.08\n\n[Context]\nfilesystems=xdg-videos:ro;\n",
        false,
    );

    // Step 1: Grant narrow folder ~/Clips:rw via override
    let grant_plan = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.videolan.VLC".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.filesystems".to_string(), "~/Clips:rw".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.30", &grant_plan.plan_id, &grant_plan.plan_sha256)
        .await
        .unwrap();

    let inv1 = h.user_mgr.inspect_unified().unwrap();
    let vlc1 = inv1
        .find_flatpak("org.videolan.VLC", FlatpakScope::System)
        .unwrap();
    assert!(vlc1.effective_permissions.has_filesystem_access("~/Clips"));
    assert!(vlc1
        .effective_permissions
        .has_filesystem_access("xdg-videos"));

    // Step 2: Revoke ~/Clips (override removal) AND revoke xdg-videos (manifest negation !xdg-videos)
    let revoke_plan = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.videolan.VLC".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.filesystems".to_string(), "!xdg-videos".to_string())],
                removals: vec![("Context.filesystems".to_string(), "~/Clips:rw".to_string())],
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.30", &revoke_plan.plan_id, &revoke_plan.plan_sha256)
        .await
        .unwrap();

    let inv2 = h.user_mgr.inspect_unified().unwrap();
    let vlc2 = inv2
        .find_flatpak("org.videolan.VLC", FlatpakScope::System)
        .unwrap();
    assert!(!vlc2.effective_permissions.has_filesystem_access("~/Clips"));
    assert!(!vlc2
        .effective_permissions
        .has_filesystem_access("xdg-videos"));
    assert_eq!(
        vlc2.effective_permissions
            .filesystem_grant("xdg-videos")
            .unwrap()
            .mode,
        FilesystemAccessMode::Revoked
    );
}

#[tokio::test]
async fn app_06_key_level_override_rollback_preserves_other_keys() {
    let h = setup_sprint2_harness();

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.blender.Blender",
        "x86_64",
        "stable",
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        "Blender",
        "4.3.2",
        "[Application]\nname=org.blender.Blender\nruntime=org.freedesktop.Platform/x86_64/24.08\n\n[Context]\nshared=network;ipc;\nsockets=wayland;pulseaudio;\ndevices=dri;\n",
        false,
    );
    install_mock_flatpak_app(
        &h.user_flatpak_dir,
        "org.audacityteam.Audacity",
        "x86_64",
        "stable",
        "9999999999999999999999999999999999999999999999999999999999999999",
        "Audacity",
        "3.7.1",
        "[Application]\nname=org.audacityteam.Audacity\nruntime=org.gnome.Platform/x86_64/47\n\n[Context]\nsockets=wayland;pulseaudio;\n",
        false,
    );

    // Pre-populate Blender system override with existing admin comment, sockets, and [Environment]
    let sys_ov_dir = h.system_flatpak_dir.join("overrides");
    fs::create_dir_all(&sys_ov_dir).unwrap();
    let sys_ov_path = sys_ov_dir.join("org.blender.Blender");
    fs::write(
        &sys_ov_path,
        "# Custom studio workstation config\n[Context]\nsockets=wayland;\nfilesystems=~/Assets:ro;\n\n[Environment]\nCYCLES_HIP_VISIBLE_DEVICES=0\n",
    )
    .unwrap();

    // Tx 1: Add ~/Renders:rw to Context.filesystems
    let plan1 = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.blender.Blender".to_string(),
                scope: FlatpakScope::System,
                additions: vec![(
                    "Context.filesystems".to_string(),
                    "~/Renders:rw".to_string(),
                )],
                removals: vec![],
            },
        )
        .await
        .unwrap();
    let tx1 = h
        .engine
        .commit_plan(":1.30", &plan1.plan_id, &plan1.plan_sha256)
        .await
        .unwrap();

    // Tx 2: Revoke network on a DIFFERENT key (Context.shared)
    let plan2 = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.blender.Blender".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.shared".to_string(), "!network".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap();
    let _tx2 = h
        .engine
        .commit_plan(":1.30", &plan2.plan_id, &plan2.plan_sha256)
        .await
        .unwrap();

    // Roll back Tx 1 ONLY -> ~/Renders:rw must be removed, while ~/Assets:ro, sockets=wayland;, shared=!network;, [Environment], and comment remain intact!
    let rolled_back = h
        .engine
        .rollback_transaction(":1.30", &tx1.tx_id)
        .await
        .expect("key-level rollback of tx1");
    assert_eq!(rolled_back.state, TransactionState::RolledBack);

    let disk_after_rb = fs::read_to_string(&sys_ov_path).unwrap();
    assert!(disk_after_rb.contains("# Custom studio workstation config"));
    assert!(disk_after_rb.contains("sockets=wayland;"));
    assert!(disk_after_rb.contains("filesystems=~/Assets:ro;"));
    assert!(!disk_after_rb.contains("~/Renders"));
    assert!(disk_after_rb.contains("shared=!network;"));
    assert!(disk_after_rb.contains("[Environment]\nCYCLES_HIP_VISIBLE_DEVICES=0"));

    // External drift on touched key blocks blind rollback in BOTH System and User scopes
    let plan3 = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.blender.Blender".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.filesystems".to_string(), "~/Scenes:rw".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap();
    let tx3 = h
        .engine
        .commit_plan(":1.30", &plan3.plan_id, &plan3.plan_sha256)
        .await
        .unwrap();

    // External admin modifies Context.filesystems directly
    fs::write(
        &sys_ov_path,
        disk_after_rb.replace(
            "filesystems=~/Assets:ro;",
            "filesystems=~/Assets:ro;~/ManualAdmin:ro;",
        ),
    )
    .unwrap();

    let drift_err = h
        .engine
        .rollback_transaction(":1.30", &tx3.tx_id)
        .await
        .unwrap_err();
    assert!(matches!(drift_err, AkrepError::ExternalDriftDetected(_)));
    assert!(fs::read_to_string(&sys_ov_path)
        .unwrap()
        .contains("~/ManualAdmin:ro"));

    // Verify UserFlatpakManager also performs key-level rollback & drift detection
    let usr_ov_dir = h.user_flatpak_dir.join("overrides");
    fs::create_dir_all(&usr_ov_dir).unwrap();
    fs::write(
        usr_ov_dir.join("org.audacityteam.Audacity"),
        "[Context]\nsockets=wayland;\n",
    )
    .unwrap();
    let utx1 = h
        .user_mgr
        .apply_user_override(
            "org.audacityteam.Audacity",
            &[(
                "Context.filesystems".to_string(),
                "~/Podcasts:rw".to_string(),
            )],
            &[],
        )
        .unwrap();
    let _utx2 = h
        .user_mgr
        .apply_user_override(
            "org.audacityteam.Audacity",
            &[("Context.devices".to_string(), "!all".to_string())],
            &[],
        )
        .unwrap();
    let urb1 = h.user_mgr.rollback_user_override(&utx1.tx_id).unwrap();
    assert_eq!(urb1.state, TransactionState::RolledBack);
    let usr_disk = fs::read_to_string(usr_ov_dir.join("org.audacityteam.Audacity")).unwrap();
    assert!(usr_disk.contains("sockets=wayland;"));
    assert!(usr_disk.contains("devices=!all;"));
    assert!(!usr_disk.contains("~/Podcasts"));

    // Verify O_NOFOLLOW symlink defense in system overrides
    fs::remove_file(&sys_ov_path).unwrap();
    let decoy = h._root_tmp.path().join("decoy_secret.conf");
    fs::write(&decoy, "DO_NOT_TOUCH\n").unwrap();
    symlink(&decoy, &sys_ov_path).unwrap();

    let symlink_err = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.blender.Blender".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.filesystems".to_string(), "~/Safe:ro".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(symlink_err, AkrepError::ForbiddenOperation(_)));
    assert_eq!(fs::read_to_string(&decoy).unwrap(), "DO_NOT_TOUCH\n");
}

#[tokio::test]
async fn app_07_global_override_protected_from_mutation() {
    let h = setup_sprint2_harness();

    let sys_ov_dir = h.system_flatpak_dir.join("overrides");
    fs::create_dir_all(&sys_ov_dir).unwrap();
    let global_content = "[Context]\nfilesystems=xdg-download:ro;\n";
    fs::write(sys_ov_dir.join("global"), global_content).unwrap();

    // Global overrides are visible read-only in snapshots
    let snap_raw = h.engine.inspect_system_flatpaks(":1.30").await.unwrap();
    let snap: FlatpakInstallationSnapshot = serde_json::from_str(&snap_raw).unwrap();
    assert!(snap.global_override_present);
    assert!(snap.global_override_tokens.contains(&(
        "Context.filesystems".to_string(),
        "xdg-download:ro".to_string()
    )));

    // Any attempt to mutate "global" (case-insensitive) in System or User scope is rejected
    for candidate in ["global", "Global", "GLOBAL"] {
        let sys_err = h
            .engine
            .create_plan(
                ":1.30",
                ActionRequest::FlatpakSetOverride {
                    app_id: candidate.to_string(),
                    scope: FlatpakScope::System,
                    additions: vec![("Context.sockets".to_string(), "!x11".to_string())],
                    removals: vec![],
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(sys_err, AkrepError::ForbiddenOperation(_)));

        let usr_err = h
            .user_mgr
            .apply_user_override(
                candidate,
                &[("Context.sockets".to_string(), "!x11".to_string())],
                &[],
            )
            .unwrap_err();
        assert!(matches!(usr_err, AkrepError::ForbiddenOperation(_)));
    }

    assert_eq!(
        fs::read_to_string(sys_ov_dir.join("global")).unwrap(),
        global_content
    );
}

#[tokio::test]
async fn app_08_nonexistent_app_id_rejected() {
    let h = setup_sprint2_harness();

    let missing_sys = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.nonexistent.GhostReader".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.shared".to_string(), "!network".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(missing_sys, AkrepError::InvalidInput(_)));
    assert!(!h
        .system_flatpak_dir
        .join("overrides/org.nonexistent.GhostReader")
        .exists());

    let missing_usr = h
        .user_mgr
        .apply_user_override(
            "org.nonexistent.GhostReader",
            &[("Context.shared".to_string(), "!network".to_string())],
            &[],
        )
        .unwrap_err();
    assert!(matches!(missing_usr, AkrepError::InvalidInput(_)));

    // Malformed app_ids (starting with digit, < 3 segments, traversal) rejected
    for malformed in ["123.bad.App", "two.segments", "../org.escape.App"] {
        assert!(h
            .user_mgr
            .apply_user_override(
                malformed,
                &[("Context.shared".to_string(), "!network".to_string())],
                &[],
            )
            .is_err());
    }
}

#[tokio::test]
async fn app_09_unauthorized_system_override_denied_by_polkit() {
    let h = setup_sprint2_harness();

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.signal.Signal",
        "x86_64",
        "stable",
        "7777777777777777777777777777777777777777777777777777777777777777",
        "Signal Desktop",
        "7.40.0",
        "[Application]\nname=org.signal.Signal\nruntime=org.freedesktop.Platform/x86_64/24.08\n\n[Context]\nshared=network;ipc;\n",
        false,
    );

    // Caller :1.99 has zero Polkit authorizations
    h.auth.deny_all_for_bus(":1.99", 1002);
    // Caller :1.98 is authorized ONLY for firewall.manage, NOT flatpak.system-manage
    h.auth
        .allow_actions_for_bus(":1.98", 1003, [POLKIT_FIREWALL_MANAGE]);

    let req = ActionRequest::FlatpakSetOverride {
        app_id: "org.signal.Signal".to_string(),
        scope: FlatpakScope::System,
        additions: vec![(
            "Context.filesystems".to_string(),
            "xdg-pictures:ro".to_string(),
        )],
        removals: vec![],
    };
    assert_eq!(req.required_polkit_action(), POLKIT_FLATPAK_SYSTEM_MANAGE);

    for unauth_bus in [":1.99", ":1.98"] {
        let err = h
            .engine
            .create_plan(unauth_bus, req.clone())
            .await
            .unwrap_err();
        assert!(matches!(err, AkrepError::PolkitDenied(_)));
        let insp_err = h
            .engine
            .inspect_system_flatpaks(unauth_bus)
            .await
            .unwrap_err();
        assert!(matches!(insp_err, AkrepError::PolkitDenied(_)));
    }

    // Revoking Polkit authorization between CreatePlan and CommitPlan also denies with zero changes
    let staged = h.engine.create_plan(":1.30", req).await.unwrap();
    h.auth.deny_all_for_bus(":1.30", 1000);
    let commit_err = h
        .engine
        .commit_plan(":1.30", &staged.plan_id, &staged.plan_sha256)
        .await
        .unwrap_err();
    assert!(matches!(commit_err, AkrepError::PolkitDenied(_)));
    assert!(!h
        .system_flatpak_dir
        .join("overrides/org.signal.Signal")
        .exists());
}

#[tokio::test]
async fn app_10_non_flatpak_native_app_marked_unsupported() {
    let h = setup_sprint2_harness();

    install_mock_flatpak_app(
        &h.system_flatpak_dir,
        "org.mozilla.firefox",
        "x86_64",
        "stable",
        "8888888888888888888888888888888888888888888888888888888888888888",
        "Firefox",
        "134.0",
        "[Application]\nname=org.mozilla.firefox\nruntime=org.freedesktop.Platform/x86_64/24.08\n\n[Context]\nshared=network;\n",
        false,
    );

    // Native RPM desktop entries + exported Flatpak desktop entry in /usr/share/applications
    fs::write(
        h.system_apps_dir.join("org.gnome.Calculator.desktop"),
        "[Desktop Entry]\nType=Application\nName=Calculator\nExec=/usr/bin/gnome-calculator\n",
    )
    .unwrap();
    fs::write(
        h.system_apps_dir.join("htop.desktop"),
        "[Desktop Entry]\nType=Application\nName=Htop\nExec=env TERM=xterm-256color /usr/bin/htop\n",
    )
    .unwrap();
    fs::write(
        h.user_apps_dir.join("custom-script.desktop"),
        "[Desktop Entry]\nType=Application\nName=Custom Tool\nExec=/usr/local/bin/custom-tool\n",
    )
    .unwrap();
    fs::write(
        h.system_apps_dir.join("org.mozilla.firefox.desktop"),
        "[Desktop Entry]\nType=Application\nName=Firefox\nExec=/usr/bin/flatpak run org.mozilla.firefox\nX-Flatpak=org.mozilla.firefox\n",
    )
    .unwrap();

    let inv = h.user_mgr.inspect_unified().unwrap();
    assert_eq!(
        inv.native_apps.len(),
        3,
        "Flatpak export must be excluded while system and user native .desktop apps are listed"
    );

    let calc = inv
        .find_native("org.gnome.Calculator")
        .expect("Calculator native app");
    assert_eq!(calc.exec_bin, "/usr/bin/gnome-calculator");
    assert!(!calc.origin.sandbox_supported());
    assert!(matches!(
        calc.origin,
        AppOrigin::NativePackage {
            sandbox_supported: false,
            ..
        }
    ));

    let htop = inv.find_native("htop.desktop").expect("htop native app");
    assert_eq!(htop.exec_bin, "/usr/bin/htop");
    assert!(!htop.origin.sandbox_supported());

    // Attempting sandbox override mutations on native apps is rejected as unsupported
    let sys_err = h
        .engine
        .create_plan(
            ":1.30",
            ActionRequest::FlatpakSetOverride {
                app_id: "org.gnome.Calculator".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.shared".to_string(), "!network".to_string())],
                removals: vec![],
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(sys_err, AkrepError::ForbiddenOperation(_)));
    assert!(sys_err.message().contains("sandbox_supported=false"));

    let usr_err = h
        .user_mgr
        .apply_user_override(
            "org.gnome.Calculator",
            &[("Context.shared".to_string(), "!network".to_string())],
            &[],
        )
        .unwrap_err();
    assert!(matches!(usr_err, AkrepError::ForbiddenOperation(_)));
    assert!(usr_err.message().contains("sandbox_supported=false"));
}

// ============================================================================
// Desktop Privacy Acceptance Tests (PRIV-01 .. PRIV-08)
// ============================================================================

#[tokio::test]
async fn priv_01_portal_interface_discovery() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    let sample_xml = r#"
        <!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
         "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
        <node>
          <interface name="org.freedesktop.DBus.Introspectable"/>
          <interface name="org.freedesktop.DBus.Properties"/>
          <interface name="org.freedesktop.portal.ScreenCast"/>
          <interface name="org.freedesktop.portal.RemoteDesktop"/>
          <interface name="org.freedesktop.portal.GlobalShortcuts"/>
          <interface name="org.freedesktop.portal.Camera"/>
          <interface name="org.freedesktop.portal.Usb"/>
        </node>
    "#;

    mock.set_portal_from_introspection_xml(
        sample_xml,
        Some((6, Some(1 | 2 | 4), Some(1 | 2 | 4))),
        Some((2, Some(1 | 2 | 4))),
        Some(2),
        Some((1, Some(true))),
        Some(1),
    );

    let report = mock.discover_portals().await.unwrap();
    assert_eq!(report.portal_service_availability, Availability::Available);
    for iface in [
        IFACE_PORTAL_SCREENCAST,
        IFACE_PORTAL_REMOTE_DESKTOP,
        IFACE_PORTAL_GLOBAL_SHORTCUTS,
        IFACE_PORTAL_CAMERA,
        IFACE_PORTAL_USB,
    ] {
        assert!(report.discovered_interfaces.contains(&iface.to_string()));
    }
    assert_eq!(report.screencast.version, Some(6));
    assert_eq!(report.remote_desktop.version, Some(2));
    assert_eq!(report.global_shortcuts.version, Some(2));
    assert_eq!(report.camera.version, Some(1));
    assert_eq!(report.camera.is_camera_present, Some(true));
    assert_eq!(report.usb.version, Some(1));
    assert_eq!(
        report.core_wayland_portals_severity(),
        AkrepSeverity::Protected
    );
}

#[tokio::test]
async fn priv_02_screencast_capability_detection() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    let xml = r#"<node><interface name="org.freedesktop.portal.ScreenCast"/></node>"#;

    // Monitor (1) + Window (2), Cursor Hidden (1) + Metadata (4)
    mock.set_portal_from_introspection_xml(
        xml,
        Some((5, Some(1 | 2), Some(1 | 4))),
        None,
        None,
        None,
        None,
    );
    let r1 = mock.discover_portals().await.unwrap();
    assert_eq!(r1.screencast.availability, Availability::Available);
    assert_eq!(r1.screencast.severity, AkrepSeverity::Protected);
    assert_eq!(
        r1.screencast.source_types,
        vec![ScreenCastSourceType::Monitor, ScreenCastSourceType::Window]
    );
    assert_eq!(
        r1.screencast.cursor_modes,
        vec![ScreenCastCursorMode::Hidden, ScreenCastCursorMode::Metadata]
    );

    // Zero source types mask flags Attention rather than Protected
    mock.set_portal_from_introspection_xml(
        xml,
        Some((5, Some(0), Some(1))),
        None,
        None,
        None,
        None,
    );
    let r2 = mock.discover_portals().await.unwrap();
    assert_eq!(r2.screencast.severity, AkrepSeverity::Attention);
    assert!(!r2.screencast.severity.is_protected());
}

#[tokio::test]
async fn priv_03_remotedesktop_capability_detection() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    let xml = r#"<node><interface name="org.freedesktop.portal.RemoteDesktop"/></node>"#;

    // Keyboard (1) + Pointer (2) + Touchscreen (4)
    mock.set_portal_from_introspection_xml(xml, None, Some((2, Some(1 | 2 | 4))), None, None, None);
    let r1 = mock.discover_portals().await.unwrap();
    assert_eq!(r1.remote_desktop.availability, Availability::Available);
    assert_eq!(r1.remote_desktop.severity, AkrepSeverity::Protected);
    assert_eq!(
        r1.remote_desktop.device_types,
        vec![
            RemoteDesktopDeviceType::Keyboard,
            RemoteDesktopDeviceType::Pointer,
            RemoteDesktopDeviceType::Touchscreen
        ]
    );

    // Zero device types mask flags Attention
    mock.set_portal_from_introspection_xml(xml, None, Some((2, Some(0))), None, None, None);
    let r2 = mock.discover_portals().await.unwrap();
    assert_eq!(r2.remote_desktop.severity, AkrepSeverity::Attention);
    assert!(!r2.remote_desktop.severity.is_protected());
}

#[tokio::test]
async fn priv_04_wayland_user_consent_preserved_no_silent_grant() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    let ctrl = PrivacySessionController::new(mock.clone());

    // 1. Silent grant or bypassing interactive compositor portal consent is strictly forbidden
    let silent_grant_err = ctrl
        .execute_action(PortalPrivacyAction::SilentGrantPermission {
            table: "screencast".to_string(),
            resource_id: "monitor-0".to_string(),
            app_id: "com.obsproject.Studio".to_string(),
            permissions: vec!["yes".to_string()],
        })
        .await
        .unwrap_err();
    assert!(matches!(
        silent_grant_err,
        AkrepError::ForbiddenOperation(_)
    ));

    let bypass_err = ctrl
        .execute_action(PortalPrivacyAction::BypassInteractiveConsent {
            portal_interface: IFACE_PORTAL_REMOTE_DESKTOP.to_string(),
            app_id: "org.remmina.Remmina".to_string(),
        })
        .await
        .unwrap_err();
    assert!(matches!(bypass_err, AkrepError::ForbiddenOperation(_)));

    // 2. Even revoking permissions or terminating streams requires explicit user_confirmed=true
    let unconfirmed_revoke = ctrl
        .execute_action(PortalPrivacyAction::RevokeStoredPermission {
            table: "camera".to_string(),
            resource_id: "camera".to_string(),
            app_id: "org.mozilla.firefox".to_string(),
            user_confirmed: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        unconfirmed_revoke,
        AkrepError::ForbiddenOperation(_)
    ));

    let unconfirmed_kill = ctrl
        .execute_action(PortalPrivacyAction::TerminateActiveStream {
            node_id: 101,
            user_confirmed: false,
        })
        .await
        .unwrap_err();
    assert!(matches!(
        unconfirmed_kill,
        AkrepError::ForbiddenOperation(_)
    ));

    // 3. Flatpak overrides cannot grant direct talk access to PermissionStore
    let escape_req = ActionRequest::FlatpakSetOverride {
        app_id: "com.obsproject.Studio".to_string(),
        scope: FlatpakScope::User,
        additions: vec![(
            "Session Bus Policy.org.freedesktop.impl.portal.PermissionStore".to_string(),
            "talk".to_string(),
        )],
        removals: vec![],
    };
    assert!(matches!(
        escape_req.validate_red_lines().unwrap_err(),
        AkrepError::ForbiddenOperation(_)
    ));
}

#[tokio::test]
async fn priv_05_permission_store_revocation() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    mock.set_permission_table_entries(
        "screencast",
        vec![
            PermissionStoreEntry {
                table: "screencast".to_string(),
                resource_id: "default".to_string(),
                app_id: "com.obsproject.Studio".to_string(),
                permissions: vec!["yes".to_string()],
            },
            PermissionStoreEntry {
                table: "screencast".to_string(),
                resource_id: "default".to_string(),
                app_id: "org.mozilla.firefox".to_string(),
                permissions: vec!["yes".to_string()],
            },
        ],
    );
    mock.set_permission_table_missing("devices");

    let ctrl = PrivacySessionController::new(mock.clone());
    let snap1 = ctrl.snapshot().await.unwrap();

    // Missing table ("devices") gracefully reports Unknown instead of failing
    let dev_tbl = snap1
        .permission_tables
        .iter()
        .find(|t| t.table == "devices")
        .unwrap();
    assert_eq!(dev_tbl.availability, Availability::Unknown);
    assert!(dev_tbl.entries.is_empty());

    let sc_tbl1 = snap1
        .permission_tables
        .iter()
        .find(|t| t.table == "screencast")
        .unwrap();
    assert_eq!(sc_tbl1.entries.len(), 2);

    // Revoke OBS Studio screencast permission
    ctrl.execute_action(PortalPrivacyAction::RevokeStoredPermission {
        table: "screencast".to_string(),
        resource_id: "default".to_string(),
        app_id: "com.obsproject.Studio".to_string(),
        user_confirmed: true,
    })
    .await
    .expect("revoke OBS permission");

    let sc_tbl2 = mock.query_permission_table("screencast").await.unwrap();
    assert_eq!(sc_tbl2.entries.len(), 1);
    assert_eq!(sc_tbl2.entries[0].app_id, "org.mozilla.firefox");
    assert_eq!(
        mock.revoked_permissions(),
        vec![(
            "screencast".to_string(),
            "default".to_string(),
            "com.obsproject.Studio".to_string()
        )]
    );
}

#[tokio::test]
async fn priv_06_missing_portal_reported_as_unknown_or_unavailable() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));

    // Case 1: Usb portal absent, Camera property unreadable (None -> Unknown)
    let partial_xml = r#"
        <node>
          <interface name="org.freedesktop.portal.ScreenCast"/>
          <interface name="org.freedesktop.portal.RemoteDesktop"/>
          <interface name="org.freedesktop.portal.Camera"/>
        </node>
    "#;
    mock.set_portal_from_introspection_xml(
        partial_xml,
        Some((5, Some(1 | 2), Some(1 | 2))),
        Some((2, Some(1 | 2))),
        None,
        None,
        None,
    );

    let r1 = mock.discover_portals().await.unwrap();
    assert_eq!(r1.usb.availability, Availability::Unavailable);
    assert_eq!(r1.usb.severity, AkrepSeverity::Unavailable);
    assert!(!r1.usb.severity.is_protected());

    assert_eq!(r1.global_shortcuts.availability, Availability::Unavailable);
    assert!(!r1.global_shortcuts.severity.is_protected());

    assert_eq!(r1.camera.availability, Availability::Unknown);
    assert_eq!(r1.camera.severity, AkrepSeverity::Unknown);
    assert!(!r1.camera.severity.is_protected());

    // Core Wayland severity combines Protected + Protected + Unknown => Unknown (never Protected!)
    assert_eq!(r1.core_wayland_portals_severity(), AkrepSeverity::Unknown);
    assert!(!r1.core_wayland_portals_severity().is_protected());

    // Case 2: Entire portal Desktop service missing/offline
    mock.set_portal_report(PortalDiscoveryReport::unavailable());
    let r2 = mock.discover_portals().await.unwrap();
    assert_eq!(r2.portal_service_availability, Availability::Unavailable);
    assert_eq!(
        r2.core_wayland_portals_severity(),
        AkrepSeverity::Unavailable
    );
    assert!(!r2.core_wayland_portals_severity().is_protected());
}

fn sample_multi_stream_pipewire_json() -> String {
    serde_json::json!([
        {
            "id": 40,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Audio/Source",
                    "node.name": "alsa_input.usb-mic",
                    "media.name": "Studio USB Microphone"
                }
            }
        },
        {
            "id": 41,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Audio/Sink",
                    "node.name": "alsa_output.speakers",
                    "media.name": "Speakers"
                }
            }
        },
        {
            "id": 50,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Video/Source",
                    "media.role": "Screen",
                    "node.name": "niri-screencast-source",
                    "media.name": "xdg-desktop-portal-gnome"
                }
            }
        },
        {
            "id": 60,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Video/Source",
                    "media.role": "Camera",
                    "device.api": "v4l2",
                    "node.name": "v4l2_input.webcam0",
                    "media.name": "HD Webcam"
                }
            }
        },
        {
            "id": 101,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Stream/Input/Audio",
                    "application.name": "Firefox",
                    "application.id": "org.mozilla.firefox",
                    "application.process.binary": "firefox",
                    "application.process.id": 4100,
                    "application.process.uid": 1000,
                    "media.name": "WebRTC Voice"
                }
            }
        },
        {
            "id": 102,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Stream/Input/Video",
                    "application.name": "OBS Studio",
                    "application.id": "com.obsproject.Studio",
                    "application.process.binary": "obs",
                    "application.process.id": 4200,
                    "application.process.uid": 1000,
                    "media.name": "PipeWire Screen Capture"
                }
            }
        },
        {
            "id": 103,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Stream/Input/Video",
                    "application.name": "Snapshot",
                    "application.id": "org.gnome.Snapshot",
                    "application.process.binary": "snapshot",
                    "application.process.id": 4300,
                    "application.process.uid": 1000,
                    "media.name": "Camera Preview"
                }
            }
        },
        {
            "id": 104,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "suspended",
                "props": {
                    "media.class": "Stream/Input/Audio",
                    "application.name": "PausedRecorder",
                    "application.process.uid": 1000,
                    "media.name": "Idle Capture"
                }
            }
        },
        {
            "id": 105,
            "type": "PipeWire:Interface:Node",
            "info": {
                "state": "running",
                "props": {
                    "media.class": "Stream/Input/Audio",
                    "application.name": "OtherUserApp",
                    "application.process.uid": 1001,
                    "media.name": "Other User Capture"
                }
            }
        },
        {
            "id": 201,
            "type": "PipeWire:Interface:Link",
            "info": { "output-node-id": 40, "input-node-id": 101, "state": "active" }
        },
        {
            "id": 202,
            "type": "PipeWire:Interface:Link",
            "info": { "output-node-id": 50, "input-node-id": 102, "state": "active" }
        },
        {
            "id": 203,
            "type": "PipeWire:Interface:Link",
            "info": { "output-node-id": 60, "input-node-id": 103, "state": "active" }
        },
        {
            "id": 204,
            "type": "PipeWire:Interface:Link",
            "info": { "output-node-id": 40, "input-node-id": 104, "state": "paused" }
        },
        {
            "id": 205,
            "type": "PipeWire:Interface:Link",
            "info": { "output-node-id": 40, "input-node-id": 105, "state": "active" }
        }
    ])
    .to_string()
}

#[tokio::test]
async fn priv_07_wayland_boundaries_enforced_invalid_node_rejected() {
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    mock.set_pipewire_dump_json(Some(sample_multi_stream_pipewire_json()));
    let ctrl = PrivacySessionController::new(mock.clone());

    let graph = mock.inspect_pipewire_graph().await.unwrap();
    // Nodes 101 (Mic), 102 (ScreenCast), 103 (Camera), and 105 (running stream) are detected; 104 (suspended) is excluded
    assert!(graph.active_streams.iter().any(|s| s.node_id == 101
        && s.kind == PrivacyStreamKind::Microphone
        && s.application_name == "Firefox"));
    assert!(graph.active_streams.iter().any(|s| s.node_id == 102
        && s.kind == PrivacyStreamKind::ScreenCast
        && s.process_binary.as_deref() == Some("obs")));
    assert!(graph.active_streams.iter().any(|s| s.node_id == 103
        && s.kind == PrivacyStreamKind::Camera
        && s.application_name == "Snapshot"));
    assert!(!graph.active_streams.iter().any(|s| s.node_id == 104));

    // Reject core node 0, unknown node 9999, hardware mic 40, speaker sink 41, compositor portal 50, suspended node 104, and foreign uid node 105
    assert!(matches!(
        ctrl.execute_action(PortalPrivacyAction::TerminateActiveStream {
            node_id: 0,
            user_confirmed: true
        })
        .await
        .unwrap_err(),
        AkrepError::InvalidInput(_)
    ));
    assert!(matches!(
        ctrl.execute_action(PortalPrivacyAction::TerminateActiveStream {
            node_id: 9999,
            user_confirmed: true
        })
        .await
        .unwrap_err(),
        AkrepError::InvalidInput(_)
    ));
    for protected_hw_or_portal in [40u32, 41u32, 50u32, 60u32] {
        let err = ctrl
            .execute_action(PortalPrivacyAction::TerminateActiveStream {
                node_id: protected_hw_or_portal,
                user_confirmed: true,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, AkrepError::ForbiddenOperation(_)));
    }
    assert!(matches!(
        ctrl.execute_action(PortalPrivacyAction::TerminateActiveStream {
            node_id: 104,
            user_confirmed: true
        })
        .await
        .unwrap_err(),
        AkrepError::InvalidInput(_)
    ));
    assert!(matches!(
        ctrl.execute_action(PortalPrivacyAction::TerminateActiveStream {
            node_id: 105,
            user_confirmed: true
        })
        .await
        .unwrap_err(),
        AkrepError::ForbiddenOperation(_)
    ));
    assert!(mock.destroyed_nodes().is_empty());

    // Valid termination of active user-owned ScreenCast stream 102 succeeds cleanly
    ctrl.execute_action(PortalPrivacyAction::TerminateActiveStream {
        node_id: 102,
        user_confirmed: true,
    })
    .await
    .expect("terminate active screencast node 102");
    assert_eq!(mock.destroyed_nodes(), vec![102]);

    let graph_after = mock.inspect_pipewire_graph().await.unwrap();
    assert!(!graph_after.active_streams.iter().any(|s| s.node_id == 102));
    assert!(graph_after.active_streams.iter().any(|s| s.node_id == 101));
    assert!(graph_after.active_streams.iter().any(|s| s.node_id == 103));
}

#[tokio::test]
async fn priv_08_daemon_disconnect_leaves_portal_sessions_intact() {
    let h = setup_sprint2_harness();
    let mock = Arc::new(MockPortalPrivacyBackend::new_wayland_desktop(1000));
    mock.set_pipewire_dump_json(Some(sample_multi_stream_pipewire_json()));
    mock.set_permission_table_entries(
        "screencast",
        vec![PermissionStoreEntry {
            table: "screencast".to_string(),
            resource_id: "session-live-1".to_string(),
            app_id: "com.obsproject.Studio".to_string(),
            permissions: vec!["yes".to_string()],
        }],
    );

    // 1. Inspect active sessions, then drop controller AND simulate akrep-privd idle exit / disconnect
    {
        let transient_ctrl = PrivacySessionController::new(mock.clone());
        let initial_snap = transient_ctrl.snapshot().await.unwrap();
        assert!(initial_snap.active_streams.iter().any(|s| s.node_id == 102));

        let idle = Arc::new(IdleTracker::new());
        assert!(idle.is_idle_expired(Duration::ZERO));
        drop(h.engine);
    }

    // 2. Zero PipeWire nodes were destroyed and zero PermissionStore grants were revoked
    assert!(mock.destroyed_nodes().is_empty());
    assert!(mock.revoked_permissions().is_empty());

    // 3. Reconnecting a fresh PrivacySessionController sees all portal permissions and running streams intact
    let reconnected_ctrl = PrivacySessionController::new(mock);
    let snap_after = reconnected_ctrl.snapshot().await.unwrap();
    assert!(snap_after
        .active_streams
        .iter()
        .any(|s| s.node_id == 101 && s.state == "running"));
    assert!(snap_after
        .active_streams
        .iter()
        .any(|s| s.node_id == 102 && s.state == "running"));
    let sc_tbl = snap_after
        .permission_tables
        .iter()
        .find(|t| t.table == "screencast")
        .unwrap();
    assert_eq!(sc_tbl.entries.len(), 1);
    assert_eq!(sc_tbl.entries[0].resource_id, "session-live-1");
}
