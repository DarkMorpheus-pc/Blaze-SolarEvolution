use crate::adapters::selinux::command_c_locale;
use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    generate_uuid_v4, redact_secrets, validate_clamd_endpoint, validate_scan_target_path,
    ActionRequest, AkrepError, PlanDiffItem, RiskLevel, ScanFinding, ScanHistoryEntry,
    ScanJobState, ScanProgress, ScanRequest, ScanVerdict, ScannerEngineState, ScannerStatusReport,
    SignatureDbStatus, SignatureFreshness, StateFingerprint, TransactionRecord,
    OPTIONAL_SCANNER_INSTALL_HINT,
};
use blaze_sentinel_core::{sha256_hex, AkrepSeverity, Availability};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::os::unix::io::AsRawFd;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use walkdir::WalkDir;

pub const DEFAULT_CLAMD_SOCKETS: &[&str] = &[
    "/run/clamd.scan/clamd.sock",
    "/var/run/clamd.scan/clamd.sock",
];
pub const DEFAULT_CLAMSCAN_BIN: &str = "/usr/bin/clamscan";
pub const DEFAULT_CLAMAV_DB_DIR: &str = "/var/lib/clamav";
pub const SIGNATURE_FRESH_MAX_SECS: u64 = 7 * 86_400;
pub const MAX_TRACKED_SCAN_JOBS: usize = 64;
const INSTREAM_CHUNK_SIZE: usize = 32 * 1024;
const MAX_SCAN_WALK_FILES: usize = 50_000;

pub fn validate_unix_socket_metadata(
    sock_path: &Path,
    enforce_system_owner: bool,
) -> Result<(), AkrepError> {
    let meta = fs::symlink_metadata(sock_path)?;
    if meta.file_type().is_symlink() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Symlink clamd socket path '{}' is forbidden",
            sock_path.display()
        )));
    }
    if !meta.file_type().is_socket() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Refusing non-socket clamd path '{}'",
            sock_path.display()
        )));
    }
    if enforce_system_owner {
        let mode = meta.mode() & 0o777;
        if (mode & 0o002) != 0 {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Refusing world-writable clamd socket '{}' (mode {:04o})",
                sock_path.display(),
                mode
            )));
        }
        if meta.uid() >= 1000 {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Refusing non-system clamd socket '{}' owned by uid={}",
                sock_path.display(),
                meta.uid()
            )));
        }
    }
    Ok(())
}

pub fn resolve_and_validate_scan_path(
    raw_target: &str,
    state_dir: Option<&Path>,
) -> Result<PathBuf, AkrepError> {
    let target = validate_scan_target_path(raw_target)?;
    if let Some(sd) = state_dir {
        if target == sd || target.starts_with(sd) {
            return Err(AkrepError::ForbiddenOperation(
                "Scanning inside Akrep state directory is forbidden".to_string(),
            ));
        }
    }

    let mut cur = PathBuf::from("/");
    for comp in target.components() {
        if let Component::Normal(seg) = comp {
            cur.push(seg);
            let meta = fs::symlink_metadata(&cur).map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    AkrepError::InvalidInput(format!(
                        "Scan target_path '{}' does not exist",
                        raw_target
                    ))
                } else {
                    AkrepError::from(e)
                }
            })?;
            if meta.file_type().is_symlink() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Symlink component '{}' in scan path is forbidden",
                    cur.display()
                )));
            }
        }
    }

    let canon = fs::canonicalize(&target).map_err(|_| {
        AkrepError::InvalidInput(format!("Scan target_path '{}' does not exist", raw_target))
    })?;
    let _ = validate_scan_target_path(&canon.to_string_lossy())?;

    if let Some(sd) = state_dir {
        if canon == sd || canon.starts_with(sd) {
            return Err(AkrepError::ForbiddenOperation(
                "Scanning inside Akrep state directory is forbidden".to_string(),
            ));
        }
        if let Ok(canon_sd) = fs::canonicalize(sd) {
            if canon == canon_sd || canon.starts_with(&canon_sd) {
                return Err(AkrepError::ForbiddenOperation(
                    "Scanning inside canonical Akrep state directory is forbidden".to_string(),
                ));
            }
        }
    }

    let root_meta = fs::symlink_metadata(&canon)?;
    if !root_meta.file_type().is_file() && !root_meta.file_type().is_dir() {
        return Err(AkrepError::ForbiddenOperation(
            "Scan target_path must be a regular file or directory".to_string(),
        ));
    }

    Ok(canon)
}

pub fn parse_cvd_header_version(header: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(header).ok()?;
    if !text.starts_with("ClamAV-VDB:") {
        return None;
    }
    let mut parts = text.split(':');
    let _magic = parts.next()?;
    let _build_time = parts.next()?;
    let ver_str = parts.next()?.trim();
    ver_str.parse::<u32>().ok()
}

pub fn inspect_signature_db_at(db_dir: &Path, now: SystemTime) -> SignatureDbStatus {
    if !db_dir.is_dir() {
        return SignatureDbStatus {
            freshness: SignatureFreshness::Unavailable,
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
            age_days: None,
            db_path: None,
            version: None,
            detail: format!(
                "ClamAV signature database directory '{}' not present",
                db_dir.display()
            ),
        };
    }

    let select_newest = |candidates: &[&str]| -> Option<(PathBuf, SystemTime)> {
        let mut best: Option<(PathBuf, SystemTime)> = None;
        for name in candidates {
            let p = db_dir.join(name);
            let Ok(meta) = fs::symlink_metadata(&p) else {
                continue;
            };
            if !meta.file_type().is_file() {
                continue;
            }
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            match &best {
                Some((_, best_t)) if mtime <= *best_t => {}
                _ => best = Some((p, mtime)),
            }
        }
        best
    };

    let chosen = select_newest(&["daily.cld", "daily.cvd"])
        .or_else(|| select_newest(&["main.cld", "main.cvd"]));

    let Some((db_file, mtime)) = chosen else {
        return SignatureDbStatus {
            freshness: SignatureFreshness::Unavailable,
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
            age_days: None,
            db_path: None,
            version: None,
            detail: "No ClamAV daily/main.{cvd,cld} signature database found".to_string(),
        };
    };

    let mut hdr = [0u8; 512];
    let version = fs::File::open(&db_file)
        .ok()
        .and_then(|mut f| f.read(&mut hdr).ok().map(|n| &hdr[..n]))
        .and_then(parse_cvd_header_version);

    let age_secs = now
        .duration_since(mtime)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    let age_days = age_secs / 86_400;

    if age_secs <= SIGNATURE_FRESH_MAX_SECS {
        SignatureDbStatus {
            freshness: SignatureFreshness::Healthy,
            severity: AkrepSeverity::Protected,
            availability: Availability::Available,
            age_days: Some(age_days),
            db_path: Some(db_file.to_string_lossy().to_string()),
            version,
            detail: format!(
                "Signature DB '{}' is fresh ({} day(s) old)",
                db_file.file_name().unwrap_or_default().to_string_lossy(),
                age_days
            ),
        }
    } else {
        SignatureDbStatus {
            freshness: SignatureFreshness::Warning,
            severity: AkrepSeverity::Attention,
            availability: Availability::Available,
            age_days: Some(age_days),
            db_path: Some(db_file.to_string_lossy().to_string()),
            version,
            detail: format!(
                "Signature DB '{}' is stale ({} day(s) old; > 7 days)",
                db_file.file_name().unwrap_or_default().to_string_lossy(),
                age_days
            ),
        }
    }
}

pub fn parse_clamd_instream_reply(raw: &str) -> ScanVerdict {
    let trimmed = raw.trim_matches(|c: char| c == '\0' || c.is_whitespace());
    if trimmed.is_empty() {
        return ScanVerdict::Unknown {
            reason: "Empty response from clamd INSTREAM".to_string(),
        };
    }

    let body = trimmed
        .strip_prefix("stream:")
        .map(str::trim)
        .unwrap_or(trimmed);

    if body.eq_ignore_ascii_case("OK") {
        return ScanVerdict::Clean;
    }

    if let Some(sig_part) = body.strip_suffix("FOUND") {
        let sig = sig_part.trim();
        if sig.is_empty() {
            return ScanVerdict::Unknown {
                reason: "Malformed FOUND response without signature name".to_string(),
            };
        }
        if sig.starts_with("PUA.") || sig.starts_with("Heuristics.Suspicious") {
            return ScanVerdict::Suspicious {
                reason: sig.to_string(),
            };
        }
        return ScanVerdict::Infected {
            signature: sig.to_string(),
        };
    }

    if let Some(err_part) = body.strip_suffix("ERROR") {
        return ScanVerdict::Unknown {
            reason: redact_secrets(err_part.trim()),
        };
    }

    ScanVerdict::Unknown {
        reason: redact_secrets(body),
    }
}

#[async_trait]
pub trait AvScanBackend: Send + Sync {
    fn inspect_engine(&self) -> Result<(ScannerEngineState, SignatureDbStatus), AkrepError>;
    async fn scan_file_stream(
        &self,
        path: &Path,
        cancel: &AtomicBool,
    ) -> Result<ScanVerdict, AkrepError>;
}

pub struct ClamdUnixSocketScanner {
    socket_paths: Vec<PathBuf>,
    clamscan_bin: PathBuf,
    db_dir: PathBuf,
    enforce_system_socket_ownership: bool,
}

impl ClamdUnixSocketScanner {
    pub fn new_system() -> Self {
        Self {
            socket_paths: DEFAULT_CLAMD_SOCKETS.iter().map(PathBuf::from).collect(),
            clamscan_bin: PathBuf::from(DEFAULT_CLAMSCAN_BIN),
            db_dir: PathBuf::from(DEFAULT_CLAMAV_DB_DIR),
            enforce_system_socket_ownership: true,
        }
    }

    pub fn with_config(
        socket_endpoints: &[String],
        clamscan_bin: PathBuf,
        db_dir: PathBuf,
    ) -> Result<Self, AkrepError> {
        Self::with_config_strict(socket_endpoints, clamscan_bin, db_dir, false)
    }

    pub fn with_config_strict(
        socket_endpoints: &[String],
        clamscan_bin: PathBuf,
        db_dir: PathBuf,
        enforce_system_socket_ownership: bool,
    ) -> Result<Self, AkrepError> {
        let mut socket_paths = Vec::with_capacity(socket_endpoints.len());
        for ep in socket_endpoints {
            socket_paths.push(validate_clamd_endpoint(ep)?);
        }
        Ok(Self {
            socket_paths,
            clamscan_bin,
            db_dir,
            enforce_system_socket_ownership,
        })
    }

    fn probe_listening_unix_socket(&self) -> Result<Option<PathBuf>, AkrepError> {
        for candidate in &self.socket_paths {
            let cand_str = candidate.to_string_lossy();
            let validated = validate_clamd_endpoint(&cand_str)?;
            match fs::symlink_metadata(&validated) {
                Ok(_) => {
                    validate_unix_socket_metadata(
                        &validated,
                        self.enforce_system_socket_ownership,
                    )?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(AkrepError::from(e)),
            }
            if StdUnixStream::connect(&validated).is_ok() {
                return Ok(Some(validated));
            }
        }
        Ok(None)
    }

    async fn stream_file_over_unix_socket(
        mut stream: UnixStream,
        target_file: &Path,
        cancel: &AtomicBool,
    ) -> Result<ScanVerdict, AkrepError> {
        let target_owned = target_file.to_path_buf();
        let mut file = tokio::task::spawn_blocking(move || {
            let canon = resolve_and_validate_scan_path(&target_owned.to_string_lossy(), None)?;
            let f = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&canon)
                .map_err(|e| {
                    if e.raw_os_error() == Some(libc::ELOOP) {
                        AkrepError::ForbiddenOperation(
                            "Symlink target rejected by scanner".to_string(),
                        )
                    } else {
                        AkrepError::from(e)
                    }
                })?;
            let meta = f.metadata()?;
            if !meta.file_type().is_file() {
                return Err(AkrepError::ForbiddenOperation(
                    "Non-regular file rejected by scanner".to_string(),
                ));
            }
            if let Ok(fd_target) = fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd())) {
                let _ = validate_scan_target_path(&fd_target.to_string_lossy())?;
            }
            Ok::<fs::File, AkrepError>(f)
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("file open join error: {}", e)))??;

        stream.write_all(b"zINSTREAM\0").await?;

        let mut early_close = false;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(AkrepError::InvalidInput(
                    "Scan cancelled by user".to_string(),
                ));
            }

            let (file_back, chunk) = tokio::task::spawn_blocking(move || {
                let mut buf = vec![0u8; INSTREAM_CHUNK_SIZE];
                let n = file.read(&mut buf)?;
                buf.truncate(n);
                Ok::<(fs::File, Vec<u8>), std::io::Error>((file, buf))
            })
            .await
            .map_err(|e| AkrepError::Internal(format!("chunk read join error: {}", e)))??;
            file = file_back;

            if chunk.is_empty() {
                break;
            }

            let len_be = (chunk.len() as u32).to_be_bytes();
            if let Err(w_err) = stream.write_all(&len_be).await {
                if matches!(
                    w_err.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ) {
                    early_close = true;
                    break;
                }
                return Err(AkrepError::from(w_err));
            }
            if let Err(w_err) = stream.write_all(&chunk).await {
                if matches!(
                    w_err.kind(),
                    std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset
                ) {
                    early_close = true;
                    break;
                }
                return Err(AkrepError::from(w_err));
            }
        }

        if !early_close {
            let _ = stream.write_all(&0u32.to_be_bytes()).await;
            let _ = stream.flush().await;
        }

        let mut resp_bytes = Vec::with_capacity(256);
        let mut one = [0u8; 1];
        let read_fut = async {
            while resp_bytes.len() < 4096 {
                match stream.read(&mut one).await {
                    Ok(0) => break,
                    Ok(_) => {
                        if one[0] == 0 || one[0] == b'\n' {
                            break;
                        }
                        resp_bytes.push(one[0]);
                    }
                    Err(e) => return Err(AkrepError::from(e)),
                }
            }
            Ok::<(), AkrepError>(())
        };

        tokio::time::timeout(Duration::from_secs(30), read_fut)
            .await
            .map_err(|_| {
                AkrepError::IoError("Timed out waiting for clamd INSTREAM verdict".to_string())
            })??;

        let reply_str = String::from_utf8_lossy(&resp_bytes);
        Ok(parse_clamd_instream_reply(&reply_str))
    }

    async fn scan_via_clamscan_cli(
        clamscan_bin: &Path,
        db_dir: &Path,
        target_file: &Path,
        cancel: &AtomicBool,
    ) -> Result<ScanVerdict, AkrepError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(AkrepError::InvalidInput(
                "Scan cancelled by user".to_string(),
            ));
        }

        let bin = clamscan_bin.to_path_buf();
        let db = db_dir.to_path_buf();
        let target = target_file.to_path_buf();

        tokio::task::spawn_blocking(move || {
            let canon_target = resolve_and_validate_scan_path(&target.to_string_lossy(), None)?;
            let meta = fs::symlink_metadata(&canon_target)?;
            if !meta.file_type().is_file() {
                return Err(AkrepError::ForbiddenOperation(
                    "Non-regular or symlink target rejected by clamscan fallback".to_string(),
                ));
            }

            let nice_path = Path::new("/usr/bin/nice");
            let mut cmd = if nice_path.is_file() {
                let mut c = command_c_locale(nice_path);
                c.args(["-n", "15"]).arg(&bin);
                c
            } else {
                command_c_locale(&bin)
            };

            cmd.arg(format!("--database={}", db.display()))
                .args(["--no-summary", "--stdout", "--"])
                .arg(&canon_target);

            let out = cmd.output()?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);

            match out.status.code() {
                Some(0) => Ok(ScanVerdict::Clean),
                Some(1) => {
                    let first_line = stdout.lines().next().unwrap_or_default().trim();
                    let rhs = first_line
                        .rsplit_once(':')
                        .map(|(_, r)| r.trim())
                        .unwrap_or(first_line);
                    Ok(parse_clamd_instream_reply(rhs))
                }
                _ => Ok(ScanVerdict::Unknown {
                    reason: redact_secrets(stderr.trim()),
                }),
            }
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("clamscan task join error: {}", e)))?
    }
}

#[async_trait]
impl AvScanBackend for ClamdUnixSocketScanner {
    fn inspect_engine(&self) -> Result<(ScannerEngineState, SignatureDbStatus), AkrepError> {
        let sig_db = inspect_signature_db_at(&self.db_dir, SystemTime::now());
        if let Some(sock) = self.probe_listening_unix_socket()? {
            return Ok((
                ScannerEngineState::ClamdSocket {
                    socket_path: sock.to_string_lossy().to_string(),
                },
                sig_db,
            ));
        }

        if self.clamscan_bin.is_file() && sig_db.availability == Availability::Available {
            return Ok((
                ScannerEngineState::ClamscanCli {
                    binary_path: self.clamscan_bin.to_string_lossy().to_string(),
                },
                sig_db,
            ));
        }

        Ok((
            ScannerEngineState::Unavailable {
                install_hint: OPTIONAL_SCANNER_INSTALL_HINT.to_string(),
            },
            sig_db,
        ))
    }

    async fn scan_file_stream(
        &self,
        path: &Path,
        cancel: &AtomicBool,
    ) -> Result<ScanVerdict, AkrepError> {
        for candidate in &self.socket_paths {
            let validated = validate_clamd_endpoint(&candidate.to_string_lossy())?;
            match fs::symlink_metadata(&validated) {
                Ok(_) => {
                    validate_unix_socket_metadata(
                        &validated,
                        self.enforce_system_socket_ownership,
                    )?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(AkrepError::from(e)),
            }

            match UnixStream::connect(&validated).await {
                Ok(stream) => {
                    return Self::stream_file_over_unix_socket(stream, path, cancel).await;
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    ) =>
                {
                    continue;
                }
                Err(e) => return Err(AkrepError::from(e)),
            }
        }

        let sig_db = inspect_signature_db_at(&self.db_dir, SystemTime::now());
        if self.clamscan_bin.is_file() && sig_db.availability == Availability::Available {
            return Self::scan_via_clamscan_cli(&self.clamscan_bin, &self.db_dir, path, cancel)
                .await;
        }
        Err(AkrepError::BackendUnavailable(format!(
            "Optional ClamAV scanner is unavailable (install hint: {})",
            OPTIONAL_SCANNER_INSTALL_HINT
        )))
    }
}

pub struct MockClamScanner {
    engine_state: Mutex<ScannerEngineState>,
    sig_db: Mutex<SignatureDbStatus>,
    per_file_delay: Mutex<Duration>,
    infected_markers: Mutex<Vec<(Vec<u8>, String)>>,
}

impl Default for MockClamScanner {
    fn default() -> Self {
        Self::new_healthy()
    }
}

impl MockClamScanner {
    pub fn new_healthy() -> Self {
        let eicar = build_eicar_test_bytes();
        Self {
            engine_state: Mutex::new(ScannerEngineState::ClamdSocket {
                socket_path: "/run/clamd.scan/clamd.sock".to_string(),
            }),
            sig_db: Mutex::new(SignatureDbStatus {
                freshness: SignatureFreshness::Healthy,
                severity: AkrepSeverity::Protected,
                availability: Availability::Available,
                age_days: Some(1),
                db_path: Some("/var/lib/clamav/daily.cld".to_string()),
                version: Some(27450),
                detail: "Mock signature DB fresh (1 day old)".to_string(),
            }),
            per_file_delay: Mutex::new(Duration::ZERO),
            infected_markers: Mutex::new(vec![(eicar, "Win.Test.EICAR_HDB-1".to_string())]),
        }
    }

    pub fn new_unavailable() -> Self {
        Self {
            engine_state: Mutex::new(ScannerEngineState::Unavailable {
                install_hint: OPTIONAL_SCANNER_INSTALL_HINT.to_string(),
            }),
            sig_db: Mutex::new(SignatureDbStatus {
                freshness: SignatureFreshness::Unavailable,
                severity: AkrepSeverity::Unavailable,
                availability: Availability::Unavailable,
                age_days: None,
                db_path: None,
                version: None,
                detail: "ClamAV not installed".to_string(),
            }),
            per_file_delay: Mutex::new(Duration::ZERO),
            infected_markers: Mutex::new(Vec::new()),
        }
    }

    pub fn set_engine_state(&self, state: ScannerEngineState) {
        *self.engine_state.lock().expect("mock engine mutex") = state;
    }

    pub fn set_signature_db(&self, db: SignatureDbStatus) {
        *self.sig_db.lock().expect("mock sig_db mutex") = db;
    }

    pub fn set_per_file_delay(&self, delay: Duration) {
        *self.per_file_delay.lock().expect("mock delay mutex") = delay;
    }
}

pub fn build_eicar_test_bytes() -> Vec<u8> {
    // Split standard 68-byte EICAR anti-malware test string across runtime fragments
    let p1 = b"X5O!P%@AP[4\\PZX54(P^)7CC)7}";
    let p2 = b"$EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*";
    let mut out = Vec::with_capacity(p1.len() + p2.len());
    out.extend_from_slice(p1);
    out.extend_from_slice(p2);
    out
}

#[async_trait]
impl AvScanBackend for MockClamScanner {
    fn inspect_engine(&self) -> Result<(ScannerEngineState, SignatureDbStatus), AkrepError> {
        Ok((
            self.engine_state.lock().expect("mock engine mutex").clone(),
            self.sig_db.lock().expect("mock sig_db mutex").clone(),
        ))
    }

    async fn scan_file_stream(
        &self,
        path: &Path,
        cancel: &AtomicBool,
    ) -> Result<ScanVerdict, AkrepError> {
        if cancel.load(Ordering::SeqCst) {
            return Err(AkrepError::InvalidInput(
                "Scan cancelled by user".to_string(),
            ));
        }
        let delay = *self.per_file_delay.lock().expect("mock delay mutex");
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
            if cancel.load(Ordering::SeqCst) {
                return Err(AkrepError::InvalidInput(
                    "Scan cancelled by user".to_string(),
                ));
            }
        }
        let bytes = fs::read(path)?;
        let markers = self.infected_markers.lock().expect("mock markers").clone();
        for (needle, sig) in markers {
            if bytes.windows(needle.len()).any(|w| w == needle.as_slice()) {
                return Ok(ScanVerdict::Infected { signature: sig });
            }
        }
        Ok(ScanVerdict::Clean)
    }
}

pub trait ScannerInstallerOps: Send + Sync {
    fn is_installed(&self) -> Result<bool, AkrepError>;
    fn install_optional_packages(&self) -> Result<String, AkrepError>;
    fn remove_optional_packages(&self) -> Result<String, AkrepError>;
}

pub struct LinuxScannerInstallerOps;

impl ScannerInstallerOps for LinuxScannerInstallerOps {
    fn is_installed(&self) -> Result<bool, AkrepError> {
        let rpm_bin = if Path::new("/bin/rpm").is_file() {
            "/bin/rpm"
        } else {
            "/usr/bin/rpm"
        };
        if !Path::new(rpm_bin).is_file() {
            return Ok(false);
        }
        let ok = command_c_locale(rpm_bin)
            .args(["-q", "clamav", "clamd", "clamav-freshclam"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        Ok(ok)
    }

    fn install_optional_packages(&self) -> Result<String, AkrepError> {
        let dnf_bin = Path::new("/usr/bin/dnf5");
        if !dnf_bin.is_file() {
            return Err(AkrepError::BackendUnavailable(
                "/usr/bin/dnf5 is not available on this system".to_string(),
            ));
        }
        // Strictly install on-demand ClamAV packages without shell; clamonacc is NEVER installed or enabled.
        let out = command_c_locale(dnf_bin)
            .args(["install", "-y", "clamav", "clamd", "clamav-freshclam"])
            .output()?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "dnf5 install failed: {}",
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok("Installed optional scanner packages: clamav, clamd, clamav-freshclam".to_string())
    }

    fn remove_optional_packages(&self) -> Result<String, AkrepError> {
        let dnf_bin = Path::new("/usr/bin/dnf5");
        if !dnf_bin.is_file() {
            return Err(AkrepError::BackendUnavailable(
                "/usr/bin/dnf5 is not available for rollback".to_string(),
            ));
        }
        let out = command_c_locale(dnf_bin)
            .args(["remove", "-y", "clamav", "clamd", "clamav-freshclam"])
            .output()?;
        if !out.status.success() {
            return Err(AkrepError::RollbackFailed(format!(
                "dnf5 remove failed during rollback: {}",
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok("Removed optional scanner packages on rollback".to_string())
    }
}

#[derive(Default)]
pub struct MockScannerInstallerOps {
    installed: Mutex<bool>,
    recorded_ops: Mutex<Vec<String>>,
}

impl MockScannerInstallerOps {
    pub fn new(installed: bool) -> Self {
        Self {
            installed: Mutex::new(installed),
            recorded_ops: Mutex::new(Vec::new()),
        }
    }

    pub fn recorded_ops(&self) -> Vec<String> {
        self.recorded_ops.lock().expect("mock ops").clone()
    }
}

impl ScannerInstallerOps for MockScannerInstallerOps {
    fn is_installed(&self) -> Result<bool, AkrepError> {
        Ok(*self.installed.lock().expect("mock installed"))
    }

    fn install_optional_packages(&self) -> Result<String, AkrepError> {
        *self.installed.lock().expect("mock installed") = true;
        self.recorded_ops
            .lock()
            .expect("mock ops")
            .push("dnf5 install -y clamav clamd clamav-freshclam".to_string());
        Ok("Mock installed clamav, clamd, clamav-freshclam".to_string())
    }

    fn remove_optional_packages(&self) -> Result<String, AkrepError> {
        *self.installed.lock().expect("mock installed") = false;
        self.recorded_ops
            .lock()
            .expect("mock ops")
            .push("dnf5 remove -y clamav clamd clamav-freshclam".to_string());
        Ok("Mock removed clamav, clamd, clamav-freshclam".to_string())
    }
}

struct ScanJobHandle {
    progress: Mutex<ScanProgress>,
    cancel_flag: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OptionalScannerInstallSnapshot {
    previously_installed: bool,
}

#[derive(Clone)]
pub struct ScannerAdapter {
    state_dir: PathBuf,
    backend: Arc<dyn AvScanBackend>,
    installer: Arc<dyn ScannerInstallerOps>,
    jobs: Arc<Mutex<HashMap<String, Arc<ScanJobHandle>>>>,
    history_lock: Arc<Mutex<()>>,
    allow_test_socket_override: bool,
}

impl ScannerAdapter {
    pub fn new(
        state_dir: PathBuf,
        backend: Arc<dyn AvScanBackend>,
        installer: Arc<dyn ScannerInstallerOps>,
    ) -> Self {
        Self::with_options(state_dir, backend, installer, false)
    }

    pub fn with_options(
        state_dir: PathBuf,
        backend: Arc<dyn AvScanBackend>,
        installer: Arc<dyn ScannerInstallerOps>,
        allow_test_socket_override: bool,
    ) -> Self {
        Self {
            state_dir,
            backend,
            installer,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            history_lock: Arc::new(Mutex::new(())),
            allow_test_socket_override,
        }
    }

    pub fn new_system(state_dir: PathBuf) -> Self {
        Self::with_options(
            state_dir,
            Arc::new(ClamdUnixSocketScanner::new_system()),
            Arc::new(LinuxScannerInstallerOps),
            false,
        )
    }

    pub fn has_active_scans(&self) -> bool {
        let jobs = self.jobs.lock().expect("scan jobs mutex");
        jobs.values()
            .any(|h| h.progress.lock().expect("progress mutex").state == ScanJobState::Running)
    }

    pub fn history_path(&self) -> PathBuf {
        self.state_dir.join("scan_history.json")
    }

    pub fn load_history(&self) -> Vec<ScanHistoryEntry> {
        let p = self.history_path();
        let Ok(raw) = fs::read_to_string(&p) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<ScanHistoryEntry>>(&raw).unwrap_or_default()
    }

    fn append_history_entry(&self, entry: ScanHistoryEntry) {
        let _guard = self.history_lock.lock().expect("scan history mutex");
        let _ = fs::create_dir_all(&self.state_dir);
        let mut entries = self.load_history();
        entries.push(entry);
        if entries.len() > 200 {
            let drop_count = entries.len() - 200;
            entries.drain(0..drop_count);
        }
        let _ = write_json_atomic_0600(&self.history_path(), &entries);
    }

    pub fn inspect_status(&self) -> Result<ScannerStatusReport, AkrepError> {
        let (engine_state, signature_db) = self.backend.inspect_engine()?;
        let (availability, severity) = match &engine_state {
            ScannerEngineState::Unavailable { .. } => {
                (Availability::Unavailable, AkrepSeverity::Unavailable)
            }
            ScannerEngineState::ClamdSocket { .. } => {
                let sev = match signature_db.freshness {
                    SignatureFreshness::Healthy => AkrepSeverity::Protected,
                    SignatureFreshness::Warning => AkrepSeverity::Attention,
                    SignatureFreshness::Unavailable => AkrepSeverity::Attention,
                };
                (Availability::Available, sev)
            }
            ScannerEngineState::ClamscanCli { .. } => {
                (Availability::Available, signature_db.severity)
            }
        };

        Ok(ScannerStatusReport {
            engine_state,
            signature_db,
            // ClamAV on-access daemon (clamonacc) is NEVER started or claimed active in Akrep.
            realtime_protection_active: false,
            severity,
            availability,
            recent_history: self.load_history(),
        })
    }

    pub async fn start_scan(
        &self,
        req: ScanRequest,
        actor_uid: u32,
    ) -> Result<ScanProgress, AkrepError> {
        req.validate_red_lines()?;
        if req.socket_override.is_some() && !self.allow_test_socket_override {
            return Err(AkrepError::ForbiddenOperation(
                "Custom clamd socket_override is forbidden on production scanner".to_string(),
            ));
        }
        let target_path = resolve_and_validate_scan_path(&req.target_path, Some(&self.state_dir))?;
        let local_quarantine = self.state_dir.join("quarantine");

        let scan_backend: Arc<dyn AvScanBackend> = if let Some(sock_override) = &req.socket_override
        {
            let validated = validate_clamd_endpoint(sock_override)?;
            Arc::new(ClamdUnixSocketScanner::with_config(
                &[validated.to_string_lossy().to_string()],
                PathBuf::from("/nonexistent/clamscan"),
                PathBuf::from(DEFAULT_CLAMAV_DB_DIR),
            )?)
        } else {
            Arc::clone(&self.backend)
        };

        let (engine_state, _) = scan_backend.inspect_engine()?;
        if let ScannerEngineState::Unavailable { install_hint } = engine_state {
            return Err(AkrepError::BackendUnavailable(format!(
                "Optional ClamAV scanner is not installed (install optional packages: {})",
                install_hint
            )));
        }

        let job_id = generate_uuid_v4()?;
        let started_at = Utc::now().to_rfc3339();
        let initial_progress = ScanProgress {
            job_id: job_id.clone(),
            target_path: req.target_path.clone(),
            scanned_files: 0,
            total_files: 0,
            percent: 0,
            current_path: None,
            findings: Vec::new(),
            state: ScanJobState::Running,
            started_at: started_at.clone(),
            finished_at: None,
            error_message: None,
        };

        let cancel_flag = Arc::new(AtomicBool::new(false));
        let handle = Arc::new(ScanJobHandle {
            progress: Mutex::new(initial_progress.clone()),
            cancel_flag: Arc::clone(&cancel_flag),
        });
        {
            let mut jobs = self.jobs.lock().expect("scan jobs mutex");
            if jobs.len() >= MAX_TRACKED_SCAN_JOBS {
                let mut terminal_ids: Vec<(String, String)> = jobs
                    .iter()
                    .filter_map(|(id, h)| {
                        let p = h.progress.lock().expect("progress mutex");
                        if p.state != ScanJobState::Running {
                            Some((id.clone(), p.started_at.clone()))
                        } else {
                            None
                        }
                    })
                    .collect();
                terminal_ids.sort_by(|a, b| a.1.cmp(&b.1));
                for (evict_id, _) in terminal_ids {
                    if jobs.len() < MAX_TRACKED_SCAN_JOBS {
                        break;
                    }
                    jobs.remove(&evict_id);
                }
                if jobs.len() >= MAX_TRACKED_SCAN_JOBS {
                    return Err(AkrepError::TransactionConflict(format!(
                        "Maximum concurrent scan jobs ({}) reached",
                        MAX_TRACKED_SCAN_JOBS
                    )));
                }
            }
            jobs.insert(job_id.clone(), Arc::clone(&handle));
        }

        let adapter_clone = self.clone();
        let target_clone = target_path.clone();
        let recursive = req.recursive;
        let quarantine_skip = local_quarantine;
        let job_id_clone = job_id.clone();

        let join_handle = tokio::spawn(async move {
            adapter_clone
                .execute_scan_worker(
                    job_id_clone,
                    target_clone,
                    recursive,
                    quarantine_skip,
                    actor_uid,
                    started_at,
                    scan_backend,
                    handle,
                    cancel_flag,
                )
                .await;
        });

        if req.synchronous {
            let _ = join_handle.await;
            return self.get_scan_status(&job_id);
        }

        tokio::task::yield_now().await;
        self.get_scan_status(&job_id)
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_scan_worker(
        &self,
        job_id: String,
        target_path: PathBuf,
        recursive: bool,
        quarantine_skip: PathBuf,
        actor_uid: u32,
        started_at: String,
        scan_backend: Arc<dyn AvScanBackend>,
        handle: Arc<ScanJobHandle>,
        cancel_flag: Arc<AtomicBool>,
    ) {
        let cancel_for_walk = Arc::clone(&cancel_flag);
        let target_for_walk = target_path.clone();
        let q_skip = quarantine_skip.clone();

        let walk_res =
            tokio::task::spawn_blocking(move || -> Result<Vec<(PathBuf, u64)>, AkrepError> {
                let meta = fs::symlink_metadata(&target_for_walk)?;
                if meta.file_type().is_file() {
                    return Ok(vec![(target_for_walk, meta.len())]);
                }

                let mut walker = WalkDir::new(&target_for_walk).follow_links(false);
                if !recursive {
                    walker = walker.max_depth(1);
                }

                let mut files = Vec::new();
                let it = walker.into_iter().filter_entry(|entry| {
                    if entry.path_is_symlink() {
                        return false;
                    }
                    let p = entry.path();
                    if p == q_skip || p.starts_with(&q_skip) {
                        return false;
                    }
                    let s = p.to_string_lossy();
                    for skip_prefix in
                        ["/proc", "/sys", "/dev", "/run", "/var/lib/akrep/quarantine"]
                    {
                        if s == skip_prefix || s.starts_with(&format!("{}/", skip_prefix)) {
                            return false;
                        }
                    }
                    true
                });

                for entry_res in it {
                    if cancel_for_walk.load(Ordering::SeqCst) {
                        return Err(AkrepError::InvalidInput("Scan cancelled".to_string()));
                    }
                    let Ok(entry) = entry_res else {
                        continue;
                    };
                    if entry.path_is_symlink() || !entry.file_type().is_file() {
                        continue;
                    }
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    files.push((entry.into_path(), size));
                    if files.len() >= MAX_SCAN_WALK_FILES {
                        break;
                    }
                }
                files.sort_by(|a, b| a.0.cmp(&b.0));
                Ok(files)
            })
            .await;

        let files = match walk_res {
            Ok(Ok(list)) => list,
            Ok(Err(err)) => {
                let done_at = Utc::now().to_rfc3339();
                let final_state = if cancel_flag.load(Ordering::SeqCst) {
                    ScanJobState::Cancelled
                } else {
                    ScanJobState::Failed
                };
                let mut g = handle.progress.lock().expect("progress mutex");
                g.state = final_state;
                g.finished_at = Some(done_at.clone());
                g.current_path = None;
                if final_state == ScanJobState::Failed {
                    g.error_message = Some(redact_secrets(err.message()));
                }
                let scanned = g.scanned_files;
                let fcount = g.findings.len();
                let tpath = g.target_path.clone();
                drop(g);
                self.append_history_entry(ScanHistoryEntry {
                    job_id,
                    target_path: tpath,
                    scanned_files: scanned,
                    findings_count: fcount,
                    state: final_state,
                    started_at,
                    finished_at: done_at,
                    actor_uid,
                });
                return;
            }
            Err(join_err) => {
                let done_at = Utc::now().to_rfc3339();
                let mut g = handle.progress.lock().expect("progress mutex");
                g.state = ScanJobState::Failed;
                g.finished_at = Some(done_at);
                g.current_path = None;
                g.error_message = Some(format!("walk join error: {}", join_err));
                return;
            }
        };

        let total_files = files.len();
        {
            let mut g = handle.progress.lock().expect("progress mutex");
            g.total_files = total_files;
            if total_files == 0 {
                g.percent = 100;
            }
        }

        for (idx, (file_path, size_bytes)) in files.into_iter().enumerate() {
            if cancel_flag.load(Ordering::SeqCst) {
                let done_at = Utc::now().to_rfc3339();
                let (scanned, fcount, tpath) = {
                    let mut g = handle.progress.lock().expect("progress mutex");
                    g.state = ScanJobState::Cancelled;
                    g.current_path = None;
                    g.finished_at = Some(done_at.clone());
                    (g.scanned_files, g.findings.len(), g.target_path.clone())
                };
                self.append_history_entry(ScanHistoryEntry {
                    job_id,
                    target_path: tpath,
                    scanned_files: scanned,
                    findings_count: fcount,
                    state: ScanJobState::Cancelled,
                    started_at,
                    finished_at: done_at,
                    actor_uid,
                });
                return;
            }

            let file_str = file_path.to_string_lossy().to_string();
            {
                let mut g = handle.progress.lock().expect("progress mutex");
                g.current_path = Some(file_str.clone());
            }

            let scan_res = scan_backend
                .scan_file_stream(&file_path, &cancel_flag)
                .await;
            if cancel_flag.load(Ordering::SeqCst) {
                let done_at = Utc::now().to_rfc3339();
                let (scanned, fcount, tpath) = {
                    let mut g = handle.progress.lock().expect("progress mutex");
                    g.state = ScanJobState::Cancelled;
                    g.current_path = None;
                    g.finished_at = Some(done_at.clone());
                    (g.scanned_files, g.findings.len(), g.target_path.clone())
                };
                self.append_history_entry(ScanHistoryEntry {
                    job_id,
                    target_path: tpath,
                    scanned_files: scanned,
                    findings_count: fcount,
                    state: ScanJobState::Cancelled,
                    started_at,
                    finished_at: done_at,
                    actor_uid,
                });
                return;
            }

            let verdict = match scan_res {
                Ok(v) => v,
                Err(e) => ScanVerdict::Unknown {
                    reason: redact_secrets(e.message()),
                },
            };

            let finding_opt = match &verdict {
                ScanVerdict::Clean => None,
                ScanVerdict::Infected { signature } => Some(ScanFinding {
                    path: file_str,
                    verdict: verdict.clone(),
                    threat_name: Some(signature.clone()),
                    size_bytes,
                }),
                ScanVerdict::Suspicious { reason } => Some(ScanFinding {
                    path: file_str,
                    verdict: verdict.clone(),
                    threat_name: Some(reason.clone()),
                    size_bytes,
                }),
                ScanVerdict::Unknown { .. } => Some(ScanFinding {
                    path: file_str,
                    verdict: verdict.clone(),
                    threat_name: None,
                    size_bytes,
                }),
            };

            let completed_count = idx + 1;
            let pct = (completed_count * 100)
                .checked_div(total_files)
                .unwrap_or(100)
                .min(100) as u8;
            {
                let mut g = handle.progress.lock().expect("progress mutex");
                g.scanned_files = completed_count;
                g.percent = pct;
                if let Some(f) = finding_opt {
                    g.findings.push(f);
                }
            }
        }

        let done_at = Utc::now().to_rfc3339();
        let (scanned, fcount, tpath) = {
            let mut g = handle.progress.lock().expect("progress mutex");
            g.state = ScanJobState::Completed;
            g.percent = 100;
            g.current_path = None;
            g.finished_at = Some(done_at.clone());
            (g.scanned_files, g.findings.len(), g.target_path.clone())
        };
        self.append_history_entry(ScanHistoryEntry {
            job_id,
            target_path: tpath,
            scanned_files: scanned,
            findings_count: fcount,
            state: ScanJobState::Completed,
            started_at,
            finished_at: done_at,
            actor_uid,
        });
    }

    pub fn get_scan_status(&self, job_id: &str) -> Result<ScanProgress, AkrepError> {
        let handle = {
            let jobs = self.jobs.lock().expect("scan jobs mutex");
            jobs.get(job_id).cloned().ok_or_else(|| {
                AkrepError::InvalidInput(format!("Unknown scan job_id '{}'", job_id))
            })?
        };
        let snap = handle.progress.lock().expect("progress mutex").clone();
        Ok(snap)
    }

    pub fn cancel_scan(&self, job_id: &str) -> Result<ScanProgress, AkrepError> {
        let handle = {
            let jobs = self.jobs.lock().expect("scan jobs mutex");
            jobs.get(job_id).cloned().ok_or_else(|| {
                AkrepError::InvalidInput(format!("Unknown scan job_id '{}'", job_id))
            })?
        };
        handle.cancel_flag.store(true, Ordering::SeqCst);
        let mut g = handle.progress.lock().expect("progress mutex");
        if g.state == ScanJobState::Running {
            g.state = ScanJobState::Cancelled;
            g.current_path = None;
            if g.finished_at.is_none() {
                g.finished_at = Some(Utc::now().to_rfc3339());
            }
        }
        Ok(g.clone())
    }

    fn installer_fingerprint(&self) -> Result<StateFingerprint, AkrepError> {
        let installed = self.installer.is_installed()?;
        let payload = format!("optional_scanner.installed={}", installed);
        Ok(StateFingerprint {
            target: "packages.optional_scanner".to_string(),
            sha256: sha256_hex(payload.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        })
    }
}

#[async_trait]
impl MutationBackend for ScannerAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        if !matches!(req, ActionRequest::InstallOptionalScanner) {
            return Err(AkrepError::InvalidInput(
                "ScannerAdapter only mutates InstallOptionalScanner".to_string(),
            ));
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            let installed = this.installer.is_installed()?;
            let fp = this.installer_fingerprint()?;
            Ok(PreviewOutcome {
                risk: RiskLevel::Medium,
                diffs: vec![PlanDiffItem {
                    target: "packages.optional_scanner".to_string(),
                    current_value: if installed {
                        "installed".to_string()
                    } else {
                        "absent".to_string()
                    },
                    desired_value: "installed (clamav, clamd, clamav-freshclam)".to_string(),
                    description_tr: "Istege bagli ClamAV talep uzerine tarama araclari (clamav, clamd, clamav-freshclam) DNF5 ile kurulacak (clamonacc etkinlestirilmez)."
                        .to_string(),
                    description_en: "Install optional on-demand ClamAV scanner packages (clamav, clamd, clamav-freshclam) via DNF5 without enabling clamonacc."
                        .to_string(),
                }],
                pre_fingerprints: vec![fp],
                warnings_tr: vec![
                    "ClamAV yalnizca talep uzerine (on-demand) dosya taramasi saglar; gercek zamanli cekirdek/clamonacc taramasi acilmaz.".to_string(),
                ],
                warnings_en: vec![
                    "ClamAV provides on-demand scanning only; clamonacc real-time access scanning is never enabled.".to_string(),
                ],
            })
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("installer preview join error: {}", e)))?
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        if !matches!(req, ActionRequest::InstallOptionalScanner) {
            return Err(AkrepError::InvalidInput(
                "ScannerAdapter only mutates InstallOptionalScanner".to_string(),
            ));
        }
        let this = self.clone();
        let snap_dir = snapshot_dir.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let previously_installed = this.installer.is_installed()?;
            let snap = OptionalScannerInstallSnapshot {
                previously_installed,
            };
            write_json_atomic_0600(&snap_dir.join("optional_scanner.json"), &snap)?;
            let summary = this.installer.install_optional_packages()?;
            Ok(ApplyOutcome {
                pre_snapshot: serde_json::to_value(&snap)
                    .map_err(|e| AkrepError::Internal(e.to_string()))?,
                target_state: serde_json::json!({
                    "installed": true,
                    "packages": ["clamav", "clamd", "clamav-freshclam"],
                }),
                summary,
            })
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("installer apply join error: {}", e)))?
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        _expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        if !matches!(req, ActionRequest::InstallOptionalScanner) {
            return Err(AkrepError::InvalidInput(
                "ScannerAdapter only mutates InstallOptionalScanner".to_string(),
            ));
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || {
            if !this.installer.is_installed()? {
                return Err(AkrepError::VerifyFailed(
                    "Optional ClamAV packages failed post-install verification".to_string(),
                ));
            }
            Ok(vec![this.installer_fingerprint()?])
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("installer verify join error: {}", e)))?
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        _post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        if !matches!(req, ActionRequest::InstallOptionalScanner) {
            return Err(AkrepError::InvalidInput(
                "ScannerAdapter only mutates InstallOptionalScanner".to_string(),
            ));
        }
        let this = self.clone();
        tokio::task::spawn_blocking(move || Ok(vec![this.installer_fingerprint()?]))
            .await
            .map_err(|e| AkrepError::Internal(format!("installer fp join error: {}", e)))?
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        if !matches!(record.request, ActionRequest::InstallOptionalScanner) {
            return Err(AkrepError::InvalidInput(
                "ScannerAdapter only rolls back InstallOptionalScanner".to_string(),
            ));
        }
        let this = self.clone();
        let snap_dir = snapshot_dir.to_path_buf();
        let pre_val = record.pre_snapshot.clone();
        tokio::task::spawn_blocking(move || {
            let snap_file = snap_dir.join("optional_scanner.json");
            if !snap_file.is_file() && pre_val.is_null() {
                return Ok("Optional scanner install aborted before mutation".to_string());
            }
            let snap: OptionalScannerInstallSnapshot = if snap_file.is_file() {
                serde_json::from_str(&fs::read_to_string(&snap_file)?)
                    .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
            } else {
                serde_json::from_value(pre_val)
                    .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
            };
            if snap.previously_installed {
                return Ok(
                    "Optional scanner packages were already installed prior to transaction; keeping installed"
                        .to_string(),
                );
            }
            this.installer.remove_optional_packages()
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("installer rollback join error: {}", e)))?
    }
}
