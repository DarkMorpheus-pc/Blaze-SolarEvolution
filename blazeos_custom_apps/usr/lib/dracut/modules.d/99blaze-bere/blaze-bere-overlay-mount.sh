#!/usr/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Dracut pre-pivot hook: cryptographically verifies and read-only overlays
# updated BERE/BERP files from BLAZE_REC/LiveOS/bere-overlay.sqsh during
# recovery boots only (never touches normal desktop or Zero-USB reinstall boots).

cmdline="$(cat /proc/cmdline 2>/dev/null || true)"
case " $cmdline " in
    *" systemd.unit=blaze-recovery-gui.target "*|*" systemd.unit=blaze-recovery.target "*)
        ;;
    *)
        return 0 2>/dev/null || exit 0
        ;;
esac

LIVE_MP="/run/initramfs/live"
OV_IMG="${LIVE_MP}/LiveOS/bere-overlay.sqsh"
MANIFEST="${LIVE_MP}/BLAZE_RECOVERY_VAULT.json"

# Must exist as regular non-symlink files
[ -f "$OV_IMG" ] && [ ! -L "$OV_IMG" ] || { return 0 2>/dev/null || exit 0; }
[ -f "$MANIFEST" ] && [ ! -L "$MANIFEST" ] || {
    echo "[BLAZE-BERE] Refusing bere-overlay.sqsh without valid BLAZE_RECOVERY_VAULT.json" > /dev/kmsg 2>/dev/null || true
    return 0 2>/dev/null || exit 0
}

# Extract expected 64-char hex SHA-256 for "bere_overlay" strictly (fail-closed)
expected_sha="$(awk -F'"' '/"bere_overlay"[[:space:]]*:/ {print tolower($4); exit}' "$MANIFEST" 2>/dev/null || true)"
if ! echo "$expected_sha" | grep -Eq '^[0-9a-f]{64}$'; then
    echo "[BLAZE-BERE] TAMPER GUARD: missing or malformed bere_overlay SHA-256 in manifest; skipping overlay" > /dev/kmsg 2>/dev/null || true
    return 0 2>/dev/null || exit 0
fi

actual_sha="$(sha256sum "$OV_IMG" 2>/dev/null | awk '{print tolower($1)}')"
if [ "$actual_sha" != "$expected_sha" ]; then
    echo "[BLAZE-BERE] TAMPER GUARD: bere-overlay.sqsh SHA-256 mismatch; falling back to factory BERE" > /dev/kmsg 2>/dev/null || true
    return 0 2>/dev/null || exit 0
fi

OV_MP="/run/bere-overlay"
mkdir -p -m 0700 "$OV_MP"
if ! mount -t squashfs -o ro,nosuid,nodev "$OV_IMG" "$OV_MP" 2>/dev/null; then
    echo "[BLAZE-BERE] Warning: failed to mount verified bere-overlay.sqsh; falling back to factory BERE" > /dev/kmsg 2>/dev/null || true
    return 0 2>/dev/null || exit 0
fi

# Bind-mount ONLY strictly allowlisted BERE/BERP paths read-only into /sysroot
for rel in \
    usr/bin/blaze-recovery \
    usr/bin/berp-recovery-gui \
    usr/bin/blaze-recovery-gui-launcher \
    usr/bin/blaze-death-screen \
    usr/libexec/blazeos-provision-bere-vault; do
    src="${OV_MP}/${rel}"
    dst="/sysroot/${rel}"
    if [ -f "$src" ] && [ ! -L "$src" ] && [ ! -L "$dst" ]; then
        mkdir -p "$(dirname "$dst")" 2>/dev/null || true
        [ -e "$dst" ] || : > "$dst"
        if mount --bind "$src" "$dst" 2>/dev/null; then
            mount -o remount,ro,bind,nosuid,nodev "$dst" 2>/dev/null || true
        fi
    fi
done

if [ -d "${OV_MP}/usr/share/berp-recovery" ] && [ ! -L "${OV_MP}/usr/share/berp-recovery" ] && [ ! -L "/sysroot/usr/share/berp-recovery" ]; then
    mkdir -p "/sysroot/usr/share/berp-recovery" 2>/dev/null || true
    if mount --bind "${OV_MP}/usr/share/berp-recovery" "/sysroot/usr/share/berp-recovery" 2>/dev/null; then
        mount -o remount,ro,bind,nosuid,nodev "/sysroot/usr/share/berp-recovery" 2>/dev/null || true
    fi
fi

echo "[BLAZE-BERE] Verified bere-overlay.sqsh mounted read-only into recovery sysroot." > /dev/kmsg 2>/dev/null || true
