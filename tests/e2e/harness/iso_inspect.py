"""Read-only inspection helpers for ISO, initrd, squashfs, RPMDB, GPG, and ELF NEEDED."""
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
from typing import Dict, List, Set, Tuple


def _c_locale_env() -> Dict[str, str]:
    """Return a copy of os.environ with LC_ALL=C and LANG=C for stable tool output."""
    return {**os.environ, "LC_ALL": "C", "LANG": "C"}


def sha256_file(path: Path, chunk_size: int = 1024 * 1024) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def md5_file(path: Path, chunk_size: int = 1024 * 1024) -> str:
    h = hashlib.md5()
    with open(path, "rb") as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            h.update(chunk)
    return h.hexdigest()


def verify_iso_sha256_file(iso_path: Path, sha_file: Path) -> Tuple[bool, str]:
    line = sha_file.read_text(encoding="utf-8").strip().splitlines()[0]
    expected = line.split()[0].lower()
    actual = sha256_file(iso_path).lower()
    return actual == expected, f"expected={expected} actual={actual}"


def checkisomd5_path() -> str:
    found = shutil.which("checkisomd5")
    if found:
        return found
    home_bin = Path.home() / "bin" / "checkisomd5"
    if home_bin.is_file() and os.access(home_bin, os.X_OK):
        return str(home_bin)
    raise FileNotFoundError("checkisomd5 not found in PATH or ~/bin")


def run_checkisomd5(iso_path: Path) -> subprocess.CompletedProcess:
    exe = checkisomd5_path()
    return subprocess.run(
        [exe, "--verbose", str(iso_path)],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        timeout=180,
    )


def xorriso_list(iso_path: Path, iso_dir: str = "/") -> List[str]:
    res = subprocess.run(
        ["xorriso", "-indev", str(iso_path), "-find", iso_dir, "-exec", "echo"],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=60,
    )
    return [line.strip().strip("'") for line in res.stdout.splitlines() if line.strip()]


def xorriso_extract(iso_path: Path, iso_member: str, dest_path: Path) -> Path:
    dest_path.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        [
            "xorriso",
            "-indev",
            str(iso_path),
            "-osirrox",
            "on",
            "-extract",
            iso_member,
            str(dest_path),
        ],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=180,
    )
    return dest_path


def xorriso_pvd_info(iso_path: Path) -> str:
    res = subprocess.run(
        ["xorriso", "-indev", str(iso_path), "-pvd_info"],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=60,
    )
    return res.stdout + "\n" + res.stderr


def find_zstd_initrd_offset(initrd_bytes: bytes) -> int:
    """Locate the zstd-compressed cpio payload after early uncompressed microcode cpio."""
    zstd_magic = b"\x28\xb5\x2f\xfd"
    idx = initrd_bytes.find(zstd_magic)
    if idx != -1:
        return idx
    raise ValueError("Could not locate zstd magic in initrd")


def _run_zstd_cpio_pipeline(
    zst_file: Path,
    cpio_args: List[str],
    cwd: Path | None = None,
) -> str:
    env = _c_locale_env()
    p1 = subprocess.Popen(
        ["zstd", "-d", "-c", str(zst_file)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        env=env,
    )
    p2 = subprocess.Popen(
        ["cpio", *cpio_args],
        cwd=str(cwd) if cwd is not None else None,
        stdin=p1.stdout,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    assert p1.stdout is not None
    p1.stdout.close()
    try:
        out, err2 = p2.communicate(timeout=60)
        p1.wait(timeout=15)
        err1 = p1.stderr.read().decode("utf-8", errors="replace") if p1.stderr else ""
    except BaseException:
        for proc in (p2, p1):
            if proc.poll() is None:
                proc.kill()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
        raise
    finally:
        for pipe in (p1.stderr, p2.stdout, p2.stderr):
            if pipe is not None:
                try:
                    pipe.close()
                except OSError:
                    pass
    if p1.returncode != 0:
        raise RuntimeError(f"zstd -d failed (rc={p1.returncode}): {err1.strip()}")
    if p2.returncode != 0:
        raise RuntimeError(f"cpio {' '.join(cpio_args)} failed (rc={p2.returncode}): {err2.strip()}")
    return out


def extract_initrd_zstd_cpio_list(initrd_path: Path) -> Set[str]:
    data = initrd_path.read_bytes()
    offset = find_zstd_initrd_offset(data)
    with tempfile.TemporaryDirectory(prefix="blaze_initrd_") as tmp:
        zst_file = Path(tmp) / "initrd.zst"
        zst_file.write_bytes(data[offset:])
        out = _run_zstd_cpio_pipeline(zst_file, ["-it", "--quiet"])
        return {line.lstrip("./") for line in out.splitlines() if line.strip()}


def extract_initrd_zstd_files(initrd_path: Path, patterns: List[str], dest_dir: Path) -> Path:
    data = initrd_path.read_bytes()
    offset = find_zstd_initrd_offset(data)
    dest_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="blaze_initrd_ext_") as tmp:
        zst_file = Path(tmp) / "initrd.zst"
        zst_file.write_bytes(data[offset:])
        _run_zstd_cpio_pipeline(zst_file, ["-idm", "--quiet", *patterns], cwd=dest_dir)
    return dest_dir


def unsquashfs_list(squashfs_path: Path, targets: List[str] | None = None) -> List[str]:
    cmd = ["unsquashfs", "-llc", str(squashfs_path)]
    if targets:
        cmd.extend(targets)
    res = subprocess.run(
        cmd,
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=120,
    )
    return res.stdout.splitlines()


def unsquashfs_cat(squashfs_path: Path, inner_path: str) -> bytes:
    inner = inner_path.lstrip("/")
    res = subprocess.run(
        ["unsquashfs", "-cat", str(squashfs_path), inner],
        env=_c_locale_env(),
        capture_output=True,
        check=True,
        timeout=60,
    )
    return res.stdout


def unsquashfs_extract_paths(squashfs_path: Path, dest_dir: Path, paths: List[str]) -> Path:
    dest_dir.mkdir(parents=True, exist_ok=True)
    clean_paths = [p.lstrip("/") for p in paths]
    subprocess.run(
        ["unsquashfs", "-no-xattrs", "-f", "-d", str(dest_dir), str(squashfs_path), *clean_paths],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=180,
    )
    return dest_dir


def readelf_needed(elf_path: Path) -> List[str]:
    res = subprocess.run(
        ["readelf", "-d", str(elf_path)],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=20,
    )
    needed: List[str] = []
    for line in res.stdout.splitlines():
        if "(NEEDED)" in line:
            m = re.search(r"Shared library:\s*\[([^\]]+)\]", line)
            if m:
                needed.append(m.group(1))
    return needed


def rpm_query_root(root_dir: Path, dbpath: str = "/usr/lib/sysimage/rpm") -> List[str]:
    res = subprocess.run(
        ["rpm", "--root", str(root_dir), "--dbpath", dbpath, "-qa", "--qf", "%{NAME}\n"],
        env=_c_locale_env(),
        capture_output=True,
        text=True,
        check=True,
        timeout=60,
    )
    return sorted(line.strip() for line in res.stdout.splitlines() if line.strip())


def rpm_verify_signatures(rpm_paths: List[Path], gpg_key_path: Path) -> Dict[str, str]:
    results: Dict[str, str] = {}
    env = _c_locale_env()
    with tempfile.TemporaryDirectory(prefix="blaze_rpmdb_") as tmp:
        db_root = Path(tmp)
        subprocess.run(
            ["rpm", "--root", str(db_root), "--initdb"],
            env=env,
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        )
        subprocess.run(
            ["rpm", "--root", str(db_root), "--import", str(gpg_key_path)],
            env=env,
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        )
        for rpm_path in rpm_paths:
            res = subprocess.run(
                ["rpm", "--root", str(db_root), "-K", str(rpm_path)],
                env=env,
                capture_output=True,
                text=True,
                timeout=30,
            )
            results[str(rpm_path)] = f"rc={res.returncode} {res.stdout.strip()} {res.stderr.strip()}".strip()
    return results
