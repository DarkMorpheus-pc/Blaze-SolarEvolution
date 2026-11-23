#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ORIG_INITRD="${ORIG_INITRD:-$REPO_ROOT/work_f45_stable/initrd_orig/initrd}"
OUT_INITRD="${OUT_INITRD:-$REPO_ROOT/work_f45_stable/initrd_work/patched_initrd}"
OVERLAY="${OVERLAY:-$REPO_ROOT/blazeos_custom_apps}"
ROOTFS_SRC="${ROOTFS_SRC:-$REPO_ROOT/work_f45_stable/rootfs}"

if [[ ! -f "$ORIG_INITRD" ]]; then
  echo "ERROR: Original initrd not found: $ORIG_INITRD" >&2
  exit 1
fi

for req in \
  "$OVERLAY/etc/plymouth/plymouthd.conf" \
  "$OVERLAY/usr/lib64/plymouth/script.so" \
  "$OVERLAY/usr/share/plymouth/themes/spinner/watermark.png" \
  "$OVERLAY/usr/share/plymouth/themes/blazeos/blazeos.plymouth" \
  "$OVERLAY/usr/share/plymouth/themes/blazeos/blazeos.script" \
  "$OVERLAY/transparent_boot_logo.png" \
  "$OVERLAY/usr/bin/blaze-death-screen" \
  "$OVERLAY/usr/bin/blaze-recovery" \
  "$OVERLAY/usr/share/blazeos/identity/os-release" \
  "$OVERLAY/usr/share/blazeos/identity/fedora-release" \
  "$OVERLAY/usr/share/blazeos/identity/system-release-cpe" \
  "$OVERLAY/usr/share/blazeos/identity/buildstamp" \
  "$OVERLAY/usr/lib/modprobe.d/blazeos-usb-quirks.conf"; do
  if [[ ! -f "$req" ]]; then
    echo "ERROR: Required asset missing: $req" >&2
    exit 1
  fi
done

OFFSET="$(python3 - "$ORIG_INITRD" <<'PY'
import sys

path = sys.argv[1]
with open(path, "rb") as f:
    data = f.read()

trailer = b"TRAILER!!!"
zstd_magic = b"\x28\xb5\x2f\xfd"

t_idx = data.find(trailer)
if t_idx < 0:
    raise SystemExit("ERROR: TRAILER!!! marker not found in initrd")

z_idx = data.find(zstd_magic, t_idx + len(trailer))
if z_idx < 0:
    raise SystemExit("ERROR: zstd magic not found after TRAILER!!!")

gap = data[t_idx + len(trailer):z_idx]
if gap.strip(b"\x00"):
    raise SystemExit("ERROR: non-zero bytes between early cpio TRAILER!!! and zstd stream")

print(z_idx)
PY
)"

echo "Detected zstd offset after TRAILER!!!: $OFFSET"

TMP_DIR="$(mktemp -d /tmp/blaze_patch_initrd.XXXXXX)"
trap 'rm -rf "$TMP_DIR"' EXIT

UNPACK_DIR="$TMP_DIR/rootfs"
mkdir -p "$UNPACK_DIR" "$(dirname "$OUT_INITRD")"

head -c "$OFFSET" "$ORIG_INITRD" > "$TMP_DIR/early.cpio"

fakeroot bash -c '
  set -euo pipefail
  orig_initrd="$1"
  offset="$2"
  overlay="$3"
  rootfs_src="$4"
  unpack_dir="$5"
  out_zst="$6"

  cd "$unpack_dir"
  tail -c +$((offset + 1)) "$orig_initrd" | zstd -d -c | cpio -id --no-absolute-filenames --quiet

  # 1. BlazeOS Plymouth boot theme
  install -D -m 0644 "$overlay/etc/plymouth/plymouthd.conf" etc/plymouth/plymouthd.conf
  install -D -m 0755 "$overlay/usr/lib64/plymouth/script.so" usr/lib64/plymouth/script.so
  install -D -m 0644 "$overlay/usr/share/plymouth/themes/spinner/watermark.png" usr/share/plymouth/themes/spinner/watermark.png

  mkdir -p usr/share/plymouth/themes/blazeos
  cp -a --no-preserve=ownership "$overlay/usr/share/plymouth/themes/blazeos/." usr/share/plymouth/themes/blazeos/
  chmod 0644 usr/share/plymouth/themes/blazeos/*
  install -D -m 0644 "$overlay/transparent_boot_logo.png" usr/share/plymouth/themes/blazeos/logo.png
  install -D -m 0644 "$overlay/transparent_boot_logo.png" usr/share/plymouth/themes/blazeos/logo-glow.png

  # 2. BlazeOS Screen of Death (blaze-death-screen) & standalone BERP (blaze-recovery)
  install -D -m 0755 "$overlay/usr/bin/blaze-death-screen" usr/bin/blaze-death-screen
  install -D -m 0755 "$overlay/usr/bin/blaze-recovery" usr/bin/blaze-recovery
  for helper_bin in lsblk chvt du; do
    if [[ -x "$rootfs_src/usr/bin/$helper_bin" ]]; then
      install -D -m 0755 "$rootfs_src/usr/bin/$helper_bin" "usr/bin/$helper_bin"
    fi
  done

  for unit_dir in \
    etc/systemd/system/emergency.service.d \
    etc/systemd/system/dracut-emergency.service.d \
    etc/systemd/system/rescue.service.d; do
    mkdir -p "$unit_dir"
    cat > "$unit_dir/99-blaze-death-screen.conf" <<EOF
[Service]
ExecStartPre=-/usr/bin/plymouth quit
ExecStartPre=-/usr/bin/chvt 1
ExecStart=
ExecStart=-/usr/bin/blaze-death-screen
StandardInput=tty-force
StandardOutput=tty
StandardError=tty
TTYPath=/dev/tty1
EOF
  done

  mkdir -p \
    var/lib/dracut/hooks/cmdline \
    var/lib/dracut/hooks/emergency \
    var/lib/dracut/hooks/initqueue/finished \
    var/lib/dracut/hooks/initqueue/online \
    var/lib/dracut/hooks/initqueue/settled \
    var/lib/dracut/hooks/initqueue/timeout \
    var/lib/dracut/hooks/mount \
    var/lib/dracut/hooks/pre-mount \
    var/lib/dracut/hooks/pre-pivot \
    var/lib/dracut/hooks/pre-trigger \
    var/lib/dracut/hooks/pre-udev \
    var/lib/dracut/hooks/cleanup \
    var/lib/dracut/hooks/shutdown
  rm -rf usr/lib/dracut/hooks
  ln -s ../../../var/lib/dracut/hooks usr/lib/dracut/hooks

  cat > var/lib/dracut/hooks/emergency/00-blaze-death-screen.sh <<EOF
#!/usr/bin/sh
/usr/bin/plymouth quit >/dev/null 2>&1 || :
/usr/bin/chvt 1 >/dev/null 2>&1 || :
/usr/bin/blaze-death-screen </dev/tty1 >/dev/tty1 2>&1
EOF
  chmod 0755 var/lib/dracut/hooks/emergency/00-blaze-death-screen.sh
  printf "BLAZE_BERE_DEATH_GUARD_V1\n" > etc/blaze-bere-guard

  # 3. Blaze SolarEvolution identity inside initramfs (dracut/Plymouth/Anaconda)
  # Preserve an explicit Fedora base marker in initrd-release so Ventoy'\''s
  # rhel7/ventoy-hook.sh (`grep -q Fedora /etc/os-release`) triggers iso-scan cleanly.
  dracut_ver="$(sed -n "s/^DRACUT_VERSION=//p" usr/lib/initrd-release 2>/dev/null | head -n 1 | tr -d "\"")"
  install -D -m 0644 "$overlay/usr/share/blazeos/identity/os-release" usr/lib/initrd-release
  printf "BASE_DISTRO=\"Fedora Linux 45\"\n" >> usr/lib/initrd-release
  if [[ -n "$dracut_ver" ]]; then
    sed -i -e "s|^VERSION=.*|VERSION=\"5.05 ${dracut_ver}\"|" \
           -e "s|^PRETTY_NAME=.*|PRETTY_NAME=\"Blaze SolarEvolution 5.05 ${dracut_ver} (Initramfs)\"|" \
           usr/lib/initrd-release
    printf "DRACUT_VERSION=\"%s\"\n" "$dracut_ver" >> usr/lib/initrd-release
  else
    sed -i -e "s|^PRETTY_NAME=.*|PRETTY_NAME=\"Blaze SolarEvolution 5.05 (Initramfs)\"|" usr/lib/initrd-release
  fi
  ln -sf ../usr/lib/initrd-release etc/initrd-release
  ln -sf initrd-release usr/lib/os-release
  ln -sf initrd-release etc/os-release
  install -D -m 0644 "$overlay/usr/share/blazeos/identity/fedora-release" usr/lib/fedora-release
  install -D -m 0644 "$overlay/usr/share/blazeos/identity/system-release-cpe" usr/lib/system-release-cpe
  install -D -m 0644 "$overlay/usr/share/blazeos/identity/buildstamp" .buildstamp
  ln -sf ../usr/lib/fedora-release etc/fedora-release
  ln -sf fedora-release etc/redhat-release
  ln -sf fedora-release etc/system-release
  ln -sf ../usr/lib/system-release-cpe etc/system-release-cpe

  # 4. Real-hardware USB bridge UAS quirks & Ventoy /dev/ventoy dmsetup resilience
  install -D -m 0644 "$overlay/usr/lib/modprobe.d/blazeos-usb-quirks.conf" usr/lib/modprobe.d/blazeos-usb-quirks.conf
  mkdir -p etc/udev/rules.d
  cat > etc/udev/rules.d/99-blaze-ventoy-compat.rules <<'\''UDEVEOF'\''
SUBSYSTEM=="block", KERNEL=="dm-[0-9]*", ATTR{dm/name}=="ventoy", ENV{DM_UDEV_DISABLE_DISK_RULES_FLAG}="", IMPORT{builtin}="blkid", SYMLINK+="ventoy mapper/ventoy", TAG+="systemd"
UDEVEOF
  chmod 0644 etc/udev/rules.d/99-blaze-ventoy-compat.rules

  python3 - usr/bin/dmsquash-live-root <<'\''PYEOF'\''
import pathlib, sys

p = pathlib.Path(sys.argv[1])
txt = p.read_text(encoding="utf-8")

old_fs = """    livedev_fstype=$(det_fs "$livedev")
    load_fstype "$livedev_fstype\""""
new_fs = """    livedev_fstype=$(det_fs "$livedev")
    if [ -z "$livedev_fstype" ] || [ "$livedev_fstype" = "auto" ]; then
        livedev_fstype=$(blkid -s TYPE -o value "$livedev" 2>/dev/null || echo "auto")
        [ -z "$livedev_fstype" ] && livedev_fstype="auto"
    fi
    if [ "$livedev_fstype" = "iso9660" ] || [ "$livedev_fstype" = "udf" ]; then
        modprobe -q isofs 2>/dev/null || :
    fi
    load_fstype "$livedev_fstype\""""

old_end = "ln -s null /dev/root"
new_end = """ln -s null /dev/root
command -v cancel_wait_for_dev > /dev/null || . /lib/dracut-dev-lib.sh
if [ "$livedev" = "/dev/ventoy" ] || [ -e /dev/ventoy ]; then
    cancel_wait_for_dev /dev/ventoy 2>/dev/null || :
fi"""

if old_fs not in txt:
    raise SystemExit("ERROR: Expected det_fs block not found in usr/bin/dmsquash-live-root")
if old_end not in txt:
    raise SystemExit("ERROR: Expected /dev/root block not found in usr/bin/dmsquash-live-root")

txt = txt.replace(old_fs, new_fs, 1).replace(old_end, new_end, 1)
p.write_text(txt, encoding="utf-8")
PYEOF
  sh -n usr/bin/dmsquash-live-root

  find . -mindepth 1 -print0 | LC_ALL=C sort -z | cpio --null -o -H newc --quiet | zstd -T0 -15 -c > "$out_zst"
' _ "$ORIG_INITRD" "$OFFSET" "$OVERLAY" "$ROOTFS_SRC" "$UNPACK_DIR" "$TMP_DIR/main.cpio.zst"

cat "$TMP_DIR/early.cpio" "$TMP_DIR/main.cpio.zst" > "$OUT_INITRD.tmp"
python3 - "$OUT_INITRD.tmp" <<'PYALIGN'
import os, sys

path = sys.argv[1]
rem = os.path.getsize(path) % 4
if rem:
    pad_len = 4 - rem
    # Append RFC 8878 Zstandard skippable frame (8-byte header + pad_len bytes)
    # so both `zstd -d` / `zstdcat` and Linux kernel initramfs trailing cpio stay valid & 4-byte aligned.
    with open(path, "ab") as f:
        f.write(b"\x50\x2a\x4d\x18" + pad_len.to_bytes(4, "little") + (b"\x00" * pad_len))
PYALIGN
mv -f "$OUT_INITRD.tmp" "$OUT_INITRD"

echo "Patched initrd written to: $OUT_INITRD ($(stat -c %s "$OUT_INITRD") bytes)"
