Name:           blaze-release
Version:        5.05.1
Release:        4%{?dist}
Summary:        Blaze repository definition, GPG key and OS identity guard
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
BuildArch:      noarch
Source0:        blaze.repo
Source1:        RPM-GPG-KEY-blaze
Source2:        %{name}-%{version}.tar
Requires:       coreutils
Requires:       gawk
Requires:       grep
Recommends:     blaze-akrep
Recommends:     solarui-config
Recommends:     blaze-scap-compat

%description
Installs the Blaze package repository definition, the public GPG signing key,
DNF5 releasever=45 pinning and excludepkgs protections, kernel-install hooks,
and the Blaze SolarEvolution 5.05.1 identity guard so system updates preserve
BlazeOS branding across release files, GRUB entries, Anaconda, and logos.

%prep
%setup -q -c -T -a 2

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar
install -Dpm0644 %{SOURCE0} %{buildroot}/usr/share/dnf5/repos.d/blaze.repo
install -Dpm0644 %{SOURCE1} %{buildroot}/etc/pki/rpm-gpg/RPM-GPG-KEY-blaze

%post
if [ -d /etc/systemd/system ]; then
    mkdir -p /etc/systemd/system/multi-user.target.wants
    ln -sf /etc/systemd/system/blazeos-identity-guard.service \
           /etc/systemd/system/multi-user.target.wants/blazeos-identity-guard.service 2>/dev/null || :
fi
if [ ! -x /usr/bin/gnome-shell ]; then
    rm -f /etc/dnf/protected.d/fedora-workstation.conf 2>/dev/null || :
fi
if [ -x /usr/bin/blazeos-sync-identity ]; then
    /usr/bin/blazeos-sync-identity --force --quiet || :
fi

%posttrans
# Heal SUID/SGID permissions on FUSE & system helpers if stripped by older ISO installs
for suid_bin in /usr/bin/fusermount3 /usr/bin/fusermount /usr/bin/fusermount-glusterfs /usr/bin/mount.nfs; do
    if [ -f "$suid_bin" ] && [ ! -u "$suid_bin" ]; then
        chmod 4755 "$suid_bin" 2>/dev/null || :
    fi
done
# Ensure newuidmap/newgidmap use file capabilities ONLY (0755 + cap_setuid/cap_setgid)
# because combining SUID 4755 with file caps drops cap_dac_override and breaks rootless podman!
if [ -f /usr/bin/newuidmap ] && [ -u /usr/bin/newuidmap ] && command -v setcap >/dev/null 2>&1; then
    chmod 0755 /usr/bin/newuidmap 2>/dev/null || :
    setcap cap_setuid+ep /usr/bin/newuidmap 2>/dev/null || chmod 4755 /usr/bin/newuidmap 2>/dev/null || :
fi
if [ -f /usr/bin/newgidmap ] && [ -u /usr/bin/newgidmap ] && command -v setcap >/dev/null 2>&1; then
    chmod 0755 /usr/bin/newgidmap 2>/dev/null || :
    setcap cap_setgid+ep /usr/bin/newgidmap 2>/dev/null || chmod 4755 /usr/bin/newgidmap 2>/dev/null || :
fi
# Heal dconf baseline icon-theme when Papirus-Dark is not installed so GTK4 symbolic icons resolve via Adwaita
if [ -f /etc/dconf/db/local.d/00-blaze-theme ] && [ ! -f /usr/share/icons/Papirus-Dark/index.theme ]; then
    if grep -q "icon-theme='Papirus-Dark'" /etc/dconf/db/local.d/00-blaze-theme 2>/dev/null; then
        sed -i "s/icon-theme='Papirus-Dark'/icon-theme='Adwaita'/g" /etc/dconf/db/local.d/00-blaze-theme 2>/dev/null || :
        dconf update 2>/dev/null || :
    fi
fi
if [ -x /usr/bin/blazeos-sync-identity ]; then
    /usr/bin/blazeos-sync-identity --quiet || :
fi

%transfiletriggerin -- /etc /usr/lib /boot/loader/entries /usr/share/pixmaps /usr/share/icons
if grep -E '^(/etc/(os-release|fedora-release|redhat-release|system-release|system-release-cpe|issue(\.net)?|default/grub)|/usr/lib/(os-release|fedora-release|system-release-cpe|issue(\.net)?)|/boot/loader/entries/|/usr/share/(pixmaps|icons)/)' >/dev/null 2>&1; then
    if [ -x /usr/bin/blazeos-sync-identity ]; then
        /usr/bin/blazeos-sync-identity --force --quiet || :
    fi
fi

%transfiletriggerpostun -- /etc /usr/lib /boot/loader/entries /usr/share/pixmaps /usr/share/icons
if grep -E '^(/etc/(os-release|fedora-release|redhat-release|system-release|system-release-cpe|issue(\.net)?|default/grub)|/usr/lib/(os-release|fedora-release|system-release-cpe|issue(\.net)?)|/boot/loader/entries/|/usr/share/(pixmaps|icons)/)' >/dev/null 2>&1; then
    if [ -x /usr/bin/blazeos-sync-identity ]; then
        /usr/bin/blazeos-sync-identity --quiet || :
    fi
fi

%files
/usr/share/dnf5/repos.d/blaze.repo
/etc/pki/rpm-gpg/RPM-GPG-KEY-blaze
/usr/share/blazeos/identity
%attr(0755,root,root) /usr/bin/blazeos-sync-identity
/etc/dnf/vars/releasever
/usr/share/dnf5/vars.d/releasever
/etc/dnf/libdnf5.conf.d/10-blaze-identity.conf
/etc/dnf/protected.d/blaze-release.conf
/etc/dnf/libdnf5-plugins/actions.d/blaze-identity.actions
%attr(0755,root,root) /etc/kernel/install.d/05-blaze-identity.install
%attr(0755,root,root) /etc/kernel/install.d/99-zz-blaze-identity.install
/etc/systemd/system/blazeos-identity-guard.service
/etc/anaconda/profile.d/blazeos-solarevolution.conf
/etc/blazeos/update-channel.conf

%changelog
* Fri Nov 27 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-4
- Shorten blaze.repo metadata_expire to 15m so new BlazeHouse OTA releases appear promptly
* Fri Nov 27 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-3
- Heal missing fusermount3/FUSE SUID bits, capability-only newuidmap/newgidmap permissions, and uninstalled Papirus-Dark dconf icon theme baseline in %%posttrans

* Thu Nov 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-2
- Recommend blaze-akrep, solarui-config, and blaze-scap-compat for seamless in-place upgrades from 5.05

* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Bump SolarEvolution identity to 5.05.1 with BLAZE_BASE_ID=fedora and BLAZE_BASE_VERSION_ID=45

* Mon Nov 23 2026 Blaze Release <release@blazeos.com.tr> - 5.05-3
- Update Blaze-SolarEvolution GitHub URLs and add /etc/blazeos/update-channel.conf for BlazeHouse channel upgrades

* Mon Nov 23 2026 Blaze Release <release@blazeos.com.tr> - 5.05-2
- Embed Blaze SolarEvolution 5.05 identity guard, DNF5 releasever=45 pinning, excludepkgs, kernel-install hooks, and RPM file triggers

* Mon Oct 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial blaze-release package
