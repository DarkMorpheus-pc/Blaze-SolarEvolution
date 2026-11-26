%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           blaze-recovery
Version:        5.05.1
Release:        2%{?dist}
Summary:        BlazeOS BERP/BERE emergency recovery environment
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
ExclusiveArch:  x86_64
Requires:       python3

%description
Graphical recovery environment (BERE) with swipe-to-confirm actions, recovery backend and the terminal Screen of Death.

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar

%posttrans
if [ ! -e /run/initramfs/live ] && [ ! -e /dev/mapper/live-base ]; then
    if ! (command -v systemd-detect-virt >/dev/null 2>&1 && systemd-detect-virt --container --quiet 2>/dev/null); then
        if command -v blkid >/dev/null 2>&1 && [ -n "$(blkid -L BLAZE_REC 2>/dev/null || true)" ]; then
            mkdir -p -m 0755 /var/lib/blazeos 2>/dev/null || :
            printf 'rpm:blaze-recovery-%{version}-%{release}\n' > /var/lib/blazeos/bere-vault-needs-refresh 2>/dev/null || :
            if [ -d /run/systemd/system ] && command -v systemctl >/dev/null 2>&1; then
                systemctl daemon-reload >/dev/null 2>&1 || :
                systemctl start --no-block blazeos-bere-vault-refresh.service >/dev/null 2>&1 || :
            fi
        fi
    fi
fi

%files
/usr/bin/blaze-recovery
/usr/bin/berp-recovery-gui
/usr/bin/blaze-recovery-gui-launcher
/usr/bin/blaze-death-screen
/usr/libexec/blazeos-provision-bere-vault
/usr/share/berp-recovery
/usr/lib/dracut/modules.d/99blaze-bere
%config(noreplace) /etc/dracut.conf.d/99-blaze-bere.conf
%config(noreplace) /etc/udev/rules.d/99-blaze-recovery-vault.rules
%attr(0755,root,root) /etc/grub.d/09_berp
%attr(0755,root,root) /etc/grub.d/99_berp
%attr(0755,root,root) /etc/kernel/install.d/95-blaze-bere-vault.install
/etc/systemd/system/blaze-recovery.service
/etc/systemd/system/blaze-recovery.target
/etc/systemd/system/blaze-recovery.target.wants
/etc/systemd/system/blaze-recovery-gui.service
/etc/systemd/system/blaze-recovery-gui.target
/etc/systemd/system/blaze-recovery-gui.target.wants
/etc/systemd/system/blazeos-bere-vault-refresh.service
/etc/systemd/system/multi-user.target.wants/blazeos-bere-vault-refresh.service

%changelog
* Thu Nov 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-2
- Add SHA-256 and RPMDB-verified physical BLAZE_REC vault self-update overlay, dracut pre-pivot mount guard, kernel refresh hook, and UI unslop polish
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Transactional GRUB/EFI repair, recovery systemd units, and GRUB hooks
* Mon Nov 23 2026 Blaze Release <release@blazeos.com.tr> - 5.05-2
- Support optional full-OS BLAZE_REC vault, SHA-256 Zero-USB kexec reinstall, and udev RO lock
* Mon Oct 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of blaze-recovery for the Blaze repository
