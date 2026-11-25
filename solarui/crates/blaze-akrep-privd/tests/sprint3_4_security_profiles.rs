use blaze_akrep_common::{
    build_cidr_port_rich_rule, is_rfc1918_ipv4_cidr, validate_clamd_endpoint,
    validate_scan_target_path, ActionPlan, ActionRequest, AkrepError, DeviceSecurityStatus,
    FirewallProto, FlatpakScope, MockFlatpakRuntimeProbe, ProfileCatalogResponse,
    QuarantineInventory, RulePersistence, ScanJobState, ScanProgress, ScanRequest, ScanVerdict,
    ScannerEngineState, ScannerStatusReport, SecureBootState, SecurityProfileSpec,
    SignatureFreshness, TransactionState, POLKIT_PACKAGES_INSTALL, POLKIT_PROFILE_APPLY,
    POLKIT_QUARANTINE_MANAGE, POLKIT_SELINUX_DIAGNOSE,
};
use blaze_akrep_privd::{
    build_eicar_test_bytes, build_polkit_subject, decode_qpkg_and_hash, inspect_kernel_lockdown_at,
    inspect_secure_boot_at, inspect_signature_db_at, inspect_tpm_at, inspect_usb_devices_at,
    parse_rpm_signature_lines, AvScanBackend, ClamdUnixSocketScanner, CompositeMutationBackend,
    DeviceSecurityAdapter, FirewallAdapter, FirewallBackend, LinuxDeviceSecurityOps,
    MockAuthorizer, MockClamScanner, MockDeviceSecurityOps, MockFirewallBackend,
    MockScannerInstallerOps, MockSelinuxOps, ProfileAdapter, QuarantineAdapter, ScannerAdapter,
    SelinuxAdapter, SelinuxSysOps, SystemFlatpakAdapter, TransactionEngine,
    EFI_SECURE_BOOT_VAR_REL, QPKG_MAGIC,
};
use blaze_sentinel_core::{sha256_hex, AkrepSeverity, Availability};
use chrono::{Duration as ChronoDuration, Utc};
use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixListener;

fn official_profiles_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../blazeos_custom_apps/usr/share/akrep/profiles")
        .canonicalize()
        .expect("official profiles dir must exist")
}

struct Sprint34Harness {
    root_tmp: TempDir,
    state_dir: PathBuf,
    work_dir: PathBuf,
    auth: Arc<MockAuthorizer>,
    selinux_ops: Arc<MockSelinuxOps>,
    firewall_backend: Arc<MockFirewallBackend>,
    scanner_backend: Arc<MockClamScanner>,
    installer_ops: Arc<MockScannerInstallerOps>,
    device_ops: Arc<MockDeviceSecurityOps>,
    engine: TransactionEngine,
}

fn setup_harness_with_profiles_dir(profiles_dir: PathBuf) -> Sprint34Harness {
    let root_tmp = TempDir::new().expect("tempdir");
    let state_dir = root_tmp.path().join("var_lib_akrep");
    let work_dir = root_tmp.path().join("workspace_samples");
    let sys_flatpak_dir = root_tmp.path().join("var_lib_flatpak");
    let sys_apps_dir = root_tmp.path().join("usr_share_applications");
    fs::create_dir_all(&work_dir).expect("mkdir work_dir");
    fs::create_dir_all(&sys_flatpak_dir).expect("mkdir flatpak");
    fs::create_dir_all(&sys_apps_dir).expect("mkdir apps");

    let auth = Arc::new(MockAuthorizer::new());
    auth.allow_all_for_bus(":1.40", 1000);

    let selinux_ops = Arc::new(MockSelinuxOps::new_fedora_enforcing());
    let firewall_backend = Arc::new(MockFirewallBackend::new_fedora_workstation());
    let scanner_backend = Arc::new(MockClamScanner::new_healthy());
    let installer_ops = Arc::new(MockScannerInstallerOps::new(false));
    let device_ops = Arc::new(MockDeviceSecurityOps::new_healthy_uefi());

    let selinux = Arc::new(SelinuxAdapter::new(selinux_ops.clone()));
    let firewall = Arc::new(FirewallAdapter::new(firewall_backend.clone()));
    let flatpak = Arc::new(SystemFlatpakAdapter::with_roots(
        sys_flatpak_dir,
        sys_apps_dir,
        Arc::new(MockFlatpakRuntimeProbe::new()),
    ));
    let device_security = Arc::new(DeviceSecurityAdapter::new(device_ops.clone()));
    let scanner = Arc::new(ScannerAdapter::with_options(
        state_dir.clone(),
        scanner_backend.clone(),
        installer_ops.clone(),
        true,
    ));
    let quarantine = Arc::new(QuarantineAdapter::new_lazy(state_dir.clone()));
    let public_cache_status = root_tmp.path().join("var_cache_akrep/active_profile.json");
    let profiles = Arc::new(
        ProfileAdapter::with_paths(
            profiles_dir,
            state_dir.clone(),
            selinux.clone(),
            firewall.clone(),
        )
        .with_public_status_path(public_cache_status),
    );

    let composite = Arc::new(CompositeMutationBackend::new_full(
        selinux,
        firewall,
        flatpak,
        device_security,
        scanner,
        quarantine,
        profiles,
    ));

    let engine =
        TransactionEngine::new_with_options(state_dir.clone(), auth.clone(), composite, true)
            .expect("engine init");

    Sprint34Harness {
        root_tmp,
        state_dir,
        work_dir,
        auth,
        selinux_ops,
        firewall_backend,
        scanner_backend,
        installer_ops,
        device_ops,
        engine,
    }
}

fn setup_sprint34_harness() -> Sprint34Harness {
    setup_harness_with_profiles_dir(official_profiles_dir())
}

async fn spawn_mock_clamd_instream_server(
    sock_path: PathBuf,
    per_chunk_delay: Duration,
) -> tokio::task::JoinHandle<()> {
    let listener = UnixListener::bind(&sock_path).expect("bind mock clamd unix socket");
    let eicar = build_eicar_test_bytes();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let eicar_ref = eicar.clone();
            tokio::spawn(async move {
                let mut cmd = [0u8; 10];
                if stream.read_exact(&mut cmd).await.is_err() {
                    return;
                }
                if &cmd != b"zINSTREAM\0" {
                    let _ = stream.write_all(b"stream: UNKNOWN COMMAND ERROR\0").await;
                    return;
                }

                let mut collected = Vec::new();
                loop {
                    let mut len_buf = [0u8; 4];
                    if stream.read_exact(&mut len_buf).await.is_err() {
                        return;
                    }
                    let chunk_len = u32::from_be_bytes(len_buf) as usize;
                    if chunk_len == 0 {
                        break;
                    }
                    if !per_chunk_delay.is_zero() {
                        tokio::time::sleep(per_chunk_delay).await;
                    }
                    let mut chunk = vec![0u8; chunk_len];
                    if stream.read_exact(&mut chunk).await.is_err() {
                        return;
                    }
                    collected.extend_from_slice(&chunk);
                }

                let infected = collected
                    .windows(eicar_ref.len())
                    .any(|w| w == eicar_ref.as_slice());
                let reply: &[u8] = if infected {
                    b"stream: Win.Test.EICAR_HDB-1 FOUND\0"
                } else {
                    b"stream: OK\0"
                };
                let _ = stream.write_all(reply).await;
                let _ = stream.flush().await;
            });
        }
    })
}

fn set_file_mtime_secs_ago(path: &Path, secs_ago: u64) {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("unix epoch");
    let target_secs = now.as_secs().saturating_sub(secs_ago) as libc::time_t;
    let c_path = CString::new(path.as_os_str().as_bytes()).expect("c_path");
    let times = [
        libc::timespec {
            tv_sec: target_secs,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: target_secs,
            tv_nsec: 0,
        },
    ];
    // SAFETY: `c_path` is valid NUL-terminated path and `times` has 2 `timespec` structs.
    let rc = unsafe {
        libc::utimensat(
            libc::AT_FDCWD,
            c_path.as_ptr(),
            times.as_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    assert_eq!(rc, 0, "utimensat failed");
}

// ============================================================================
// Device Security Inspector Tests
// ============================================================================

#[tokio::test]
async fn dev_01_secure_boot_tpm2_lockdown_and_never_false_protected() {
    let tmp = TempDir::new().unwrap();
    let sys = tmp.path().join("sys");
    let proc_dir = tmp.path().join("proc");
    let dev = tmp.path().join("dev");
    let etc = tmp.path().join("etc");
    fs::create_dir_all(&sys).unwrap();
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&dev).unwrap();
    fs::create_dir_all(&etc).unwrap();

    // 1. Legacy BIOS (missing /sys/firmware/efi) -> Unavailable, NEVER Protected
    let sb_legacy = inspect_secure_boot_at(&sys);
    assert_eq!(sb_legacy.state, SecureBootState::LegacyBios);
    assert_eq!(sb_legacy.severity, AkrepSeverity::Unavailable);
    assert_eq!(sb_legacy.availability, Availability::Unavailable);
    assert!(!sb_legacy.severity.is_protected());

    // 2. Virtual OVMF UEFI without SecureBoot variable -> Unavailable, NEVER Protected
    fs::create_dir_all(sys.join("firmware/efi/efivars")).unwrap();
    fs::create_dir_all(sys.join("class/dmi/id")).unwrap();
    fs::write(sys.join("class/dmi/id/bios_vendor"), "EDK II / OVMF\n").unwrap();
    let sb_ovmf_missing = inspect_secure_boot_at(&sys);
    assert_eq!(sb_ovmf_missing.state, SecureBootState::UnsupportedFirmware);
    assert!(!sb_ovmf_missing.severity.is_protected());

    // 3. UEFI Secure Boot disabled (last byte = 0) -> Attention, NEVER Protected
    let sb_var_path = sys.join(EFI_SECURE_BOOT_VAR_REL);
    fs::write(&sb_var_path, [0x06, 0x00, 0x00, 0x00, 0x00]).unwrap();
    let sb_off = inspect_secure_boot_at(&sys);
    assert_eq!(sb_off.state, SecureBootState::Disabled);
    assert_eq!(sb_off.secure_boot_enabled, Some(false));
    assert_eq!(sb_off.severity, AkrepSeverity::Attention);
    assert!(!sb_off.severity.is_protected());

    // 4. UEFI Secure Boot enabled (last byte = 1) -> Protected
    fs::write(&sb_var_path, [0x06, 0x00, 0x00, 0x00, 0x01]).unwrap();
    let sb_on = inspect_secure_boot_at(&sys);
    assert_eq!(sb_on.state, SecureBootState::Enabled);
    assert_eq!(sb_on.secure_boot_enabled, Some(true));
    assert_eq!(sb_on.severity, AkrepSeverity::Protected);

    // 5. TPM missing vs 1.2 vs 2.0
    let tpm_missing = inspect_tpm_at(&sys, &dev);
    assert!(!tpm_missing.tpm2_available);
    assert_eq!(tpm_missing.availability, Availability::Unavailable);

    fs::create_dir_all(sys.join("class/tpm/tpm0")).unwrap();
    fs::write(sys.join("class/tpm/tpm0/tpm_version_major"), "1\n").unwrap();
    let tpm_v1 = inspect_tpm_at(&sys, &dev);
    assert!(!tpm_v1.tpm2_available);
    assert_eq!(tpm_v1.severity, AkrepSeverity::Unavailable);

    fs::write(sys.join("class/tpm/tpm0/tpm_version_major"), "2\n").unwrap();
    let tpm_v2 = inspect_tpm_at(&sys, &dev);
    assert!(tpm_v2.tpm2_available);
    assert_eq!(tpm_v2.severity, AkrepSeverity::Protected);

    // 6. Kernel module signature enforcement & lockdown
    fs::create_dir_all(sys.join("module/module/parameters")).unwrap();
    fs::create_dir_all(sys.join("kernel/security")).unwrap();
    fs::create_dir_all(sys.join("class/iommu/dmar0")).unwrap();
    fs::write(
        proc_dir.join("cmdline"),
        "BOOT_IMAGE=/vmlinuz quiet module.sig_enforce=1\n",
    )
    .unwrap();
    fs::write(sys.join("module/module/parameters/sig_enforce"), "Y\n").unwrap();
    fs::write(
        sys.join("kernel/security/lockdown"),
        "none [integrity] confidentiality\n",
    )
    .unwrap();
    let kd = inspect_kernel_lockdown_at(&proc_dir, &sys);
    assert!(kd.module_sig_enforced);
    assert_eq!(kd.lockdown_mode, "integrity");
    assert!(kd.iommu_active);
    assert_eq!(kd.severity, AkrepSeverity::Protected);
}

#[tokio::test]
async fn dev_02_usb_audit_unauthorized_and_hid_spoof_anomalies_and_rpm_signatures() {
    let tmp = TempDir::new().unwrap();
    let sys = tmp.path().join("sys");
    let usb = sys.join("bus/usb/devices");
    fs::create_dir_all(&usb).unwrap();

    // Normal keyboard (HID 03 only, authorized=1)
    let kb = usb.join("1-1");
    let kb_if = usb.join("1-1:1.0");
    fs::create_dir_all(&kb).unwrap();
    fs::create_dir_all(&kb_if).unwrap();
    fs::write(kb.join("idVendor"), "046d\n").unwrap();
    fs::write(kb.join("idProduct"), "c52b\n").unwrap();
    fs::write(kb.join("manufacturer"), "Logitech\n").unwrap();
    fs::write(kb.join("product"), "USB Receiver\n").unwrap();
    fs::write(kb.join("bDeviceClass"), "00\n").unwrap();
    fs::write(kb.join("authorized"), "1\n").unwrap();
    fs::write(kb_if.join("bInterfaceClass"), "03\n").unwrap();

    // Composite Mass-Storage (08) + HID (03) BadUSB spoof + unauthorized device
    let bad = usb.join("1-2");
    let bad_if0 = usb.join("1-2:1.0");
    let bad_if1 = usb.join("1-2:1.1");
    let unauth = usb.join("1-3");
    for d in [&bad, &bad_if0, &bad_if1, &unauth] {
        fs::create_dir_all(d).unwrap();
    }
    fs::write(bad.join("idVendor"), "1234\n").unwrap();
    fs::write(bad.join("idProduct"), "5678\n").unwrap();
    fs::write(bad.join("product"), "USB Flash Drive\n").unwrap();
    fs::write(bad.join("bDeviceClass"), "00\n").unwrap();
    fs::write(bad.join("authorized"), "1\n").unwrap();
    fs::write(bad_if0.join("bInterfaceClass"), "08\n").unwrap();
    fs::write(bad_if1.join("bInterfaceClass"), "03\n").unwrap();

    fs::write(unauth.join("idVendor"), "dead\n").unwrap();
    fs::write(unauth.join("idProduct"), "beef\n").unwrap();
    fs::write(unauth.join("authorized"), "0\n").unwrap();

    let usb_rep = inspect_usb_devices_at(&sys);
    assert_eq!(usb_rep.devices.len(), 3);
    assert_eq!(usb_rep.unauthorized_count, 1);
    assert_eq!(usb_rep.anomaly_count, 2);
    assert_eq!(usb_rep.severity, AkrepSeverity::Attention);
    let bad_entry = usb_rep
        .devices
        .iter()
        .find(|d| d.sysfs_name == "1-2")
        .unwrap();
    assert!(bad_entry
        .anomalies
        .contains(&"hid_spoof_anomaly".to_string()));

    // RPM signature parser check (RSA + EdDSA/OPENPGP + gpg-pubkey ignored + unsigned flagged)
    let rpm_sample = "\
gpg-pubkey|(none)|(none)
blaze-release|(none)|EdDSA/SHA512, Wed 25 Nov 2026, Key ID 1234567890abcdef
sudo|RSA/SHA256, Mon 01 Nov 2026, Key ID fedcba0987654321|(none)
unsigned-test-pkg|(none)|(none)
";
    let (total, signed, unsigned) = parse_rpm_signature_lines(rpm_sample);
    assert_eq!(total, 3);
    assert_eq!(signed, 2);
    assert_eq!(unsigned, vec!["unsigned-test-pkg".to_string()]);

    // Verify D-Bus engine endpoint InspectDeviceSecurity enforces Polkit
    let h = setup_sprint34_harness();
    h.auth.deny_all_for_bus(":1.99", 1005);
    let denied = h.engine.inspect_device_security(":1.99").await.unwrap_err();
    assert!(matches!(denied, AkrepError::PolkitDenied(_)));

    let ok_json = h.engine.inspect_device_security(":1.40").await.unwrap();
    let mut status: DeviceSecurityStatus = serde_json::from_str(&ok_json).unwrap();
    assert_eq!(status.overall_severity, AkrepSeverity::Protected);
    assert!(status.tpm.tpm2_available);

    status.overall_severity = AkrepSeverity::Attention;
    h.device_ops.set_status(status);
    let updated_json = h.engine.inspect_device_security(":1.40").await.unwrap();
    let updated: DeviceSecurityStatus = serde_json::from_str(&updated_json).unwrap();
    assert_eq!(updated.overall_severity, AkrepSeverity::Attention);

    // Ensure LinuxDeviceSecurityOps works cleanly against synthetic roots without panicking
    let proc_dir = tmp.path().join("proc");
    let dev_dir = tmp.path().join("dev");
    let etc_dir = tmp.path().join("etc");
    let canary = tmp.path().join(".blaze_canary");
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&dev_dir).unwrap();
    fs::create_dir_all(&etc_dir).unwrap();
    let linux_ops = LinuxDeviceSecurityOps::with_roots(sys, proc_dir, dev_dir, etc_dir, canary);
    let _ = DeviceSecurityAdapter::new(Arc::new(linux_ops))
        .inspect_device_security()
        .await
        .unwrap();
}

// ============================================================================
// Threat Scanner & Armored Quarantine Acceptance Tests (THR-01 .. THR-12)
// ============================================================================

#[tokio::test]
async fn thr_01_clean_file_scan() {
    let h = setup_sprint34_harness();
    let sock_path = h.root_tmp.path().join("clamd-clean.sock");
    let server = spawn_mock_clamd_instream_server(sock_path.clone(), Duration::ZERO).await;

    let clean_file = h.work_dir.join("benign_document.txt");
    let content = b"BlazeOS Akrep v0.4 clean diagnostic text payload.\n";
    fs::write(&clean_file, content).unwrap();

    let req = ScanRequest {
        target_path: clean_file.to_string_lossy().to_string(),
        recursive: false,
        socket_override: Some(sock_path.to_string_lossy().to_string()),
        synchronous: true,
    };
    let resp_json = h
        .engine
        .start_scan(":1.40", &serde_json::to_string(&req).unwrap())
        .await
        .unwrap();
    let progress: ScanProgress = serde_json::from_str(&resp_json).unwrap();

    assert_eq!(progress.state, ScanJobState::Completed);
    assert_eq!(progress.scanned_files, 1);
    assert_eq!(progress.total_files, 1);
    assert_eq!(progress.percent, 100);
    assert!(progress.findings.is_empty());
    assert_eq!(fs::read(&clean_file).unwrap(), content);

    let history_path = h.state_dir.join("scan_history.json");
    assert!(history_path.is_file());
    let hist_mode = fs::metadata(&history_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(hist_mode, 0o600);

    server.abort();
}

#[tokio::test]
async fn thr_02_eicar_test_string_detected_over_unix_instream() {
    let h = setup_sprint34_harness();
    let sock_path = h.root_tmp.path().join("clamd-eicar.sock");
    let server = spawn_mock_clamd_instream_server(sock_path.clone(), Duration::ZERO).await;

    let eicar_bytes = build_eicar_test_bytes();
    assert_eq!(eicar_bytes.len(), 68);
    let sample_file = h.work_dir.join("eicar_sample.com");
    fs::write(&sample_file, &eicar_bytes).unwrap();
    fs::write(h.work_dir.join("clean_readme.txt"), b"hello world").unwrap();

    // Direct ClamdUnixSocketScanner zINSTREAM verification
    let db_dir = h.root_tmp.path().join("clamav_db");
    fs::create_dir_all(&db_dir).unwrap();
    fs::write(
        db_dir.join("daily.cld"),
        b"ClamAV-VDB:25 Nov 2026 12-00 +0000:27455:100000:90:...",
    )
    .unwrap();
    let real_scanner = ClamdUnixSocketScanner::with_config(
        &[sock_path.to_string_lossy().to_string()],
        PathBuf::from("/nonexistent/clamscan"),
        db_dir,
    )
    .unwrap();
    let cancel = AtomicBool::new(false);
    let verdict = real_scanner
        .scan_file_stream(&sample_file, &cancel)
        .await
        .unwrap();
    assert_eq!(
        verdict,
        ScanVerdict::Infected {
            signature: "Win.Test.EICAR_HDB-1".to_string()
        }
    );

    // Full directory scan through TransactionEngine::start_scan
    let req = ScanRequest {
        target_path: h.work_dir.to_string_lossy().to_string(),
        recursive: true,
        socket_override: Some(sock_path.to_string_lossy().to_string()),
        synchronous: true,
    };
    let resp_json = h
        .engine
        .start_scan(":1.40", &serde_json::to_string(&req).unwrap())
        .await
        .unwrap();
    let progress: ScanProgress = serde_json::from_str(&resp_json).unwrap();

    assert_eq!(progress.state, ScanJobState::Completed);
    assert_eq!(progress.scanned_files, 2);
    assert_eq!(progress.findings.len(), 1);
    assert_eq!(
        progress.findings[0].threat_name.as_deref(),
        Some("Win.Test.EICAR_HDB-1")
    );
    // Scanner MUST NEVER auto-delete flagged files
    assert!(sample_file.is_file());

    server.abort();
}

#[tokio::test]
async fn thr_03_scan_cancellation() {
    let h = setup_sprint34_harness();
    h.scanner_backend
        .set_per_file_delay(Duration::from_millis(80));

    for idx in 0..25 {
        fs::write(
            h.work_dir.join(format!("blob_{:02}.dat", idx)),
            vec![b'A'; 1024],
        )
        .unwrap();
    }

    let req = ScanRequest {
        target_path: h.work_dir.to_string_lossy().to_string(),
        recursive: true,
        socket_override: None,
        synchronous: false,
    };
    let started_json = h
        .engine
        .start_scan(":1.40", &serde_json::to_string(&req).unwrap())
        .await
        .unwrap();
    let started: ScanProgress = serde_json::from_str(&started_json).unwrap();

    tokio::time::sleep(Duration::from_millis(120)).await;
    let cancelled_json = h
        .engine
        .cancel_scan(":1.40", &started.job_id)
        .await
        .unwrap();
    let cancelled: ScanProgress = serde_json::from_str(&cancelled_json).unwrap();
    assert_eq!(cancelled.state, ScanJobState::Cancelled);
    assert!(cancelled.scanned_files < 25);

    tokio::time::sleep(Duration::from_millis(120)).await;
    let status_after: ScanProgress = serde_json::from_str(
        &h.engine
            .get_scan_status(":1.40", &started.job_id)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status_after.state, ScanJobState::Cancelled);
}

#[test]
fn thr_04_fresh_signature_db_healthy() {
    let tmp = TempDir::new().unwrap();
    let daily = tmp.path().join("daily.cld");
    fs::write(
        &daily,
        b"ClamAV-VDB:25 Nov 2026 10-00 +0000:27460:2500000:90:...",
    )
    .unwrap();
    set_file_mtime_secs_ago(&daily, 2 * 86_400);

    let status = inspect_signature_db_at(tmp.path(), SystemTime::now());
    assert_eq!(status.freshness, SignatureFreshness::Healthy);
    assert_eq!(status.severity, AkrepSeverity::Protected);
    assert_eq!(status.availability, Availability::Available);
    assert_eq!(status.version, Some(27460));
    assert_eq!(status.age_days, Some(2));
}

#[test]
fn thr_05_stale_signature_db_warning() {
    let tmp = TempDir::new().unwrap();
    let daily = tmp.path().join("daily.cvd");
    fs::write(
        &daily,
        b"ClamAV-VDB:01 Nov 2026 10-00 +0000:27400:2400000:90:...",
    )
    .unwrap();
    set_file_mtime_secs_ago(&daily, 10 * 86_400);

    let status = inspect_signature_db_at(tmp.path(), SystemTime::now());
    assert_eq!(status.freshness, SignatureFreshness::Warning);
    assert_eq!(status.severity, AkrepSeverity::Attention);
    assert_eq!(status.availability, Availability::Available);
    assert!(status.age_days.unwrap_or(0) >= 10);
}

#[tokio::test]
async fn thr_06_missing_clamav_reported_unavailable_optional() {
    let tmp = TempDir::new().unwrap();
    let scanner = ClamdUnixSocketScanner::with_config(
        &[tmp.path().join("absent.sock").to_string_lossy().to_string()],
        tmp.path().join("absent-clamscan"),
        tmp.path().join("absent-db"),
    )
    .unwrap();
    let installer = Arc::new(MockScannerInstallerOps::new(false));
    let adapter = ScannerAdapter::new(
        tmp.path().join("state"),
        Arc::new(scanner),
        installer.clone(),
    );

    let report = adapter.inspect_status().unwrap();
    assert_eq!(
        report.engine_state,
        ScannerEngineState::Unavailable {
            install_hint: "clamav, clamd, clamav-freshclam".to_string()
        }
    );
    assert!(!report.realtime_protection_active);
    assert_eq!(report.availability, Availability::Unavailable);
    assert_eq!(report.severity, AkrepSeverity::Unavailable);

    // Also test optional installer transaction via TransactionEngine
    let h = setup_sprint34_harness();
    h.scanner_backend
        .set_engine_state(ScannerEngineState::Unavailable {
            install_hint: "clamav, clamd, clamav-freshclam".to_string(),
        });
    let status_json = h.engine.inspect_scanner(":1.40").await.unwrap();
    let status_rep: ScannerStatusReport = serde_json::from_str(&status_json).unwrap();
    assert!(!status_rep.realtime_protection_active);
    assert_eq!(status_rep.availability, Availability::Unavailable);

    let sample = h.work_dir.join("file.txt");
    fs::write(&sample, b"abc").unwrap();
    let scan_err = h
        .engine
        .start_scan(
            ":1.40",
            &serde_json::to_string(&ScanRequest {
                target_path: sample.to_string_lossy().to_string(),
                recursive: false,
                socket_override: None,
                synchronous: true,
            })
            .unwrap(),
        )
        .await
        .unwrap_err();
    assert!(matches!(scan_err, AkrepError::BackendUnavailable(_)));

    let install_plan = h
        .engine
        .create_plan(":1.40", ActionRequest::InstallOptionalScanner)
        .await
        .unwrap();
    assert_eq!(install_plan.polkit_action, POLKIT_PACKAGES_INSTALL);
    let rec = h
        .engine
        .commit_plan(":1.40", &install_plan.plan_id, &install_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);
    assert_eq!(
        h.installer_ops.recorded_ops(),
        vec!["dnf5 install -y clamav clamd clamav-freshclam".to_string()]
    );
}

#[tokio::test]
async fn thr_07_quarantine_preserves_bytes_and_metadata() {
    let h = setup_sprint34_harness();
    let suspect_path = h.work_dir.join("suspicious_payload.bin");
    let raw_payload = build_eicar_test_bytes();
    let expected_sha = sha256_hex(&raw_payload);
    fs::write(&suspect_path, &raw_payload).unwrap();
    fs::set_permissions(&suspect_path, fs::Permissions::from_mode(0o750)).unwrap();

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: suspect_path.to_string_lossy().to_string(),
                threat_name: "Win.Test.EICAR_HDB-1".to_string(),
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);
    assert!(!suspect_path.exists(), "original file must be unlinked");

    let inv: QuarantineInventory =
        serde_json::from_str(&h.engine.list_quarantine(":1.40").await.unwrap()).unwrap();
    assert_eq!(inv.entries.len(), 1);
    let meta = &inv.entries[0];
    assert_eq!(meta.original_path, suspect_path.to_string_lossy());
    assert_eq!(meta.original_mode & 0o777, 0o750);
    assert_eq!(meta.sha256, expected_sha);
    assert_eq!(meta.size_bytes, raw_payload.len() as u64);
    assert_eq!(meta.threat_name, "Win.Test.EICAR_HDB-1");
    assert_eq!(meta.actor_uid, 1000);
    assert!(!meta.restored);

    let qpkg_path = h
        .state_dir
        .join("quarantine")
        .join(format!("{}.akrep-qpkg", meta.quarantine_id));
    let qpkg_bytes = fs::read(&qpkg_path).unwrap();
    assert_eq!(&qpkg_bytes[..8], QPKG_MAGIC);
    // Ensure raw payload bytes are XOR-obfuscated and never sit unmasked in quarantine
    assert!(
        !qpkg_bytes
            .windows(raw_payload.len())
            .any(|w| w == raw_payload.as_slice()),
        "quarantine envelope must XOR-mask threat payload bytes"
    );
    let qpkg_mode = fs::metadata(&qpkg_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        qpkg_mode, 0o600,
        "quarantine envelope must be non-executable 0600"
    );

    let (decoded_sha, decoded_len) = decode_qpkg_and_hash(&qpkg_path, None).unwrap();
    assert_eq!(decoded_sha, expected_sha);
    assert_eq!(decoded_len, raw_payload.len() as u64);
}

#[tokio::test]
async fn thr_08_quarantine_restore_verifies_sha256() {
    let h = setup_sprint34_harness();
    let target = h.work_dir.join("false_positive_tool.bin");
    let payload = b"ELF_BENIGN_BINARY_PAYLOAD_1234567890";
    let expected_sha = sha256_hex(payload);
    fs::write(&target, payload).unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();

    let q_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: target.to_string_lossy().to_string(),
                threat_name: "Heuristics.FalsePositive".to_string(),
            },
        )
        .await
        .unwrap();
    let q_rec = h
        .engine
        .commit_plan(":1.40", &q_plan.plan_id, &q_plan.plan_sha256)
        .await
        .unwrap();
    let qid = q_rec
        .target_state
        .get("quarantine_id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // 1. Corrupt 1 byte inside .akrep-qpkg payload -> restore must fail SHA-256 verification and NOT place corrupt file
    let qpkg_path = h
        .state_dir
        .join("quarantine")
        .join(format!("{}.akrep-qpkg", qid));
    let orig_qpkg = fs::read(&qpkg_path).unwrap();
    let r_plan_bad = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::RestoreQuarantine {
                quarantine_id: qid.clone(),
            },
        )
        .await
        .unwrap();
    let mut tampered = orig_qpkg.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xff;
    fs::write(&qpkg_path, &tampered).unwrap();
    fs::set_permissions(&qpkg_path, fs::Permissions::from_mode(0o600)).unwrap();

    let restore_err = h
        .engine
        .commit_plan(":1.40", &r_plan_bad.plan_id, &r_plan_bad.plan_sha256)
        .await
        .unwrap_err();
    assert!(matches!(
        restore_err,
        AkrepError::ApplyFailed(_) | AkrepError::VerifyFailed(_)
    ));
    assert!(
        !target.exists(),
        "corrupted quarantine package must never materialize at target"
    );

    // 2. Restore intact .akrep-qpkg -> succeeds with verified SHA-256 and restored mode 0640
    fs::write(&qpkg_path, &orig_qpkg).unwrap();
    fs::set_permissions(&qpkg_path, fs::Permissions::from_mode(0o600)).unwrap();
    let r_plan_ok = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::RestoreQuarantine {
                quarantine_id: qid.clone(),
            },
        )
        .await
        .unwrap();
    let r_rec = h
        .engine
        .commit_plan(":1.40", &r_plan_ok.plan_id, &r_plan_ok.plan_sha256)
        .await
        .unwrap();
    assert_eq!(r_rec.state, TransactionState::Committed);
    assert_eq!(fs::read(&target).unwrap(), payload);
    assert_eq!(sha256_hex(&fs::read(&target).unwrap()), expected_sha);
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o640
    );
}

#[tokio::test]
async fn thr_09_symlink_target_rejected_by_quarantine() {
    let h = setup_sprint34_harness();
    let real_secret = h.work_dir.join("real_target.txt");
    fs::write(&real_secret, b"DO_NOT_TOUCH").unwrap();

    // 1. Leaf symlink
    let leaf_link = h.work_dir.join("evil_leaf_symlink");
    symlink(&real_secret, &leaf_link).unwrap();
    let err_leaf = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: leaf_link.to_string_lossy().to_string(),
                threat_name: "Test.Symlink".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err_leaf, AkrepError::ForbiddenOperation(_)));
    assert!(real_secret.is_file());

    // 2. Symlink parent directory traversal
    let real_sub = h.work_dir.join("real_sub");
    fs::create_dir_all(&real_sub).unwrap();
    let inside_file = real_sub.join("inside.bin");
    fs::write(&inside_file, b"INSIDE").unwrap();
    let sym_dir = h.work_dir.join("sym_dir");
    symlink(&real_sub, &sym_dir).unwrap();

    let err_parent = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: sym_dir.join("inside.bin").to_string_lossy().to_string(),
                threat_name: "Test.DirSymlink".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err_parent, AkrepError::ForbiddenOperation(_)));
    assert!(inside_file.is_file());
}

#[tokio::test]
async fn thr_10_quarantine_dir_permissions_0700() {
    let tmp = TempDir::new().unwrap();
    let state_dir = tmp.path().join("akrep_state");
    let vault = state_dir.join("quarantine");
    fs::create_dir_all(&vault).unwrap();
    fs::set_permissions(&vault, fs::Permissions::from_mode(0o755)).unwrap();

    let adapter = QuarantineAdapter::new(state_dir).unwrap();
    let mode = fs::metadata(adapter.vault_dir())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o700,
        "quarantine directory must be locked down to 0700"
    );
}

#[tokio::test]
async fn thr_11_tcp_clamd_endpoint_forbidden() {
    let h = setup_sprint34_harness();
    let sample = h.work_dir.join("sample.txt");
    fs::write(&sample, b"hello").unwrap();

    for forbidden_ep in [
        "tcp://127.0.0.1:3310",
        "127.0.0.1:3310",
        "localhost:3310",
        ":3310",
        "192.168.1.50:3310",
        "127.0.0.1",
        "[::1]:3310",
        "http://127.0.0.1:3310/scan",
        "relative/clamd.sock",
        "/run/clamd/../clamd.sock",
    ] {
        let direct_err = validate_clamd_endpoint(forbidden_ep).unwrap_err();
        assert!(
            matches!(direct_err, AkrepError::ForbiddenOperation(_)),
            "endpoint '{}' must be ForbiddenOperation",
            forbidden_ep
        );

        let cfg_res = ClamdUnixSocketScanner::with_config(
            &[forbidden_ep.to_string()],
            PathBuf::from("/usr/bin/clamscan"),
            PathBuf::from("/var/lib/clamav"),
        );
        assert!(matches!(cfg_res, Err(AkrepError::ForbiddenOperation(_))));

        let req = ScanRequest {
            target_path: sample.to_string_lossy().to_string(),
            recursive: false,
            socket_override: Some(forbidden_ep.to_string()),
            synchronous: true,
        };
        let dbus_err = h
            .engine
            .start_scan(":1.40", &serde_json::to_string(&req).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(dbus_err, AkrepError::ForbiddenOperation(_)));
    }
}

#[tokio::test]
async fn thr_12_false_positive_restore_requires_polkit_approval() {
    let h = setup_sprint34_harness();
    let fp_file = h.work_dir.join("legit_script.py");
    fs::write(&fp_file, b"print('safe')\n").unwrap();

    let q_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: fp_file.to_string_lossy().to_string(),
                threat_name: "Heuristics.FalsePositive".to_string(),
            },
        )
        .await
        .unwrap();
    let q_rec = h
        .engine
        .commit_plan(":1.40", &q_plan.plan_id, &q_plan.plan_sha256)
        .await
        .unwrap();
    let qid = q_rec
        .target_state
        .get("quarantine_id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    // Unprivileged caller without POLKIT_QUARANTINE_MANAGE is rejected
    h.auth
        .allow_actions_for_bus(":1.77", 1001, [POLKIT_SELINUX_DIAGNOSE]);
    let denied = h
        .engine
        .create_plan(
            ":1.77",
            ActionRequest::RestoreQuarantine {
                quarantine_id: qid.clone(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(denied, AkrepError::PolkitDenied(_)));
    assert!(!fp_file.exists());

    // Granting POLKIT_QUARANTINE_MANAGE permits restore
    h.auth
        .allow_actions_for_bus(":1.77", 1001, [POLKIT_QUARANTINE_MANAGE]);
    let r_plan = h
        .engine
        .create_plan(
            ":1.77",
            ActionRequest::RestoreQuarantine { quarantine_id: qid },
        )
        .await
        .unwrap();
    let r_rec = h
        .engine
        .commit_plan(":1.77", &r_plan.plan_id, &r_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(r_rec.state, TransactionState::Committed);
    assert!(fp_file.is_file());
}

// ============================================================================
// Declarative Security Profiles Acceptance Tests (PRF-01 .. PRF-10)
// ============================================================================

#[tokio::test]
async fn prf_01_balanced_profile_plan() {
    let h = setup_sprint34_harness();

    let catalog: ProfileCatalogResponse =
        serde_json::from_str(&h.engine.list_profiles(":1.40").await.unwrap()).unwrap();
    assert_eq!(catalog.profiles.len(), 4);
    assert!(catalog.profiles.iter().any(|p| p.id == "balanced"));

    let plan_json = h.engine.preview_profile(":1.40", "balanced").await.unwrap();
    let plan: ActionPlan = serde_json::from_str(&plan_json).unwrap();
    assert_eq!(plan.polkit_action, POLKIT_PROFILE_APPLY);
    assert!(!plan.pre_fingerprints.is_empty());

    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);
    assert_eq!(
        rec.target_state
            .get("effective_zone")
            .and_then(|v| v.as_str()),
        Some("FedoraWorkstation")
    );
}

#[tokio::test]
async fn prf_02_developer_profile_enables_cgroup_bool_keeps_selinux() {
    let h = setup_sprint34_harness();
    assert!(
        !h.selinux_ops
            .read_boolean("container_manage_cgroup")
            .unwrap()
            .unwrap()
            .current
    );

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "developer".to_string(),
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let cg = h
        .selinux_ops
        .read_boolean("container_manage_cgroup")
        .unwrap()
        .unwrap();
    assert!(cg.current);
    assert_eq!(cg.persistent, Some(true));
    assert_eq!(
        h.selinux_ops.read_runtime_mode().unwrap(),
        blaze_akrep_common::SelinuxRuntimeMode::Enforcing
    );
    assert!(h.firewall_backend.is_running().await.unwrap());
}

#[tokio::test]
async fn prf_03_gaming_profile_rfc1918_lan_rules_only() {
    let h = setup_sprint34_harness();

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "gaming".to_string(),
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let fw = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    assert_eq!(fw.runtime_rich_rules.len(), 6);
    assert!(fw.permanent_rich_rules.is_empty());

    for cidr in ["192.168.0.0/16", "10.0.0.0/8", "172.16.0.0/12"] {
        let tcp_rule = build_cidr_port_rich_rule(cidr, "27040", FirewallProto::Tcp).unwrap();
        let udp_rule = build_cidr_port_rich_rule(cidr, "27031-27036", FirewallProto::Udp).unwrap();
        assert!(fw.runtime_rich_rules.contains(&tcp_rule));
        assert!(fw.runtime_rich_rules.contains(&udp_rule));
    }

    // Forbidden W^X memory booleans remain strictly off
    for forbidden in ["selinuxuser_execheap", "selinuxuser_execstack"] {
        assert!(
            !h.selinux_ops
                .read_boolean(forbidden)
                .unwrap()
                .unwrap()
                .current
        );
    }
}

#[tokio::test]
async fn prf_04_public_network_profile_locks_zone_and_high_ports() {
    let h = setup_sprint34_harness();
    // Simulate active ssh runtime service in public zone before switching to public-network
    h.firewall_backend
        .add_runtime_service("public", "ssh", 0)
        .await
        .unwrap();

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "public-network".to_string(),
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let fw_ws = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    let pub_z = h.firewall_backend.get_zone_state("public").await.unwrap();

    // wlan0 moved out of FedoraWorkstation (which had 1025-65535/tcp,udp, mdns, samba-client) into public
    assert!(fw_ws.runtime_interfaces.is_empty());
    assert!(pub_z.runtime_interfaces.contains(&"wlan0".to_string()));
    assert!(pub_z.runtime_interfaces.contains(&"eth0".to_string()));
    assert!(!pub_z
        .runtime_ports
        .contains(&("1025-65535".to_string(), "tcp".to_string())));
    assert!(!pub_z.runtime_services.contains(&"ssh".to_string()));
    assert!(!pub_z.runtime_services.contains(&"mdns".to_string()));
    assert!(!pub_z.runtime_services.contains(&"samba-client".to_string()));

    // Switching back to balanced restores wlan0 to FedoraWorkstation and restores ssh & mdns on public
    let bal_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "balanced".to_string(),
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.40", &bal_plan.plan_id, &bal_plan.plan_sha256)
        .await
        .unwrap();

    let fw_ws_after = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    let pub_after = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(fw_ws_after
        .runtime_interfaces
        .contains(&"wlan0".to_string()));
    assert!(pub_after.runtime_services.contains(&"ssh".to_string()));
    assert!(pub_after.runtime_services.contains(&"mdns".to_string()));
}

#[tokio::test]
async fn prf_05_profile_switch_creates_transaction() {
    let h = setup_sprint34_harness();

    for profile_id in ["developer", "gaming", "public-network", "balanced"] {
        let plan = h
            .engine
            .create_plan(
                ":1.40",
                ActionRequest::ApplyProfile {
                    profile_id: profile_id.to_string(),
                },
            )
            .await
            .unwrap();
        let rec = h
            .engine
            .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap();
        assert_eq!(rec.state, TransactionState::Committed);
    }

    let history = h.engine.get_history(":1.40", 20).await.unwrap();
    assert_eq!(history.len(), 4);
    assert!(history
        .iter()
        .all(|r| r.state == TransactionState::Committed));
}

#[tokio::test]
async fn prf_06_profile_rollback_restores_managed_state_preserving_user_rules() {
    let h = setup_sprint34_harness();

    // User creates pre-existing custom port & rich rule before profile activation
    let user_rich =
        build_cidr_port_rich_rule("192.168.88.0/24", "9090", FirewallProto::Tcp).unwrap();
    h.firewall_backend
        .add_runtime_port("FedoraWorkstation", "8443", FirewallProto::Tcp, 0)
        .await
        .unwrap();
    h.firewall_backend
        .add_runtime_rich_rule("FedoraWorkstation", &user_rich, 0)
        .await
        .unwrap();

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "gaming".to_string(),
            },
        )
        .await
        .unwrap();
    let committed = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();

    let during = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    assert_eq!(during.runtime_rich_rules.len(), 7); // 1 user + 6 Akrep gaming rules

    // Rollback gaming transaction -> removes ONLY Akrep's 6 rules, preserving user's 8443/tcp & 9090/tcp rich rule
    let rb = h
        .engine
        .rollback_transaction(":1.40", &committed.tx_id)
        .await
        .unwrap();
    assert_eq!(rb.state, TransactionState::RolledBack);

    let after = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    assert!(after
        .runtime_ports
        .contains(&("8443".to_string(), "tcp".to_string())));
    assert_eq!(after.runtime_rich_rules, vec![user_rich]);
}

#[tokio::test]
async fn prf_07_unknown_profile_rejected() {
    let h = setup_sprint34_harness();

    for bad_id in [
        "nonexistent",
        "custom_turbo",
        "disabled",
        "selinux_disabled",
    ] {
        let err = h
            .engine
            .create_plan(
                ":1.40",
                ActionRequest::ApplyProfile {
                    profile_id: bad_id.to_string(),
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            AkrepError::InvalidInput(_) | AkrepError::ForbiddenOperation(_)
        ));
    }
}

#[tokio::test]
async fn prf_08_malformed_toml_fails_safely() {
    // 1. Syntax error TOML
    assert!(SecurityProfileSpec::parse_and_validate_toml("id = [broken", None).is_err());

    // 2. Unknown field rejected via deny_unknown_fields
    let unknown_field_toml = r#"
id = "balanced"
version = 1
name_tr = "A"
name_en = "B"
description_tr = "C"
description_en = "D"
selinux_required = true
firewalld_required = true
target_zone = "FedoraWorkstation"
unexpected_rootkit_hook = "sh -c id"
"#;
    assert!(SecurityProfileSpec::parse_and_validate_toml(unknown_field_toml, None).is_err());

    // 3. selinux_required = false rejected
    let bad_selinux_toml = r#"
id = "balanced"
version = 1
name_tr = "A"
name_en = "B"
description_tr = "C"
description_en = "D"
selinux_required = false
firewalld_required = true
target_zone = "FedoraWorkstation"
"#;
    assert!(matches!(
        SecurityProfileSpec::parse_and_validate_toml(bad_selinux_toml, None),
        Err(AkrepError::ForbiddenOperation(_))
    ));

    // 4. Malformed file on disk fails safely without modifying system state
    let bad_dir = TempDir::new().unwrap();
    fs::write(bad_dir.path().join("balanced.toml"), "not valid = = toml").unwrap();
    let h = setup_harness_with_profiles_dir(bad_dir.path().to_path_buf());
    let err = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "balanced".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::InvalidInput(_)));
}

#[tokio::test]
async fn prf_09_external_drift_detected_on_profile_rollback() {
    let h = setup_sprint34_harness();

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "developer".to_string(),
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();

    // Simulate external administrator modifying FedoraWorkstation zone out-of-band after commit
    h.firewall_backend.inject_external_runtime_port(
        "FedoraWorkstation",
        "51820",
        FirewallProto::Udp,
    );

    let drift_err = h
        .engine
        .rollback_transaction(":1.40", &rec.tx_id)
        .await
        .unwrap_err();
    assert!(matches!(drift_err, AkrepError::ExternalDriftDetected(_)));

    // External admin port must NOT be wiped by blind rollback
    let fw = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    assert!(fw
        .runtime_ports
        .contains(&("51820".to_string(), "udp".to_string())));
}

#[tokio::test]
async fn prf_10_profile_persists_across_engine_restart() {
    let tmp = TempDir::new().unwrap();
    let state_dir = tmp.path().join("var_lib_akrep");
    let sys_fp = tmp.path().join("var_lib_flatpak");
    let sys_apps = tmp.path().join("usr_share_apps");
    fs::create_dir_all(&sys_fp).unwrap();
    fs::create_dir_all(&sys_apps).unwrap();

    let auth = Arc::new(MockAuthorizer::new());
    auth.allow_all_for_bus(":1.40", 1000);
    let selinux_ops = Arc::new(MockSelinuxOps::new_fedora_enforcing());
    let fw_backend = Arc::new(MockFirewallBackend::new_fedora_workstation());

    let build_engine = || {
        let selinux = Arc::new(SelinuxAdapter::new(selinux_ops.clone()));
        let firewall = Arc::new(FirewallAdapter::new(fw_backend.clone()));
        let flatpak = Arc::new(SystemFlatpakAdapter::with_roots(
            sys_fp.clone(),
            sys_apps.clone(),
            Arc::new(MockFlatpakRuntimeProbe::new()),
        ));
        let dev_sec = Arc::new(DeviceSecurityAdapter::new(Arc::new(
            MockDeviceSecurityOps::new_healthy_uefi(),
        )));
        let scanner = Arc::new(ScannerAdapter::new(
            state_dir.clone(),
            Arc::new(MockClamScanner::new_healthy()),
            Arc::new(MockScannerInstallerOps::new(false)),
        ));
        let quarantine = Arc::new(QuarantineAdapter::new_lazy(state_dir.clone()));
        let profiles = Arc::new(ProfileAdapter::with_paths(
            official_profiles_dir(),
            state_dir.clone(),
            selinux.clone(),
            firewall.clone(),
        ));
        let composite = Arc::new(CompositeMutationBackend::new_full(
            selinux, firewall, flatpak, dev_sec, scanner, quarantine, profiles,
        ));
        TransactionEngine::new(state_dir.clone(), auth.clone(), composite).unwrap()
    };

    {
        let engine1 = build_engine();
        let p = engine1
            .create_plan(
                ":1.40",
                ActionRequest::ApplyProfile {
                    profile_id: "developer".to_string(),
                },
            )
            .await
            .unwrap();
        engine1
            .commit_plan(":1.40", &p.plan_id, &p.plan_sha256)
            .await
            .unwrap();
    }

    // Restart with fresh engine instance over the same state_dir
    let engine2 = build_engine();
    let cat: ProfileCatalogResponse =
        serde_json::from_str(&engine2.list_profiles(":1.40").await.unwrap()).unwrap();
    assert_eq!(cat.active_profile_id.as_deref(), Some("developer"));
    assert_eq!(cat.managed_state.managed_booleans.len(), 1);

    // Switching back to balanced on Engine 2 reverts container_manage_cgroup using persisted managed state
    let p_bal = engine2
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "balanced".to_string(),
            },
        )
        .await
        .unwrap();
    engine2
        .commit_plan(":1.40", &p_bal.plan_id, &p_bal.plan_sha256)
        .await
        .unwrap();
    assert!(
        !selinux_ops
            .read_boolean("container_manage_cgroup")
            .unwrap()
            .unwrap()
            .current
    );
}

// ============================================================================
// Security Regression Suite (SEC-01 .. SEC-11)
// ============================================================================

#[tokio::test]
async fn sec_01_polkit_system_bus_name_enforced_across_all_privileged_endpoints() {
    let subject = build_polkit_subject(":1.88");
    assert_eq!(subject.0, "system-bus-name");
    assert!(!subject.1.contains_key("pid"));

    let h = setup_sprint34_harness();
    h.auth.deny_all_for_bus(":1.99", 1009);

    assert!(matches!(
        h.engine
            .create_plan(
                ":1.99",
                ActionRequest::ApplyProfile {
                    profile_id: "balanced".to_string()
                }
            )
            .await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.diagnose_selinux(":1.99", "{}").await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.inspect_firewall(":1.99", "").await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.inspect_system_flatpaks(":1.99").await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.inspect_device_security(":1.99").await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.inspect_scanner(":1.99").await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine
            .start_scan(":1.99", r#"{"target_path":"/tmp"}"#)
            .await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.list_quarantine(":1.99").await,
        Err(AkrepError::PolkitDenied(_))
    ));
    assert!(matches!(
        h.engine.list_profiles(":1.99").await,
        Err(AkrepError::PolkitDenied(_))
    ));
}

#[tokio::test]
async fn sec_02_zero_shell_execution_and_metacharacter_rejection() {
    let h = setup_sprint34_harness();
    let payloads = [
        "/tmp/file;id",
        "/tmp/file$(whoami)",
        "/tmp/file`id`",
        "/tmp/file|cat",
        "/tmp/file&",
    ];
    for bad in payloads {
        let q_req = ActionRequest::QuarantineFile {
            path: bad.to_string(),
            threat_name: "Test".to_string(),
        };
        assert!(q_req.validate_red_lines().is_err());
        assert!(h.engine.create_plan(":1.40", q_req).await.is_err());

        let s_req = ScanRequest {
            target_path: bad.to_string(),
            recursive: false,
            socket_override: None,
            synchronous: true,
        };
        assert!(s_req.validate_red_lines().is_err());
    }
}

#[test]
fn sec_03_selinux_never_disabled_or_audit2allow_invoked() {
    for directive in ["SELINUX=disabled", "audit2allow", "semodule", "setenforce"] {
        let req = ActionRequest::SelinuxSetBoolean {
            name: directive.to_string(),
            value: true,
            persistent: true,
        };
        assert!(matches!(
            req.validate_red_lines(),
            Err(AkrepError::ForbiddenOperation(_))
        ));
    }

    let execstack_profile = r#"
id = "gaming"
version = 1
name_tr = "A"
name_en = "B"
description_tr = "C"
description_en = "D"
selinux_required = true
firewalld_required = true
target_zone = "FedoraWorkstation"

[[managed_booleans]]
name = "selinuxuser_execstack"
value = true
persistent = false
"#;
    assert!(matches!(
        SecurityProfileSpec::parse_and_validate_toml(execstack_profile, None),
        Err(AkrepError::ForbiddenOperation(_))
    ));
}

#[test]
fn sec_04_firewalld_never_disabled_and_lan_rules_rfc1918_scoped() {
    assert!(is_rfc1918_ipv4_cidr("192.168.0.0/16").is_ok());
    assert!(is_rfc1918_ipv4_cidr("10.0.0.0/8").is_ok());
    assert!(is_rfc1918_ipv4_cidr("172.16.0.0/12").is_ok());
    assert!(is_rfc1918_ipv4_cidr("0.0.0.0/0").is_err());
    assert!(is_rfc1918_ipv4_cidr("8.8.8.0/24").is_err());
    assert!(is_rfc1918_ipv4_cidr("172.32.0.0/16").is_err());

    let fw_disabled_toml = r#"
id = "gaming"
version = 1
name_tr = "A"
name_en = "B"
description_tr = "C"
description_en = "D"
selinux_required = true
firewalld_required = false
target_zone = "FedoraWorkstation"
"#;
    assert!(matches!(
        SecurityProfileSpec::parse_and_validate_toml(fw_disabled_toml, None),
        Err(AkrepError::ForbiddenOperation(_))
    ));

    let wan_rule_toml = r#"
id = "gaming"
version = 1
name_tr = "A"
name_en = "B"
description_tr = "C"
description_en = "D"
selinux_required = true
firewalld_required = true
target_zone = "FedoraWorkstation"

[[lan_rich_rules]]
cidr = "8.8.8.0/24"
port = "27040"
proto = "tcp"
"#;
    assert!(matches!(
        SecurityProfileSpec::parse_and_validate_toml(wan_rule_toml, None),
        Err(AkrepError::ForbiddenOperation(_))
    ));
}

#[tokio::test]
async fn sec_05_quarantine_symlink_and_system_root_escape_blocked() {
    let h = setup_sprint34_harness();

    for sys_path in [
        "/etc/passwd",
        "/usr/bin/sh",
        "/bin/ls",
        "/boot/vmlinuz",
        "/proc/1/cmdline",
        "/sys/kernel/security/lockdown",
        "/dev/null",
        "/run/dbus/system_bus_socket",
        "/var/lib/akrep/audit.log",
    ] {
        let req = ActionRequest::QuarantineFile {
            path: sys_path.to_string(),
            threat_name: "Fake".to_string(),
        };
        assert!(matches!(
            req.validate_red_lines(),
            Err(AkrepError::ForbiddenOperation(_))
        ));
        assert!(matches!(
            h.engine.create_plan(":1.40", req).await,
            Err(AkrepError::ForbiddenOperation(_))
        ));
    }

    // Symlink pointing to /etc/passwd inside work_dir
    let passwd_link = h.work_dir.join("passwd_link");
    symlink("/etc/passwd", &passwd_link).unwrap();
    let err = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: passwd_link.to_string_lossy().to_string(),
                threat_name: "Escape".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::ForbiddenOperation(_)));
}

#[tokio::test]
async fn sec_06_quarantine_toctou_inode_swap_blocked_before_unlink() {
    let h = setup_sprint34_harness();
    let victim = h.work_dir.join("swap_target.bin");
    fs::write(&victim, b"ORIGINAL_BYTES").unwrap();

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: victim.to_string_lossy().to_string(),
                threat_name: "SwapTest".to_string(),
            },
        )
        .await
        .unwrap();

    // Swap victim with a different inode between CreatePlan and CommitPlan
    let replacement = h.work_dir.join("replacement.bin");
    fs::write(&replacement, b"INNOCENT_REPLACEMENT_FILE").unwrap();
    fs::rename(&replacement, &victim).unwrap();

    let err = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::ExternalDriftDetected(_)));
    assert_eq!(fs::read(&victim).unwrap(), b"INNOCENT_REPLACEMENT_FILE");
}

#[tokio::test]
async fn sec_07_clamav_tcp_endpoints_and_clamonacc_strictly_forbidden() {
    let h = setup_sprint34_harness();
    let rep: ScannerStatusReport =
        serde_json::from_str(&h.engine.inspect_scanner(":1.40").await.unwrap()).unwrap();
    assert!(!rep.realtime_protection_active);
    assert!(matches!(
        validate_clamd_endpoint("tcp://10.0.0.2:3310"),
        Err(AkrepError::ForbiddenOperation(_))
    ));
}

#[tokio::test]
async fn sec_08_flatpak_sandbox_escape_and_user_scope_privd_isolation() {
    let h = setup_sprint34_harness();

    let user_scope = ActionRequest::FlatpakSetOverride {
        app_id: "org.example.App".to_string(),
        scope: FlatpakScope::User,
        additions: vec![("Context.filesystems".to_string(), "~/Music:ro".to_string())],
        removals: vec![],
    };
    assert!(matches!(
        h.engine.create_plan(":1.40", user_scope).await,
        Err(AkrepError::ForbiddenOperation(_))
    ));

    for blanket in ["host", "home", "/", "xdg-data/flatpak/overrides:rw"] {
        let sys_escape = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.App".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("Context.filesystems".to_string(), blanket.to_string())],
            removals: vec![],
        };
        assert!(matches!(
            sys_escape.validate_red_lines(),
            Err(AkrepError::ForbiddenOperation(_))
        ));
    }
}

#[tokio::test]
async fn sec_09_plan_hash_tamper_replay_cross_uid_and_expiry_blocked() {
    let h = setup_sprint34_harness();
    h.auth.allow_all_for_bus(":1.41", 1001);

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "developer".to_string(),
            },
        )
        .await
        .unwrap();

    // 1. Tampered SHA-256 rejected
    assert!(matches!(
        h.engine
            .commit_plan(":1.40", &plan.plan_id, &"f".repeat(64))
            .await,
        Err(AkrepError::PlanMismatch(_))
    ));

    // 2. Cross-UID commit rejected
    assert!(matches!(
        h.engine
            .commit_plan(":1.41", &plan.plan_id, &plan.plan_sha256)
            .await,
        Err(AkrepError::Unauthorized(_))
    ));

    // 3. Valid commit succeeds once, replay rejected
    let ok = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(ok.state, TransactionState::Committed);
    assert!(matches!(
        h.engine
            .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
            .await,
        Err(AkrepError::PlanMismatch(_))
    ));

    // 4. Expired plan rejected
    let mut exp_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "balanced".to_string(),
            },
        )
        .await
        .unwrap();
    exp_plan.expires_at = (Utc::now() - ChronoDuration::seconds(30)).to_rfc3339();
    exp_plan.plan_sha256 = exp_plan.compute_canonical_sha256().unwrap();
    blaze_akrep_privd::write_json_atomic_0600(
        &h.engine
            .plans_dir()
            .join(format!("{}.json", exp_plan.plan_id)),
        &exp_plan,
    )
    .unwrap();
    assert!(matches!(
        h.engine
            .commit_plan(":1.40", &exp_plan.plan_id, &exp_plan.plan_sha256)
            .await,
        Err(AkrepError::PlanExpired(_))
    ));
}

#[tokio::test]
async fn sec_10_state_vault_and_audit_log_permissions_0700_0600_and_secret_redaction() {
    let h = setup_sprint34_harness();

    let p = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "developer".to_string(),
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.40", &p.plan_id, &p.plan_sha256)
        .await
        .unwrap();

    for dir_name in ["transactions", "snapshots", "quarantine", "plans"] {
        let m = fs::metadata(h.state_dir.join(dir_name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m, 0o700, "{} must be 0700", dir_name);
    }

    for file_name in ["audit.log", "active_profile.json"] {
        let m = fs::metadata(h.state_dir.join(file_name))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(m, 0o600, "{} must be 0600", file_name);
    }

    let err = AkrepError::ApplyFailed(
        "failed with password=TopSecret123 token=tok_live_999 Bearer jwt_abc".to_string(),
    );
    let redacted = err.to_redacted_json();
    assert!(!redacted.contains("TopSecret123"));
    assert!(!redacted.contains("tok_live_999"));
    assert!(!redacted.contains("jwt_abc"));
    assert!(redacted.contains("[REDACTED]"));

    // Confirm DeleteQuarantine requires confirm_permanent_delete = true
    let bad_del = ActionRequest::DeleteQuarantine {
        quarantine_id: "q_1234".to_string(),
        confirm_permanent_delete: false,
    };
    assert!(matches!(
        bad_del.validate_red_lines(),
        Err(AkrepError::ForbiddenOperation(_))
    ));
    let port_check = ActionRequest::FirewallAddPort {
        zone: "public".to_string(),
        port: "443".to_string(),
        proto: FirewallProto::Tcp,
        mode: RulePersistence::Runtime,
        source_cidr: None,
    };
    assert!(port_check.validate_red_lines().is_ok());
}

#[tokio::test]
async fn sec_11_honest_unavailable_posture_never_falsely_protected() {
    let tmp = TempDir::new().unwrap();
    let sb = inspect_secure_boot_at(tmp.path());
    assert_eq!(sb.severity, AkrepSeverity::Unavailable);
    assert!(!sb.severity.is_protected());

    let tpm = inspect_tpm_at(tmp.path(), tmp.path());
    assert_eq!(tpm.severity, AkrepSeverity::Unavailable);
    assert!(!tpm.severity.is_protected());

    let sig = inspect_signature_db_at(&tmp.path().join("missing_clamav"), SystemTime::now());
    assert_eq!(sig.severity, AkrepSeverity::Unavailable);
    assert!(!sig.severity.is_protected());

    let h = setup_sprint34_harness();
    h.firewall_backend.set_running(false);
    let prof_err = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "balanced".to_string(),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(prof_err, AkrepError::BackendUnavailable(_)));
}

// ============================================================================
// Fix Round 1 Security & Reliability Regression Tests
// ============================================================================

#[test]
fn reg_c1_privd_unit_allows_home_and_tmp_while_akrepd_stays_isolated() {
    let systemd_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../blazeos_custom_apps/usr/lib/systemd/system")
        .canonicalize()
        .expect("systemd unit dir");
    let privd_unit = fs::read_to_string(systemd_dir.join("akrep-privd.service")).unwrap();
    assert!(privd_unit.contains("ProtectHome=no"));
    assert!(privd_unit.contains("PrivateTmp=no"));
    assert!(!privd_unit.contains("ProtectHome=yes"));
    assert!(!privd_unit.contains("PrivateTmp=yes"));

    let akrepd_unit = fs::read_to_string(systemd_dir.join("akrepd.service")).unwrap();
    assert!(akrepd_unit.contains("ProtectHome=yes"));
    assert!(akrepd_unit.contains("PrivateTmp=yes"));
}

#[tokio::test]
async fn reg_c2_prod_engine_blocks_socket_override_sensitive_paths_and_untrusted_clamd_sockets() {
    let tmp = TempDir::new().unwrap();
    let state_dir = tmp.path().join("var_lib_akrep");
    let sample = tmp.path().join("clean.txt");
    fs::write(&sample, b"hello").unwrap();

    let auth = Arc::new(MockAuthorizer::new());
    auth.allow_all_for_bus(":1.40", 1000);

    let selinux = Arc::new(SelinuxAdapter::new(Arc::new(
        MockSelinuxOps::new_fedora_enforcing(),
    )));
    let firewall = Arc::new(FirewallAdapter::new(Arc::new(
        MockFirewallBackend::new_fedora_workstation(),
    )));
    let flatpak = Arc::new(SystemFlatpakAdapter::with_roots(
        tmp.path().join("fp"),
        tmp.path().join("apps"),
        Arc::new(MockFlatpakRuntimeProbe::new()),
    ));
    let dev_sec = Arc::new(DeviceSecurityAdapter::new(Arc::new(
        MockDeviceSecurityOps::new_healthy_uefi(),
    )));
    let prod_scanner = Arc::new(ScannerAdapter::new(
        state_dir.clone(),
        Arc::new(MockClamScanner::new_healthy()),
        Arc::new(MockScannerInstallerOps::new(false)),
    ));
    let quarantine = Arc::new(QuarantineAdapter::new_lazy(state_dir.clone()));
    let profiles = Arc::new(ProfileAdapter::with_paths(
        official_profiles_dir(),
        state_dir.clone(),
        selinux.clone(),
        firewall.clone(),
    ));
    let composite = Arc::new(CompositeMutationBackend::new_full(
        selinux,
        firewall,
        flatpak,
        dev_sec,
        prod_scanner.clone(),
        quarantine,
        profiles,
    ));

    // Production TransactionEngine::new() has allow_test_actions = false
    let prod_engine = TransactionEngine::new(state_dir, auth, composite).unwrap();

    let evil_req = ScanRequest {
        target_path: sample.to_string_lossy().to_string(),
        recursive: false,
        socket_override: Some("/tmp/evil.sock".to_string()),
        synchronous: true,
    };
    let err = prod_engine
        .start_scan(":1.40", &serde_json::to_string(&evil_req).unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::ForbiddenOperation(_)));

    // Direct production ScannerAdapter also rejects socket_override
    let direct_err = prod_scanner.start_scan(evil_req, 1000).await.unwrap_err();
    assert!(matches!(direct_err, AkrepError::ForbiddenOperation(_)));

    // Sensitive credential & Akrep state paths blocked in validate_scan_target_path
    for sensitive in [
        "/etc/shadow",
        "/etc/gshadow",
        "/etc/sudoers",
        "/etc/sudoers.d/wheel",
        "/var/lib/akrep/audit.log",
    ] {
        assert!(matches!(
            validate_scan_target_path(sensitive),
            Err(AkrepError::ForbiddenOperation(_))
        ));
    }

    // Strict production clamd socket metadata checks: world-writable (0666) or non-system uid >= 1000 rejected
    let sock_path = tmp.path().join("untrusted.sock");
    let _listener = UnixListener::bind(&sock_path).unwrap();
    fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o666)).unwrap();
    let strict_scanner = ClamdUnixSocketScanner::with_config_strict(
        &[sock_path.to_string_lossy().to_string()],
        PathBuf::from("/nonexistent/clamscan"),
        tmp.path().join("db"),
        true,
    )
    .unwrap();
    assert!(matches!(
        strict_scanner.inspect_engine(),
        Err(AkrepError::ForbiddenOperation(_))
    ));

    fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o660)).unwrap();
    // SAFETY: `geteuid` is thread-safe and infallible.
    if unsafe { libc::geteuid() } >= 1000 {
        assert!(matches!(
            strict_scanner.inspect_engine(),
            Err(AkrepError::ForbiddenOperation(_))
        ));
    }
}

#[tokio::test]
async fn reg_i1_scanner_rejects_intermediate_directory_symlinks() {
    let h = setup_sprint34_harness();

    let sym_proc = h.work_dir.join("sym_proc");
    symlink("/proc", &sym_proc).unwrap();
    let proc_req = ScanRequest {
        target_path: sym_proc.join("1/cmdline").to_string_lossy().to_string(),
        recursive: false,
        socket_override: None,
        synchronous: true,
    };
    let err_proc = h
        .engine
        .start_scan(":1.40", &serde_json::to_string(&proc_req).unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err_proc, AkrepError::ForbiddenOperation(_)));

    let q_dir = h.state_dir.join("quarantine");
    let sym_q = h.work_dir.join("sym_q");
    symlink(&q_dir, &sym_q).unwrap();
    let q_req = ScanRequest {
        target_path: sym_q.to_string_lossy().to_string(),
        recursive: true,
        socket_override: None,
        synchronous: true,
    };
    let err_q = h
        .engine
        .start_scan(":1.40", &serde_json::to_string(&q_req).unwrap())
        .await
        .unwrap_err();
    assert!(matches!(err_q, AkrepError::ForbiddenOperation(_)));
}

#[tokio::test]
async fn reg_i2_active_background_scan_blocks_idle_expiry() {
    let h = setup_sprint34_harness();
    h.scanner_backend
        .set_per_file_delay(Duration::from_millis(80));
    for i in 0..10 {
        fs::write(h.work_dir.join(format!("bg_{}.bin", i)), b"payload").unwrap();
    }
    assert!(!h.engine.has_active_background_jobs());

    let req = ScanRequest {
        target_path: h.work_dir.to_string_lossy().to_string(),
        recursive: true,
        socket_override: None,
        synchronous: false,
    };
    let started: ScanProgress = serde_json::from_str(
        &h.engine
            .start_scan(":1.40", &serde_json::to_string(&req).unwrap())
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(h.engine.has_active_background_jobs());

    let _ = h
        .engine
        .cancel_scan(":1.40", &started.job_id)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(120)).await;
    assert!(!h.engine.has_active_background_jobs());
}

#[tokio::test]
async fn reg_i3_mid_step2_profile_apply_failure_rolls_back_without_double_revert_or_leaked_rules() {
    let h = setup_sprint34_harness();

    // User adds custom port 8443/tcp, then applies `developer` profile
    h.firewall_backend
        .add_runtime_port("FedoraWorkstation", "8443", FirewallProto::Tcp, 0)
        .await
        .unwrap();

    let dev_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "developer".to_string(),
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.40", &dev_plan.plan_id, &dev_plan.plan_sha256)
        .await
        .unwrap();
    assert!(
        h.selinux_ops
            .read_boolean("container_manage_cgroup")
            .unwrap()
            .unwrap()
            .current
    );

    // Prepare transition `developer -> gaming`, then inject failure after 2 of the 6 rich rules succeed
    let game_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::ApplyProfile {
                profile_id: "gaming".to_string(),
            },
        )
        .await
        .unwrap();
    h.firewall_backend.set_fail_after_rich_rule_adds(Some(2));

    let err = h
        .engine
        .commit_plan(":1.40", &game_plan.plan_id, &game_plan.plan_sha256)
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::ApplyFailed(_)));

    // Verify automatic rollback removed the 2 partial gaming rules, restored `developer` boolean, and preserved 8443/tcp!
    let fw = h
        .firewall_backend
        .get_zone_state("FedoraWorkstation")
        .await
        .unwrap();
    assert!(
        fw.runtime_rich_rules.is_empty(),
        "partial Step 2 rich rules must be cleaned up on rollback"
    );
    assert!(fw
        .runtime_ports
        .contains(&("8443".to_string(), "tcp".to_string())));
    assert!(
        h.selinux_ops
            .read_boolean("container_manage_cgroup")
            .unwrap()
            .unwrap()
            .current
    );

    let cat: ProfileCatalogResponse =
        serde_json::from_str(&h.engine.list_profiles(":1.40").await.unwrap()).unwrap();
    assert_eq!(cat.active_profile_id.as_deref(), Some("developer"));
}

#[tokio::test]
async fn reg_i4_quarantine_restore_strips_suid_sgid_bits() {
    let h = setup_sprint34_harness();
    let suid_bin = h.work_dir.join("suid_candidate.bin");
    fs::write(&suid_bin, b"BINARY_PAYLOAD").unwrap();
    fs::set_permissions(&suid_bin, fs::Permissions::from_mode(0o4755)).unwrap();

    let q_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: suid_bin.to_string_lossy().to_string(),
                threat_name: "Test.SuidCheck".to_string(),
            },
        )
        .await
        .unwrap();
    let q_rec = h
        .engine
        .commit_plan(":1.40", &q_plan.plan_id, &q_plan.plan_sha256)
        .await
        .unwrap();
    let qid = q_rec
        .target_state
        .get("quarantine_id")
        .and_then(|v| v.as_str())
        .unwrap()
        .to_string();

    let r_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::RestoreQuarantine { quarantine_id: qid },
        )
        .await
        .unwrap();
    let r_rec = h
        .engine
        .commit_plan(":1.40", &r_plan.plan_id, &r_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(r_rec.state, TransactionState::Committed);

    let restored_mode = fs::metadata(&suid_bin).unwrap().permissions().mode() & 0o7777;
    assert_eq!(
        restored_mode, 0o0755,
        "RestoreQuarantine must strip S_ISUID/S_ISGID bits (expected 0755, got {:04o})",
        restored_mode
    );
}

#[tokio::test]
async fn reg_m2_m3_m4_job_cap_quarantine_rollback_conflict_and_audit_actions() {
    let h = setup_sprint34_harness();
    let sample = h.work_dir.join("tiny.txt");
    fs::write(&sample, b"ok").unwrap();

    // M-2: Run 70 synchronous scans and verify old evicted job_ids do not grow unboundedly
    let mut first_job_id = String::new();
    let mut last_job_id = String::new();
    for i in 0..70 {
        let p: ScanProgress = serde_json::from_str(
            &h.engine
                .start_scan(
                    ":1.40",
                    &serde_json::to_string(&ScanRequest {
                        target_path: sample.to_string_lossy().to_string(),
                        recursive: false,
                        socket_override: None,
                        synchronous: true,
                    })
                    .unwrap(),
                )
                .await
                .unwrap(),
        )
        .unwrap();
        if i == 0 {
            first_job_id = p.job_id;
        } else if i == 69 {
            last_job_id = p.job_id;
        }
    }
    assert!(h
        .engine
        .get_scan_status(":1.40", &first_job_id)
        .await
        .is_err());
    assert!(h
        .engine
        .get_scan_status(":1.40", &last_job_id)
        .await
        .is_ok());

    // M-4: Verify get_scan_status / cancel_scan denials and inspect_device_security exact Polkit action audit
    h.auth.deny_all_for_bus(":1.98", 1098);
    let _ = h.engine.get_scan_status(":1.98", &last_job_id).await;
    let _ = h.engine.cancel_scan(":1.98", &last_job_id).await;

    h.auth
        .allow_actions_for_bus(":1.97", 1097, [POLKIT_SELINUX_DIAGNOSE]);
    let _ = h.engine.inspect_device_security(":1.97").await.unwrap();

    let events = h.engine.audit().read_events().unwrap();
    assert!(events.iter().any(|e| e.event == "scan_status_denied"));
    assert!(events.iter().any(|e| e.event == "scan_cancel_denied"));
    assert!(events.iter().any(|e| {
        e.event == "device_security_inspect_completed"
            && e.actor_bus_name.as_deref() == Some(":1.97")
            && e.polkit_action.as_deref() == Some(POLKIT_SELINUX_DIAGNOSE)
    }));

    // M-3: Verify QuarantineAdapter::rollback surfaces conflict error instead of silent Ok if original_path reappeared
    let m3_file = h.work_dir.join("m3_conflict.bin");
    fs::write(&m3_file, b"M3_ORIG").unwrap();
    let q_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::QuarantineFile {
                path: m3_file.to_string_lossy().to_string(),
                threat_name: "Test.M3".to_string(),
            },
        )
        .await
        .unwrap();
    let q_rec = h
        .engine
        .commit_plan(":1.40", &q_plan.plan_id, &q_plan.plan_sha256)
        .await
        .unwrap();

    fs::write(&m3_file, b"BLOCKING_NEW_FILE").unwrap();
    let q_adapter = QuarantineAdapter::new(h.state_dir.clone()).unwrap();
    let snap_dir = h.engine.snapshots_dir().join(&q_rec.tx_id);
    let rb_res = blaze_akrep_privd::MutationBackend::rollback(&q_adapter, &q_rec, &snap_dir).await;
    assert!(matches!(rb_res, Err(AkrepError::RollbackFailed(_))));
    assert_eq!(fs::read(&m3_file).unwrap(), b"BLOCKING_NEW_FILE");
}

#[tokio::test]
async fn prf_11_public_profile_cache_status_is_0644_while_vault_stays_0600() {
    let h = setup_sprint34_harness();
    let pub_path = h
        .root_tmp
        .path()
        .join("var_cache_akrep/active_profile.json");

    let plan_raw = h.engine.preview_profile(":1.40", "gaming").await.unwrap();
    let plan: ActionPlan = serde_json::from_str(&plan_raw).unwrap();
    let rec = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    assert!(pub_path.is_file());
    let pub_mode = fs::metadata(&pub_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(pub_mode, 0o644);
    let priv_mode = fs::metadata(h.state_dir.join("active_profile.json"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(priv_mode, 0o600);

    let pub_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pub_path).unwrap()).unwrap();
    assert_eq!(pub_json["active_profile_id"], "gaming");
    assert!(pub_json.get("managed_booleans").is_none());

    let rb_rec = h
        .engine
        .rollback_transaction(":1.40", &rec.tx_id)
        .await
        .unwrap();
    assert_eq!(rb_rec.state, TransactionState::RolledBack);

    let pub_after_rb: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&pub_path).unwrap()).unwrap();
    assert!(pub_after_rb["active_profile_id"].is_null());
}

#[tokio::test]
async fn prf_12_preexisting_fedora_execstack_boolean_allows_profile_switch_without_mutation() {
    let h = setup_sprint34_harness();
    // Stock Fedora 45 selinux-policy-targeted ships selinuxuser_execstack = on out of the box.
    h.selinux_ops
        .set_boolean("selinuxuser_execstack", true, true)
        .unwrap();

    let dev_plan_raw = h.engine.preview_profile(":1.40", "developer").await.unwrap();
    let dev_plan: ActionPlan = serde_json::from_str(&dev_plan_raw).unwrap();
    let dev_rec = h
        .engine
        .commit_plan(":1.40", &dev_plan.plan_id, &dev_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(dev_rec.state, TransactionState::Committed);
    assert_eq!(
        h.selinux_ops
            .read_boolean("selinuxuser_execstack")
            .unwrap()
            .unwrap()
            .current,
        true
    );

    let pub_plan_raw = h
        .engine
        .preview_profile(":1.40", "public-network")
        .await
        .unwrap();
    let pub_plan: ActionPlan = serde_json::from_str(&pub_plan_raw).unwrap();
    let pub_rec = h
        .engine
        .commit_plan(":1.40", &pub_plan.plan_id, &pub_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(pub_rec.state, TransactionState::Committed);

    let rb_rec = h
        .engine
        .rollback_transaction(":1.40", &pub_rec.tx_id)
        .await
        .unwrap();
    assert_eq!(rb_rec.state, TransactionState::RolledBack);
}
