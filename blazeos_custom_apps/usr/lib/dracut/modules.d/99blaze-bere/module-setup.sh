#!/usr/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Dracut module embedding BlazeOS Screen of Death (blaze-death-screen) and
# standalone BERP TUI (blaze-recovery) into initramfs so kernel/rootfs faults
# and Hard Reset wipes always land in BlazeOS's recovery guard instead of a
# locked sulogin prompt.

check() {
    [ -x /usr/bin/blaze-death-screen ] || return 1
    return 0
}

depends() {
    echo "btrfs"
    return 0
}

installkernel() {
    instmods -c squashfs overlay loop ext4 vfat btrfs
}

install() {
    inst_simple /usr/bin/blaze-death-screen /usr/bin/blaze-death-screen
    chmod 0755 "${initdir}/usr/bin/blaze-death-screen"

    if [ -x /usr/bin/blaze-recovery ]; then
        inst_multiple /usr/bin/blaze-recovery
    fi

    inst_multiple -o stty dd chvt clear reset sync reboot poweroff \
        btrfs findmnt lsblk blkid blockdev mount umount losetup kexec modprobe sulogin

    # Ensure udev callouts referencing /sbin/blockdev (99-blaze-recovery-vault.rules) always resolve inside initramfs.
    if [ ! -e "${initdir}/sbin/blockdev" ]; then
        if [ -x /usr/bin/blockdev ]; then
            inst_simple /usr/bin/blockdev /sbin/blockdev
        elif [ -x /sbin/blockdev ]; then
            inst_simple /sbin/blockdev /sbin/blockdev
        fi
    fi

    for unit_dir in \
        "${initdir}/etc/systemd/system/emergency.service.d" \
        "${initdir}/etc/systemd/system/dracut-emergency.service.d" \
        "${initdir}/etc/systemd/system/rescue.service.d"; do
        mkdir -p "$unit_dir"
        cat > "${unit_dir}/99-blaze-death-screen.conf" <<'EOF'
[Service]
ExecStartPre=-/usr/bin/plymouth --wait quit
ExecStartPre=-/usr/bin/chvt 1
ExecStart=
ExecStart=-/usr/bin/blaze-death-screen
StandardInput=tty-force
StandardOutput=tty
StandardError=tty
TTYPath=/dev/tty1
EOF
    done

    inst_hook emergency 00 "${moddir}/blaze-emergency-hook.sh"
}
