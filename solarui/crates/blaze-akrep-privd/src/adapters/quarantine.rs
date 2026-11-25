use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    generate_uuid_v4, redact_secrets, ActionRequest, AkrepError, PlanDiffItem, QuarantineInventory,
    QuarantineMetadata, RiskLevel, StateFingerprint, TransactionRecord,
};
use blaze_sentinel_core::sha256_hex;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

pub const QPKG_MAGIC: &[u8; 8] = b"AKREPQ1\0";
const QPKG_KEY_LEN: usize = 32;
const QPKG_HEADER_LEN: usize = 8 + QPKG_KEY_LEN + 8;
const STREAM_BUF_LEN: usize = 64 * 1024;

pub const FORBIDDEN_QUARANTINE_ROOTS: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/etc",
    "/boot",
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/var/lib/akrep",
];

pub struct Sha256Hasher {
    state: [u32; 8],
    buf: [u8; 64],
    buf_len: usize,
    total_bytes: u64,
}

impl Default for Sha256Hasher {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256Hasher {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    pub fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
                0x5be0cd19,
            ],
            buf: [0u8; 64],
            buf_len: 0,
            total_bytes: 0,
        }
    }

    fn compress_block(state: &mut [u32; 8], chunk: &[u8; 64]) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = *state;
        for (i, &w_i) in w.iter().enumerate() {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(Self::K[i])
                .wrapping_add(w_i);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
        state[4] = state[4].wrapping_add(e);
        state[5] = state[5].wrapping_add(f);
        state[6] = state[6].wrapping_add(g);
        state[7] = state[7].wrapping_add(hh);
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.total_bytes = self.total_bytes.wrapping_add(data.len() as u64);
        if self.buf_len > 0 {
            let need = 64 - self.buf_len;
            if data.len() < need {
                self.buf[self.buf_len..self.buf_len + data.len()].copy_from_slice(data);
                self.buf_len += data.len();
                return;
            }
            self.buf[self.buf_len..64].copy_from_slice(&data[..need]);
            let block = self.buf;
            Self::compress_block(&mut self.state, &block);
            self.buf_len = 0;
            data = &data[need..];
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            Self::compress_block(&mut self.state, &block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.buf[..data.len()].copy_from_slice(data);
            self.buf_len = data.len();
        }
    }

    pub fn finalize_hex(mut self) -> String {
        let bit_len = self.total_bytes.wrapping_mul(8);
        self.buf[self.buf_len] = 0x80;
        self.buf_len += 1;
        if self.buf_len > 56 {
            self.buf[self.buf_len..64].fill(0);
            let block = self.buf;
            Self::compress_block(&mut self.state, &block);
            self.buf_len = 0;
        }
        self.buf[self.buf_len..56].fill(0);
        self.buf[56..64].copy_from_slice(&bit_len.to_be_bytes());
        let block = self.buf;
        Self::compress_block(&mut self.state, &block);
        self.state.iter().map(|v| format!("{:08x}", v)).collect()
    }
}

fn generate_non_zero_xor_key() -> Result<[u8; QPKG_KEY_LEN], AkrepError> {
    let mut key = [0u8; QPKG_KEY_LEN];
    let mut urandom = File::open("/dev/urandom")?;
    urandom.read_exact(&mut key)?;
    for b in &mut key {
        if *b == 0 {
            *b = 0xA5;
        }
    }
    Ok(key)
}

fn check_forbidden_roots(path_str: &str, vault_dir: &Path) -> Result<(), AkrepError> {
    if path_str == "/" {
        return Err(AkrepError::ForbiddenOperation(
            "Root filesystem '/' is forbidden in quarantine".to_string(),
        ));
    }
    for root in FORBIDDEN_QUARANTINE_ROOTS {
        if path_str == *root || path_str.starts_with(&format!("{}/", root)) {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Quarantining or restoring inside protected system path '{}' is forbidden",
                root
            )));
        }
    }
    let vault_str = vault_dir.to_string_lossy();
    if path_str == vault_str.as_ref() || path_str.starts_with(&format!("{}/", vault_str)) {
        return Err(AkrepError::ForbiddenOperation(
            "Operating recursively inside the quarantine vault directory is forbidden".to_string(),
        ));
    }
    Ok(())
}

fn validate_quarantine_target_path(
    path_str: &str,
    vault_dir: &Path,
) -> Result<PathBuf, AkrepError> {
    if path_str.is_empty()
        || path_str.chars().any(|c| {
            c.is_control()
                || matches!(
                    c,
                    ';' | '&' | '|' | '`' | '$' | '(' | ')' | '<' | '>' | '\\' | '"' | '\''
                )
        })
    {
        return Err(AkrepError::InvalidInput(
            "Quarantine path contains invalid or forbidden characters".to_string(),
        ));
    }
    let path = Path::new(path_str);
    if !path.is_absolute() {
        return Err(AkrepError::InvalidInput(
            "Quarantine target path must be absolute".to_string(),
        ));
    }
    for comp in path.components() {
        if matches!(comp, Component::ParentDir | Component::CurDir) {
            return Err(AkrepError::ForbiddenOperation(
                "Path traversal ('..' or '.') is forbidden in quarantine path".to_string(),
            ));
        }
    }
    check_forbidden_roots(path_str, vault_dir)?;
    Ok(path.to_path_buf())
}

fn ensure_vault_dir_0700(vault_dir: &Path) -> Result<OwnedFd, AkrepError> {
    if let Ok(meta) = fs::symlink_metadata(vault_dir) {
        if meta.file_type().is_symlink() {
            return Err(AkrepError::ForbiddenOperation(
                "Quarantine vault path must not be a symlink".to_string(),
            ));
        }
    }
    fs::create_dir_all(vault_dir)?;
    let c_vault = CString::new(vault_dir.as_os_str().as_bytes())
        .map_err(|_| AkrepError::InvalidInput("Vault path contains NUL byte".to_string()))?;
    // SAFETY: `c_vault` is a valid NUL-terminated path string; descriptor is wrapped immediately in `OwnedFd`.
    let raw_fd = unsafe {
        libc::open(
            c_vault.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw_fd < 0 {
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ELOOP) {
            return Err(AkrepError::ForbiddenOperation(
                "Quarantine vault path is a symlink".to_string(),
            ));
        }
        return Err(AkrepError::from(err));
    }
    // SAFETY: `raw_fd >= 0` was just returned by `libc::open`.
    let dir_fd = unsafe { OwnedFd::from_raw_fd(raw_fd) };
    // SAFETY: `dir_fd` is a valid open directory descriptor.
    let rc = unsafe { libc::fchmod(dir_fd.as_raw_fd(), 0o700) };
    if rc != 0 {
        return Err(AkrepError::from(std::io::Error::last_os_error()));
    }
    Ok(dir_fd)
}

fn open_parent_dir_nofollow(parent: &Path, vault_dir: &Path) -> Result<OwnedFd, AkrepError> {
    let root_c = CString::new("/").expect("static root cstr");
    // SAFETY: `root_c` is valid NUL-terminated `"/"`.
    let root_raw = unsafe {
        libc::open(
            root_c.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC,
        )
    };
    if root_raw < 0 {
        return Err(AkrepError::from(std::io::Error::last_os_error()));
    }
    // SAFETY: `root_raw >= 0` was returned by `libc::open`.
    let mut cur_fd = unsafe { OwnedFd::from_raw_fd(root_raw) };

    for comp in parent.components() {
        match comp {
            Component::RootDir => continue,
            Component::Normal(seg) => {
                let seg_c = CString::new(seg.as_bytes()).map_err(|_| {
                    AkrepError::InvalidInput("Path component contains NUL byte".to_string())
                })?;
                // SAFETY: `cur_fd` is an open directory FD and `seg_c` is a single normal path segment without slashes.
                let next_raw = unsafe {
                    libc::openat(
                        cur_fd.as_raw_fd(),
                        seg_c.as_ptr(),
                        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    )
                };
                if next_raw < 0 {
                    let os_err = std::io::Error::last_os_error();
                    return match os_err.raw_os_error() {
                        Some(libc::ELOOP) | Some(libc::ENOTDIR) => {
                            Err(AkrepError::ForbiddenOperation(
                                "Symlink or special file rejected by quarantine".to_string(),
                            ))
                        }
                        Some(libc::ENOENT) => Err(AkrepError::InvalidInput(format!(
                            "Parent directory '{}' does not exist",
                            parent.display()
                        ))),
                        _ => Err(AkrepError::from(os_err)),
                    };
                }
                // SAFETY: `next_raw >= 0` was returned by `libc::openat`.
                cur_fd = unsafe { OwnedFd::from_raw_fd(next_raw) };
            }
            _ => {
                return Err(AkrepError::ForbiddenOperation(
                    "Invalid path component in quarantine parent directory".to_string(),
                ));
            }
        }
    }

    if let Ok(real_dir) = fs::read_link(format!("/proc/self/fd/{}", cur_fd.as_raw_fd())) {
        check_forbidden_roots(&real_dir.to_string_lossy(), vault_dir)?;
        if let Ok(canon_vault) = fs::canonicalize(vault_dir) {
            if real_dir == canon_vault || real_dir.starts_with(&canon_vault) {
                return Err(AkrepError::ForbiddenOperation(
                    "Quarantining inside canonical vault directory is forbidden".to_string(),
                ));
            }
        }
    }

    Ok(cur_fd)
}

fn open_regular_file_at_nofollow(
    parent_fd: &OwnedFd,
    leaf_name: &str,
) -> Result<(File, libc::stat), AkrepError> {
    let leaf_c = CString::new(leaf_name)
        .map_err(|_| AkrepError::InvalidInput("Filename contains NUL byte".to_string()))?;
    // SAFETY: `parent_fd` is a valid directory FD and `leaf_c` is a NUL-terminated leaf name.
    let raw_fd = unsafe {
        libc::openat(
            parent_fd.as_raw_fd(),
            leaf_c.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw_fd < 0 {
        let os_err = std::io::Error::last_os_error();
        return match os_err.raw_os_error() {
            Some(libc::ELOOP) => Err(AkrepError::ForbiddenOperation(
                "Symlink or special file rejected by quarantine".to_string(),
            )),
            Some(libc::ENOENT) => Err(AkrepError::InvalidInput(format!(
                "Target file '{}' does not exist",
                leaf_name
            ))),
            _ => Err(AkrepError::from(os_err)),
        };
    }
    // SAFETY: `raw_fd >= 0` was returned by `libc::openat`.
    let file = unsafe { File::from_raw_fd(raw_fd) };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `file` owns a valid open descriptor and `st` is valid writable memory.
    if unsafe { libc::fstat(file.as_raw_fd(), &mut st) } != 0 {
        return Err(AkrepError::from(std::io::Error::last_os_error()));
    }
    if (st.st_mode & libc::S_IFMT) != libc::S_IFREG {
        return Err(AkrepError::ForbiddenOperation(
            "Symlink or special file rejected by quarantine".to_string(),
        ));
    }
    Ok((file, st))
}

fn read_selinux_xattr_fd(fd: i32) -> Option<String> {
    let name_c = CString::new("security.selinux").expect("static xattr name");
    let mut buf = vec![0u8; 512];
    // SAFETY: `fd` is a valid open file descriptor and `buf` has 512 writable bytes.
    let n = unsafe {
        libc::fgetxattr(
            fd,
            name_c.as_ptr(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len(),
        )
    };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    while buf.last() == Some(&0) {
        buf.pop();
    }
    let s = String::from_utf8(buf).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn set_selinux_xattr_fd(fd: i32, ctx: &str) -> Result<(), AkrepError> {
    if ctx.trim().is_empty() || ctx.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(AkrepError::InvalidInput(
            "Invalid SELinux context in quarantine metadata".to_string(),
        ));
    }
    let name_c = CString::new("security.selinux").expect("static xattr name");
    let mut val_bytes = ctx.as_bytes().to_vec();
    val_bytes.push(0);
    // SAFETY: `fd` is an open writable file descriptor and `val_bytes` is a valid byte buffer.
    let rc = unsafe {
        libc::fsetxattr(
            fd,
            name_c.as_ptr(),
            val_bytes.as_ptr() as *const libc::c_void,
            val_bytes.len(),
            0,
        )
    };
    if rc != 0 {
        let os_err = std::io::Error::last_os_error();
        // Gracefully tolerate filesystems without security xattr support (e.g. tmpfs in unit tests)
        if matches!(
            os_err.raw_os_error(),
            Some(libc::EOPNOTSUPP) | Some(libc::ENOSYS) | Some(libc::EPERM) | Some(libc::EACCES)
        ) {
            return Ok(());
        }
        return Err(AkrepError::from(os_err));
    }
    Ok(())
}

fn fchmod_fd(fd: i32, mode: u32) -> Result<(), AkrepError> {
    // Strip S_ISUID, S_ISGID, and S_ISVTX bits unconditionally so quarantine operations can never create setuid/setgid files.
    // SAFETY: `fd` is a valid open file descriptor.
    let rc = unsafe { libc::fchmod(fd, (mode & 0o0777) as libc::mode_t) };
    if rc != 0 {
        return Err(AkrepError::from(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn fsync_fd(fd: i32) -> Result<(), AkrepError> {
    // SAFETY: `fd` is a valid open file descriptor.
    let rc = unsafe { libc::fsync(fd) };
    if rc != 0 {
        return Err(AkrepError::from(std::io::Error::last_os_error()));
    }
    Ok(())
}

fn encode_stream_to_qpkg(
    mut src: File,
    qpkg_path: &Path,
    expected_size: u64,
) -> Result<(String, u64), AkrepError> {
    let key = generate_non_zero_xor_key()?;
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .mode(0o600)
        .open(qpkg_path)?;
    fchmod_fd(out.as_raw_fd(), 0o600)?;

    let mut header = [0u8; QPKG_HEADER_LEN];
    header[..8].copy_from_slice(QPKG_MAGIC);
    header[8..8 + QPKG_KEY_LEN].copy_from_slice(&key);
    header[8 + QPKG_KEY_LEN..QPKG_HEADER_LEN].copy_from_slice(&expected_size.to_le_bytes());
    out.write_all(&header)?;

    let mut hasher = Sha256Hasher::new();
    let mut buf = vec![0u8; STREAM_BUF_LEN];
    let mut offset = 0usize;
    let mut written = 0u64;

    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        for i in 0..n {
            buf[i] ^= key[(offset + i) & (QPKG_KEY_LEN - 1)];
        }
        out.write_all(&buf[..n])?;
        offset = offset.wrapping_add(n);
        written = written.saturating_add(n as u64);
    }

    out.sync_all()?;
    Ok((hasher.finalize_hex(), written))
}

pub fn decode_qpkg_and_hash(
    qpkg_path: &Path,
    mut sink: Option<&mut File>,
) -> Result<(String, u64), AkrepError> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(qpkg_path)?;

    let mut header = [0u8; QPKG_HEADER_LEN];
    f.read_exact(&mut header).map_err(|e| {
        AkrepError::VerifyFailed(format!(
            "Corrupt or truncated quarantine package header: {}",
            e
        ))
    })?;

    if &header[..8] != QPKG_MAGIC {
        return Err(AkrepError::VerifyFailed(
            "Invalid quarantine package magic (expected AKREPQ1\\0)".to_string(),
        ));
    }

    let mut key = [0u8; QPKG_KEY_LEN];
    key.copy_from_slice(&header[8..8 + QPKG_KEY_LEN]);
    let mut size_bytes = [0u8; 8];
    size_bytes.copy_from_slice(&header[8 + QPKG_KEY_LEN..QPKG_HEADER_LEN]);
    let expected_len = u64::from_le_bytes(size_bytes);

    let mut hasher = Sha256Hasher::new();
    let mut buf = vec![0u8; STREAM_BUF_LEN];
    let mut offset = 0usize;
    let mut total = 0u64;

    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for i in 0..n {
            buf[i] ^= key[(offset + i) & (QPKG_KEY_LEN - 1)];
        }
        hasher.update(&buf[..n]);
        if let Some(w) = sink.as_deref_mut() {
            w.write_all(&buf[..n])?;
        }
        offset = offset.wrapping_add(n);
        total = total.saturating_add(n as u64);
    }

    if total != expected_len {
        return Err(AkrepError::VerifyFailed(format!(
            "Quarantine package payload size mismatch (expected {}, decoded {})",
            expected_len, total
        )));
    }

    Ok((hasher.finalize_hex(), total))
}

fn hash_open_file(mut file: &File) -> Result<String, AkrepError> {
    let mut hasher = Sha256Hasher::new();
    let mut buf = vec![0u8; STREAM_BUF_LEN];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize_hex())
}

fn atomic_place_noreplace(
    parent_fd: &OwnedFd,
    tmp_leaf: &str,
    dest_leaf: &str,
) -> Result<(), AkrepError> {
    let tmp_c = CString::new(tmp_leaf)
        .map_err(|_| AkrepError::InvalidInput("Invalid temporary leaf".to_string()))?;
    let dst_c = CString::new(dest_leaf)
        .map_err(|_| AkrepError::InvalidInput("Invalid destination leaf".to_string()))?;

    // Prefer renameat2(..., RENAME_NOREPLACE) so existing files are NEVER overwritten atomically.
    // SAFETY: `parent_fd` is a valid directory FD and both names are NUL-terminated leaf strings.
    let rc = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            parent_fd.as_raw_fd(),
            tmp_c.as_ptr(),
            parent_fd.as_raw_fd(),
            dst_c.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        return Ok(());
    }

    let os_err = std::io::Error::last_os_error();
    match os_err.raw_os_error() {
        Some(libc::EEXIST) => Err(AkrepError::TransactionConflict(format!(
            "Refusing to overwrite existing file '{}'",
            dest_leaf
        ))),
        Some(libc::EINVAL) | Some(libc::ENOSYS) => {
            // Fallback to linkat (without AT_SYMLINK_FOLLOW) + unlinkat, which also refuses EEXIST.
            // SAFETY: `parent_fd` is valid and `tmp_c`/`dst_c` are valid CStrings.
            let lrc = unsafe {
                libc::linkat(
                    parent_fd.as_raw_fd(),
                    tmp_c.as_ptr(),
                    parent_fd.as_raw_fd(),
                    dst_c.as_ptr(),
                    0,
                )
            };
            if lrc != 0 {
                let lerr = std::io::Error::last_os_error();
                if lerr.raw_os_error() == Some(libc::EEXIST) {
                    return Err(AkrepError::TransactionConflict(format!(
                        "Refusing to overwrite existing file '{}'",
                        dest_leaf
                    )));
                }
                return Err(AkrepError::from(lerr));
            }
            // SAFETY: `parent_fd` and `tmp_c` are valid.
            unsafe {
                let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
            }
            Ok(())
        }
        _ => Err(AkrepError::from(os_err)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QuarantineTxSnapshot {
    op: String,
    quarantine_id: String,
    original_path: String,
    threat_name: Option<String>,
}

#[derive(Clone)]
pub struct QuarantineAdapter {
    state_dir: PathBuf,
    vault_dir: PathBuf,
}

impl QuarantineAdapter {
    pub fn new(state_dir: PathBuf) -> Result<Self, AkrepError> {
        let vault_dir = state_dir.join("quarantine");
        let _ = ensure_vault_dir_0700(&vault_dir)?;
        Ok(Self {
            state_dir,
            vault_dir,
        })
    }

    pub fn new_lazy(state_dir: PathBuf) -> Self {
        let vault_dir = state_dir.join("quarantine");
        Self {
            state_dir,
            vault_dir,
        }
    }

    pub fn vault_dir(&self) -> &Path {
        &self.vault_dir
    }

    fn meta_path(&self, quarantine_id: &str) -> PathBuf {
        self.vault_dir.join(format!("{}.json", quarantine_id))
    }

    pub fn qpkg_path(&self, quarantine_id: &str) -> PathBuf {
        self.vault_dir.join(format!("{}.akrep-qpkg", quarantine_id))
    }

    pub fn load_metadata(&self, quarantine_id: &str) -> Result<QuarantineMetadata, AkrepError> {
        let p = self.meta_path(quarantine_id);
        if !p.is_file() {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown quarantine_id '{}'",
                quarantine_id
            )));
        }
        let raw = fs::read_to_string(&p)?;
        serde_json::from_str(&raw).map_err(|e| {
            AkrepError::Internal(format!(
                "Corrupt quarantine manifest '{}': {}",
                quarantine_id,
                redact_secrets(&e.to_string())
            ))
        })
    }

    pub fn list_inventory(&self) -> Result<QuarantineInventory, AkrepError> {
        let _ = ensure_vault_dir_0700(&self.vault_dir)?;
        let mut entries = Vec::new();
        for item in fs::read_dir(&self.vault_dir)? {
            let Ok(item) = item else {
                continue;
            };
            let p = item.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&p) {
                if let Ok(meta) = serde_json::from_str::<QuarantineMetadata>(&raw) {
                    entries.push(meta);
                }
            }
        }
        entries.sort_by(|a, b| b.quarantined_at.cmp(&a.quarantined_at));
        Ok(QuarantineInventory {
            vault_path: self.vault_dir.to_string_lossy().to_string(),
            entries,
        })
    }

    fn resolve_actor_uid_from_snapshot_dir(&self, snapshot_dir: &Path) -> u32 {
        if let Some(tx_id) = snapshot_dir.file_name().and_then(|n| n.to_str()) {
            let tx_file = self
                .state_dir
                .join("transactions")
                .join(format!("{}.json", tx_id));
            if let Ok(raw) = fs::read_to_string(tx_file) {
                if let Ok(rec) = serde_json::from_str::<TransactionRecord>(&raw) {
                    return rec.actor_uid;
                }
            }
        }
        // SAFETY: `geteuid` is thread-safe and cannot fail.
        unsafe { libc::geteuid() }
    }

    fn compute_source_path_fingerprint(
        &self,
        path_str: &str,
    ) -> Result<StateFingerprint, AkrepError> {
        let target_label = format!("quarantine.path.{}", path_str);
        let validated = validate_quarantine_target_path(path_str, &self.vault_dir)?;
        let Some(parent) = validated.parent() else {
            return Err(AkrepError::InvalidInput(
                "Missing parent directory".to_string(),
            ));
        };
        let Some(leaf) = validated.file_name().and_then(|n| n.to_str()) else {
            return Err(AkrepError::InvalidInput(
                "Invalid target filename".to_string(),
            ));
        };

        let parent_fd = match open_parent_dir_nofollow(parent, &self.vault_dir) {
            Ok(fd) => fd,
            Err(AkrepError::InvalidInput(_)) => {
                return Ok(StateFingerprint {
                    target: target_label,
                    sha256: sha256_hex(b"present=false"),
                    captured_at: Utc::now().to_rfc3339(),
                });
            }
            Err(other) => return Err(other),
        };

        match open_regular_file_at_nofollow(&parent_fd, leaf) {
            Ok((file, st)) => {
                let digest = hash_open_file(&file)?;
                let state_str = format!(
                    "present=true|dev={}|ino={}|size={}|mode={:o}|sha256={}",
                    st.st_dev,
                    st.st_ino,
                    st.st_size,
                    st.st_mode & 0o7777,
                    digest
                );
                Ok(StateFingerprint {
                    target: target_label,
                    sha256: sha256_hex(state_str.as_bytes()),
                    captured_at: Utc::now().to_rfc3339(),
                })
            }
            Err(AkrepError::InvalidInput(_)) => Ok(StateFingerprint {
                target: target_label,
                sha256: sha256_hex(b"present=false"),
                captured_at: Utc::now().to_rfc3339(),
            }),
            Err(other) => Err(other),
        }
    }

    fn compute_entry_fingerprint(
        &self,
        quarantine_id: &str,
    ) -> Result<StateFingerprint, AkrepError> {
        let target_label = format!("quarantine.entry.{}", quarantine_id);
        let meta_p = self.meta_path(quarantine_id);
        let qpkg_p = self.qpkg_path(quarantine_id);
        if !meta_p.exists() && !qpkg_p.exists() {
            return Ok(StateFingerprint {
                target: target_label,
                sha256: sha256_hex(b"entry=deleted"),
                captured_at: Utc::now().to_rfc3339(),
            });
        }
        let meta = self.load_metadata(quarantine_id)?;
        let qpkg_present = qpkg_p.is_file();
        let qpkg_len = fs::symlink_metadata(&qpkg_p).map(|m| m.len()).unwrap_or(0);
        let state_str = format!(
            "entry=present|qpkg={}|qpkg_len={}|sha256={}|restored={}",
            qpkg_present, qpkg_len, meta.sha256, meta.restored
        );
        Ok(StateFingerprint {
            target: target_label,
            sha256: sha256_hex(state_str.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        })
    }

    fn quarantine_file_sync(
        &self,
        quarantine_id: &str,
        path_str: &str,
        threat_name: &str,
        actor_uid: u32,
    ) -> Result<QuarantineMetadata, AkrepError> {
        let vault_fd = ensure_vault_dir_0700(&self.vault_dir)?;
        let validated = validate_quarantine_target_path(path_str, &self.vault_dir)?;
        let parent = validated
            .parent()
            .ok_or_else(|| AkrepError::InvalidInput("Missing parent directory".to_string()))?;
        let leaf = validated
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AkrepError::InvalidInput("Invalid filename".to_string()))?;

        let parent_fd = open_parent_dir_nofollow(parent, &self.vault_dir)?;
        let (src_file, st_initial) = open_regular_file_at_nofollow(&parent_fd, leaf)?;
        let src_raw_fd = src_file.as_raw_fd();
        let selinux_context = read_selinux_xattr_fd(src_raw_fd);

        let qpkg_p = self.qpkg_path(quarantine_id);
        let meta_p = self.meta_path(quarantine_id);

        // Keep an owned duplicate descriptor to check fstat after streaming if needed
        // SAFETY: `src_raw_fd` is a valid open file descriptor.
        let dup_raw = unsafe { libc::fcntl(src_raw_fd, libc::F_DUPFD_CLOEXEC, 0) };
        if dup_raw < 0 {
            return Err(AkrepError::from(std::io::Error::last_os_error()));
        }
        // SAFETY: `dup_raw >= 0` was returned by `fcntl(F_DUPFD_CLOEXEC)`.
        let dup_fd = unsafe { OwnedFd::from_raw_fd(dup_raw) };

        let (orig_sha256, written_bytes) =
            match encode_stream_to_qpkg(src_file, &qpkg_p, st_initial.st_size as u64) {
                Ok(v) => v,
                Err(e) => {
                    let _ = fs::remove_file(&qpkg_p);
                    return Err(e);
                }
            };

        if written_bytes != st_initial.st_size as u64 {
            let _ = fs::remove_file(&qpkg_p);
            return Err(AkrepError::ExternalDriftDetected(
                "Source file size changed while capturing into quarantine".to_string(),
            ));
        }

        // Re-read and decode `.akrep-qpkg` BEFORE unlinking original file to guarantee zero data loss.
        let (decoded_sha256, decoded_len) = match decode_qpkg_and_hash(&qpkg_p, None) {
            Ok(v) => v,
            Err(e) => {
                let _ = fs::remove_file(&qpkg_p);
                return Err(e);
            }
        };

        if decoded_sha256 != orig_sha256 || decoded_len != written_bytes {
            let _ = fs::remove_file(&qpkg_p);
            return Err(AkrepError::ApplyFailed(
                "Quarantine envelope decode SHA-256 verification failed; original file left intact"
                    .to_string(),
            ));
        }

        let meta = QuarantineMetadata {
            quarantine_id: quarantine_id.to_string(),
            original_path: path_str.to_string(),
            original_mode: st_initial.st_mode & 0o7777,
            original_uid: st_initial.st_uid,
            original_gid: st_initial.st_gid,
            selinux_context,
            sha256: orig_sha256,
            size_bytes: written_bytes,
            threat_name: threat_name.to_string(),
            quarantined_at: Utc::now().to_rfc3339(),
            actor_uid,
            restored: false,
            restored_at: None,
        };

        if let Err(e) = write_json_atomic_0600(&meta_p, &meta) {
            let _ = fs::remove_file(&qpkg_p);
            return Err(e);
        }
        fsync_fd(vault_fd.as_raw_fd())?;

        // Verify both the open FD stat and the directory entry `(st_dev, st_ino, mtime, size)` BEFORE unlinkat!
        let mut st_fd_after: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `dup_fd` is valid open FD.
        if unsafe { libc::fstat(dup_fd.as_raw_fd(), &mut st_fd_after) } != 0 {
            let _ = fs::remove_file(&qpkg_p);
            let _ = fs::remove_file(&meta_p);
            return Err(AkrepError::from(std::io::Error::last_os_error()));
        }

        let leaf_c = CString::new(leaf).expect("validated leaf cstr");
        let mut st_dir_now: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `parent_fd` and `leaf_c` are valid; `AT_SYMLINK_NOFOLLOW` prevents following any swapped symlink.
        let fstatat_rc = unsafe {
            libc::fstatat(
                parent_fd.as_raw_fd(),
                leaf_c.as_ptr(),
                &mut st_dir_now,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if fstatat_rc != 0 {
            let os_err = std::io::Error::last_os_error();
            let _ = fs::remove_file(&qpkg_p);
            let _ = fs::remove_file(&meta_p);
            return Err(AkrepError::from(os_err));
        }

        let same_inode_and_state = (st_dir_now.st_mode & libc::S_IFMT) == libc::S_IFREG
            && st_dir_now.st_dev == st_initial.st_dev
            && st_dir_now.st_ino == st_initial.st_ino
            && st_dir_now.st_size == st_initial.st_size
            && st_dir_now.st_mtime == st_initial.st_mtime
            && st_dir_now.st_mtime_nsec == st_initial.st_mtime_nsec
            && st_fd_after.st_size == st_initial.st_size
            && st_fd_after.st_mtime == st_initial.st_mtime
            && st_fd_after.st_mtime_nsec == st_initial.st_mtime_nsec;

        if !same_inode_and_state {
            let _ = fs::remove_file(&qpkg_p);
            let _ = fs::remove_file(&meta_p);
            return Err(AkrepError::ExternalDriftDetected(
                "Target file inode or content changed before unlinkat; aborting quarantine"
                    .to_string(),
            ));
        }

        drop(dup_fd);

        // SAFETY: `parent_fd` and `leaf_c` verified to refer to the exact captured regular file inode.
        let unlink_rc = unsafe { libc::unlinkat(parent_fd.as_raw_fd(), leaf_c.as_ptr(), 0) };
        if unlink_rc != 0 {
            let os_err = std::io::Error::last_os_error();
            let _ = fs::remove_file(&qpkg_p);
            let _ = fs::remove_file(&meta_p);
            return Err(AkrepError::from(os_err));
        }
        fsync_fd(parent_fd.as_raw_fd())?;

        Ok(meta)
    }

    fn restore_quarantine_sync(
        &self,
        quarantine_id: &str,
        remove_vault_entry_after: bool,
    ) -> Result<QuarantineMetadata, AkrepError> {
        let vault_fd = ensure_vault_dir_0700(&self.vault_dir)?;
        let mut meta = self.load_metadata(quarantine_id)?;
        let qpkg_p = self.qpkg_path(quarantine_id);
        if !qpkg_p.is_file() {
            return Err(AkrepError::ApplyFailed(format!(
                "Quarantine package payload missing for '{}'",
                quarantine_id
            )));
        }

        let validated_dest = validate_quarantine_target_path(&meta.original_path, &self.vault_dir)?;
        let parent = validated_dest
            .parent()
            .ok_or_else(|| AkrepError::InvalidInput("Missing restore parent".to_string()))?;
        let leaf = validated_dest
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AkrepError::InvalidInput("Invalid restore filename".to_string()))?;

        let parent_fd = open_parent_dir_nofollow(parent, &self.vault_dir)?;
        let leaf_c = CString::new(leaf).expect("validated leaf");
        let mut existing_st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `parent_fd` and `leaf_c` are valid.
        let exists_rc = unsafe {
            libc::fstatat(
                parent_fd.as_raw_fd(),
                leaf_c.as_ptr(),
                &mut existing_st,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if exists_rc == 0 {
            return Err(AkrepError::TransactionConflict(format!(
                "Refusing to overwrite existing path '{}' during quarantine restore",
                meta.original_path
            )));
        }

        let nonce = generate_uuid_v4()?;
        let tmp_leaf = format!(".akrep-restore-{}.tmp", nonce);
        let tmp_c = CString::new(tmp_leaf.as_str()).expect("valid tmp leaf");

        // SAFETY: `parent_fd` is a verified non-symlink directory FD; `O_CREAT | O_EXCL | O_NOFOLLOW` creates a fresh 0600 file.
        let tmp_raw = unsafe {
            libc::openat(
                parent_fd.as_raw_fd(),
                tmp_c.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if tmp_raw < 0 {
            return Err(AkrepError::from(std::io::Error::last_os_error()));
        }
        // SAFETY: `tmp_raw >= 0` was just returned by `libc::openat`.
        let mut tmp_file = unsafe { File::from_raw_fd(tmp_raw) };

        let decode_res = decode_qpkg_and_hash(&qpkg_p, Some(&mut tmp_file));
        let (decoded_sha, decoded_size) = match decode_res {
            Ok(v) => v,
            Err(e) => {
                drop(tmp_file);
                unsafe {
                    let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
                }
                return Err(e);
            }
        };

        if decoded_sha != meta.sha256 || decoded_size != meta.size_bytes {
            drop(tmp_file);
            unsafe {
                let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
            }
            return Err(AkrepError::VerifyFailed(format!(
                "Decoded SHA-256 mismatch during restore of '{}' (expected {}, got {})",
                quarantine_id, meta.sha256, decoded_sha
            )));
        }

        // Restore ownership (if root or matching UID/GID), mode, and SELinux context on the FD BEFORE linking into place!
        // SAFETY: `tmp_file` owns a valid open FD.
        let chown_rc =
            unsafe { libc::fchown(tmp_file.as_raw_fd(), meta.original_uid, meta.original_gid) };
        if chown_rc != 0 {
            let cerr = std::io::Error::last_os_error();
            // SAFETY: `geteuid` is always safe.
            let is_root = unsafe { libc::geteuid() == 0 };
            if is_root || cerr.raw_os_error() != Some(libc::EPERM) {
                drop(tmp_file);
                unsafe {
                    let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
                }
                return Err(AkrepError::from(cerr));
            }
        }

        if let Err(e) = fchmod_fd(tmp_file.as_raw_fd(), meta.original_mode & 0o0777) {
            drop(tmp_file);
            unsafe {
                let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
            }
            return Err(e);
        }

        if let Some(ctx) = &meta.selinux_context {
            if let Err(e) = set_selinux_xattr_fd(tmp_file.as_raw_fd(), ctx) {
                drop(tmp_file);
                unsafe {
                    let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
                }
                return Err(e);
            }
        }

        if let Err(e) = tmp_file.sync_all() {
            drop(tmp_file);
            unsafe {
                let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
            }
            return Err(AkrepError::from(e));
        }
        drop(tmp_file);

        // Re-open tmp_leaf via parent_fd and verify on-disk SHA-256 matches manifest BEFORE placing at destination!
        let (verify_tmp, _) = open_regular_file_at_nofollow(&parent_fd, &tmp_leaf)?;
        let disk_sha = hash_open_file(&verify_tmp)?;
        drop(verify_tmp);
        if disk_sha != meta.sha256 {
            unsafe {
                let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
            }
            return Err(AkrepError::VerifyFailed(
                "On-disk restored temporary file SHA-256 mismatch".to_string(),
            ));
        }

        if let Err(place_err) = atomic_place_noreplace(&parent_fd, &tmp_leaf, leaf) {
            unsafe {
                let _ = libc::unlinkat(parent_fd.as_raw_fd(), tmp_c.as_ptr(), 0);
            }
            return Err(place_err);
        }
        fsync_fd(parent_fd.as_raw_fd())?;

        if remove_vault_entry_after {
            let _ = fs::remove_file(&qpkg_p);
            let _ = fs::remove_file(self.meta_path(quarantine_id));
        } else {
            meta.restored = true;
            meta.restored_at = Some(Utc::now().to_rfc3339());
            write_json_atomic_0600(&self.meta_path(quarantine_id), &meta)?;
        }
        fsync_fd(vault_fd.as_raw_fd())?;

        Ok(meta)
    }

    fn preview_sync(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        req.validate_red_lines()?;
        let _ = ensure_vault_dir_0700(&self.vault_dir)?;

        match req {
            ActionRequest::QuarantineFile { path, threat_name } => {
                let validated = validate_quarantine_target_path(path, &self.vault_dir)?;
                let parent = validated.parent().ok_or_else(|| {
                    AkrepError::InvalidInput("Missing parent directory".to_string())
                })?;
                let leaf = validated
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| AkrepError::InvalidInput("Invalid filename".to_string()))?;
                let parent_fd = open_parent_dir_nofollow(parent, &self.vault_dir)?;
                let (_file, st) = open_regular_file_at_nofollow(&parent_fd, leaf)?;
                let fp = self.compute_source_path_fingerprint(path)?;

                Ok(PreviewOutcome {
                    risk: RiskLevel::Medium,
                    diffs: vec![PlanDiffItem {
                        target: path.clone(),
                        current_value: format!("present (mode {:04o}, {} bytes)", st.st_mode & 0o7777, st.st_size),
                        desired_value: format!(
                            "quarantined (.akrep-qpkg non-executable 0600, threat={})",
                            threat_name
                        ),
                        description_tr: format!(
                            "'{}' dosyasi SHA-256 dogrulamali calistirilamaz karantina zarfina tasinacak (otomatik silme yapilmaz).",
                            path
                        ),
                        description_en: format!(
                            "Move '{}' into a non-executable SHA-256 verified quarantine envelope without permanent deletion.",
                            path
                        ),
                    }],
                    pre_fingerprints: vec![fp],
                    warnings_tr: Vec::new(),
                    warnings_en: Vec::new(),
                })
            }
            ActionRequest::RestoreQuarantine { quarantine_id } => {
                let meta = self.load_metadata(quarantine_id)?;
                if meta.restored {
                    return Err(AkrepError::InvalidInput(format!(
                        "Quarantine entry '{}' is already marked restored",
                        quarantine_id
                    )));
                }
                let entry_fp = self.compute_entry_fingerprint(quarantine_id)?;
                let dest_fp = self.compute_source_path_fingerprint(&meta.original_path)?;
                if dest_fp.sha256 != sha256_hex(b"present=false") {
                    return Err(AkrepError::TransactionConflict(format!(
                        "Restore destination '{}' already exists; refusing overwrite",
                        meta.original_path
                    )));
                }

                Ok(PreviewOutcome {
                    risk: RiskLevel::Medium,
                    diffs: vec![PlanDiffItem {
                        target: meta.original_path.clone(),
                        current_value: format!("quarantined ({})", quarantine_id),
                        desired_value: format!(
                            "restored (mode {:04o}, sha256={})",
                            meta.original_mode & 0o0777,
                            &meta.sha256[..12]
                        ),
                        description_tr: format!(
                            "Karantinadaki '{}' dosyasi SHA-256, izin ve SELinux baglami dogrulanarak '{}' konumuna geri yuklenecek.",
                            quarantine_id, meta.original_path
                        ),
                        description_en: format!(
                            "Restore quarantined entry '{}' to '{}' after SHA-256, mode, and SELinux context verification.",
                            quarantine_id, meta.original_path
                        ),
                    }],
                    pre_fingerprints: vec![entry_fp, dest_fp],
                    warnings_tr: vec![format!(
                        "Bu dosya daha once '{}' tehdidi olarak isaretlenmisti; yalnizca yanlis pozitif oldugundan eminseniz geri yukleyin.",
                        meta.threat_name
                    )],
                    warnings_en: vec![format!(
                        "This file was previously flagged as '{}'; restore only if you have verified it is a false positive.",
                        meta.threat_name
                    )],
                })
            }
            ActionRequest::DeleteQuarantine {
                quarantine_id,
                confirm_permanent_delete,
            } => {
                if !confirm_permanent_delete {
                    return Err(AkrepError::ForbiddenOperation(
                        "Permanent quarantine deletion requires confirm_permanent_delete=true"
                            .to_string(),
                    ));
                }
                let meta = self.load_metadata(quarantine_id)?;
                let entry_fp = self.compute_entry_fingerprint(quarantine_id)?;
                Ok(PreviewOutcome {
                    risk: RiskLevel::High,
                    diffs: vec![PlanDiffItem {
                        target: format!("quarantine.entry.{}", quarantine_id),
                        current_value: format!("stored ({} bytes)", meta.size_bytes),
                        desired_value: "permanently_deleted".to_string(),
                        description_tr: format!(
                            "'{}' karantina paketi ('{}') kalici olarak silinecek.",
                            quarantine_id, meta.original_path
                        ),
                        description_en: format!(
                            "Permanently delete quarantine package '{}' (originally '{}').",
                            quarantine_id, meta.original_path
                        ),
                    }],
                    pre_fingerprints: vec![entry_fp],
                    warnings_tr: vec!["Kalici karantina silme islemi geri alinamaz.".to_string()],
                    warnings_en: vec![
                        "Permanent quarantine deletion is irreversible and cannot be rolled back."
                            .to_string(),
                    ],
                })
            }
            _ => Err(AkrepError::InvalidInput(
                "QuarantineAdapter received non-quarantine ActionRequest".to_string(),
            )),
        }
    }

    fn apply_sync(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        req.validate_red_lines()?;
        let actor_uid = self.resolve_actor_uid_from_snapshot_dir(snapshot_dir);

        match req {
            ActionRequest::QuarantineFile { path, threat_name } => {
                let raw_uuid = generate_uuid_v4()?.replace('-', "");
                let quarantine_id = format!("q_{}", &raw_uuid[..16]);
                let meta =
                    self.quarantine_file_sync(&quarantine_id, path, threat_name, actor_uid)?;

                let snap = QuarantineTxSnapshot {
                    op: "quarantine".to_string(),
                    quarantine_id: quarantine_id.clone(),
                    original_path: path.clone(),
                    threat_name: Some(threat_name.clone()),
                };
                write_json_atomic_0600(&snapshot_dir.join("quarantine_op.json"), &snap)?;

                Ok(ApplyOutcome {
                    pre_snapshot: serde_json::to_value(&snap)
                        .map_err(|e| AkrepError::Internal(e.to_string()))?,
                    target_state: serde_json::json!({
                        "quarantine_id": quarantine_id,
                        "original_path": path,
                        "sha256": meta.sha256,
                    }),
                    summary: format!(
                        "Quarantined '{}' as '{}' (sha256={})",
                        path, quarantine_id, meta.sha256
                    ),
                })
            }
            ActionRequest::RestoreQuarantine { quarantine_id } => {
                let prev_meta = self.load_metadata(quarantine_id)?;
                if prev_meta.restored {
                    return Err(AkrepError::InvalidInput(format!(
                        "Quarantine entry '{}' is already restored",
                        quarantine_id
                    )));
                }
                let restored_meta = self.restore_quarantine_sync(quarantine_id, false)?;
                let snap = QuarantineTxSnapshot {
                    op: "restore".to_string(),
                    quarantine_id: quarantine_id.clone(),
                    original_path: restored_meta.original_path.clone(),
                    threat_name: Some(restored_meta.threat_name.clone()),
                };
                write_json_atomic_0600(&snapshot_dir.join("quarantine_op.json"), &snap)?;

                Ok(ApplyOutcome {
                    pre_snapshot: serde_json::to_value(&snap)
                        .map_err(|e| AkrepError::Internal(e.to_string()))?,
                    target_state: serde_json::json!({
                        "quarantine_id": quarantine_id,
                        "original_path": restored_meta.original_path,
                        "sha256": restored_meta.sha256,
                        "restored": true,
                    }),
                    summary: format!(
                        "Restored '{}' to '{}' (verified sha256={})",
                        quarantine_id, restored_meta.original_path, restored_meta.sha256
                    ),
                })
            }
            ActionRequest::DeleteQuarantine {
                quarantine_id,
                confirm_permanent_delete,
            } => {
                if !confirm_permanent_delete {
                    return Err(AkrepError::ForbiddenOperation(
                        "Permanent quarantine deletion requires confirm_permanent_delete=true"
                            .to_string(),
                    ));
                }
                let vault_fd = ensure_vault_dir_0700(&self.vault_dir)?;
                let meta = self.load_metadata(quarantine_id)?;
                let qpkg_p = self.qpkg_path(quarantine_id);
                let meta_p = self.meta_path(quarantine_id);

                if qpkg_p.exists() {
                    fs::remove_file(&qpkg_p)?;
                }
                if meta_p.exists() {
                    fs::remove_file(&meta_p)?;
                }
                fsync_fd(vault_fd.as_raw_fd())?;

                let snap = QuarantineTxSnapshot {
                    op: "delete".to_string(),
                    quarantine_id: quarantine_id.clone(),
                    original_path: meta.original_path.clone(),
                    threat_name: Some(meta.threat_name),
                };
                write_json_atomic_0600(&snapshot_dir.join("quarantine_op.json"), &snap)?;

                Ok(ApplyOutcome {
                    pre_snapshot: serde_json::to_value(&snap)
                        .map_err(|e| AkrepError::Internal(e.to_string()))?,
                    target_state: serde_json::json!({
                        "quarantine_id": quarantine_id,
                        "deleted": true,
                    }),
                    summary: format!("Permanently deleted quarantine entry '{}'", quarantine_id),
                })
            }
            _ => Err(AkrepError::InvalidInput(
                "QuarantineAdapter received non-quarantine ActionRequest".to_string(),
            )),
        }
    }

    fn verify_sync(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        match req {
            ActionRequest::QuarantineFile { path, .. } => {
                let qid = expected
                    .target_state
                    .get("quarantine_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        AkrepError::VerifyFailed(
                            "Missing quarantine_id in target_state".to_string(),
                        )
                    })?;
                let meta = self.load_metadata(qid)?;
                let qpkg_p = self.qpkg_path(qid);
                let meta_p = self.meta_path(qid);

                let qpkg_mode = fs::symlink_metadata(&qpkg_p)?.permissions().mode() & 0o777;
                let meta_mode = fs::symlink_metadata(&meta_p)?.permissions().mode() & 0o777;
                if qpkg_mode != 0o600 || meta_mode != 0o600 {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Quarantine files must be 0600 (got qpkg={:04o}, meta={:04o})",
                        qpkg_mode, meta_mode
                    )));
                }

                let (decoded_sha, _) = decode_qpkg_and_hash(&qpkg_p, None)?;
                if decoded_sha != meta.sha256 {
                    return Err(AkrepError::VerifyFailed(
                        "Quarantine package post-apply SHA-256 verification failed".to_string(),
                    ));
                }

                let src_fp = self.compute_source_path_fingerprint(path)?;
                if src_fp.sha256 != sha256_hex(b"present=false") {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Source file '{}' still present after quarantine",
                        path
                    )));
                }
                let entry_fp = self.compute_entry_fingerprint(qid)?;
                Ok(vec![entry_fp, src_fp])
            }
            ActionRequest::RestoreQuarantine { quarantine_id } => {
                let meta = self.load_metadata(quarantine_id)?;
                if !meta.restored {
                    return Err(AkrepError::VerifyFailed(
                        "Quarantine manifest not marked restored".to_string(),
                    ));
                }
                let validated =
                    validate_quarantine_target_path(&meta.original_path, &self.vault_dir)?;
                let parent = validated.parent().ok_or_else(|| {
                    AkrepError::VerifyFailed("Missing parent directory".to_string())
                })?;
                let leaf = validated
                    .file_name()
                    .and_then(|n| n.to_str())
                    .ok_or_else(|| AkrepError::VerifyFailed("Invalid leaf".to_string()))?;
                let parent_fd = open_parent_dir_nofollow(parent, &self.vault_dir)?;
                let (restored_file, restored_st) = open_regular_file_at_nofollow(&parent_fd, leaf)?;
                if (restored_st.st_mode & 0o6000) != 0 {
                    return Err(AkrepError::VerifyFailed(
                        "Restored file unexpectedly retained setuid/setgid mode bits".to_string(),
                    ));
                }
                let actual_sha = hash_open_file(&restored_file)?;
                if actual_sha != meta.sha256 {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Restored file SHA-256 mismatch (expected {}, got {})",
                        meta.sha256, actual_sha
                    )));
                }
                let entry_fp = self.compute_entry_fingerprint(quarantine_id)?;
                let dest_fp = self.compute_source_path_fingerprint(&meta.original_path)?;
                Ok(vec![entry_fp, dest_fp])
            }
            ActionRequest::DeleteQuarantine { quarantine_id, .. } => {
                if self.meta_path(quarantine_id).exists() || self.qpkg_path(quarantine_id).exists()
                {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Quarantine entry '{}' still exists after permanent delete",
                        quarantine_id
                    )));
                }
                Ok(vec![self.compute_entry_fingerprint(quarantine_id)?])
            }
            _ => Err(AkrepError::InvalidInput(
                "QuarantineAdapter received non-quarantine ActionRequest".to_string(),
            )),
        }
    }

    fn current_fingerprints_sync(
        &self,
        _req: &ActionRequest,
        reference_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let mut out = Vec::with_capacity(reference_fps.len());
        for fp in reference_fps {
            if let Some(path_str) = fp.target.strip_prefix("quarantine.path.") {
                out.push(self.compute_source_path_fingerprint(path_str)?);
            } else if let Some(qid) = fp.target.strip_prefix("quarantine.entry.") {
                out.push(self.compute_entry_fingerprint(qid)?);
            } else {
                return Err(AkrepError::Internal(format!(
                    "Unknown quarantine fingerprint target '{}'",
                    fp.target
                )));
            }
        }
        Ok(out)
    }

    fn rollback_sync(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join("quarantine_op.json");
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok(
                "Quarantine mutation aborted prior to commit; filesystem state already intact"
                    .to_string(),
            );
        }
        let snap: QuarantineTxSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };

        match snap.op.as_str() {
            "quarantine" => {
                if self.qpkg_path(&snap.quarantine_id).is_file() {
                    let _ = self
                        .restore_quarantine_sync(&snap.quarantine_id, true)
                        .map_err(|e| AkrepError::RollbackFailed(e.message().to_string()))?;
                }
                Ok(format!(
                    "Rolled back quarantine '{}' and restored '{}'",
                    snap.quarantine_id, snap.original_path
                ))
            }
            "restore" => {
                let mut meta = self.load_metadata(&snap.quarantine_id)?;
                let validated =
                    validate_quarantine_target_path(&snap.original_path, &self.vault_dir)?;
                if let (Some(parent), Some(leaf)) = (
                    validated.parent(),
                    validated.file_name().and_then(|n| n.to_str()),
                ) {
                    let parent_fd = open_parent_dir_nofollow(parent, &self.vault_dir)?;
                    if let Ok((f, _)) = open_regular_file_at_nofollow(&parent_fd, leaf) {
                        let sha = hash_open_file(&f)?;
                        drop(f);
                        if sha != meta.sha256 {
                            return Err(AkrepError::RollbackFailed(
                                "Restored file content drifted; refusing to unlink on rollback"
                                    .to_string(),
                            ));
                        }
                        let leaf_c = CString::new(leaf).expect("leaf cstr");
                        // SAFETY: `parent_fd` and `leaf_c` are valid.
                        if unsafe { libc::unlinkat(parent_fd.as_raw_fd(), leaf_c.as_ptr(), 0) } != 0
                        {
                            return Err(AkrepError::from(std::io::Error::last_os_error()));
                        }
                        fsync_fd(parent_fd.as_raw_fd())?;
                    }
                }
                meta.restored = false;
                meta.restored_at = None;
                write_json_atomic_0600(&self.meta_path(&snap.quarantine_id), &meta)?;
                Ok(format!(
                    "Rolled back restore of '{}' back into quarantine vault",
                    snap.quarantine_id
                ))
            }
            "delete" => Err(AkrepError::RollbackFailed(
                "Permanent quarantine deletion is irreversible and cannot be rolled back"
                    .to_string(),
            )),
            other => Err(AkrepError::RollbackFailed(format!(
                "Unknown quarantine rollback op '{}'",
                other
            ))),
        }
    }
}

#[async_trait]
impl MutationBackend for QuarantineAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        let this = self.clone();
        let req_c = req.clone();
        tokio::task::spawn_blocking(move || this.preview_sync(&req_c))
            .await
            .map_err(|e| AkrepError::Internal(format!("quarantine preview join error: {}", e)))?
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        let this = self.clone();
        let req_c = req.clone();
        let snap_c = snapshot_dir.to_path_buf();
        tokio::task::spawn_blocking(move || this.apply_sync(&req_c, &snap_c))
            .await
            .map_err(|e| AkrepError::Internal(format!("quarantine apply join error: {}", e)))?
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let this = self.clone();
        let req_c = req.clone();
        let exp_c = expected.clone();
        tokio::task::spawn_blocking(move || this.verify_sync(&req_c, &exp_c))
            .await
            .map_err(|e| AkrepError::Internal(format!("quarantine verify join error: {}", e)))?
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let this = self.clone();
        let req_c = req.clone();
        let fps_c = post_fps.to_vec();
        tokio::task::spawn_blocking(move || this.current_fingerprints_sync(&req_c, &fps_c))
            .await
            .map_err(|e| AkrepError::Internal(format!("quarantine fp join error: {}", e)))?
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let this = self.clone();
        let rec_c = record.clone();
        let snap_c = snapshot_dir.to_path_buf();
        tokio::task::spawn_blocking(move || this.rollback_sync(&rec_c, &snap_c))
            .await
            .map_err(|e| AkrepError::Internal(format!("quarantine rollback join error: {}", e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_sha256_matches_core_sha256_exactly() {
        for len in [0usize, 1, 55, 56, 63, 64, 65, 128, 4096, 100_003] {
            let data: Vec<u8> = (0..len).map(|i| ((i * 31 + 7) & 0xff) as u8).collect();
            let mut h = Sha256Hasher::new();
            for chunk in data.chunks(137) {
                h.update(chunk);
            }
            assert_eq!(
                h.finalize_hex(),
                sha256_hex(&data),
                "mismatch at len={}",
                len
            );
        }
    }
}
