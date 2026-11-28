%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           blaze-tools
Version:        5.05.1
Release:        3%{?dist}
Summary:        BlazeOS helper tools and installers
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
BuildArch:      noarch
Requires:       bash
Requires:       python3

%description
Post-install, desktop provisioning, NVIDIA/Waydroid/Limine helpers, bootloader switcher, optimizer, session diagnostics and the solar-torture soak test.

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar

%files
/usr/bin/blaze-bootloader
/usr/bin/blaze-optimize
/usr/bin/blazeos-apply-install-choices
/usr/bin/blazeos-control
/usr/bin/blazeos-desktop-provision
/usr/bin/blazeos-limine-install
/usr/bin/blazeos-nvidia-setup
/usr/bin/blazeos-postinstall
/usr/bin/blazeos-remove-solarui
/usr/bin/blazeos-session-diagnose
/usr/bin/blazeos-waydroid-setup
/usr/bin/blazeos-welcome
/usr/bin/solar-torture
/usr/bin/blaze-snapshot
%attr(0755,root,root) /usr/libexec/blaze-style-center-first-run
%attr(0755,root,root) /usr/libexec/blazeos-apply-logos
%attr(0755,root,root) /usr/libexec/blazeos-offline-progress-filter
%attr(0755,root,root) /usr/libexec/blazeos-offline-update
%attr(0755,root,root) /usr/libexec/blazeos-update
%attr(0755,root,root) /usr/libexec/blazeos-update-notifier
%attr(0755,root,root) /usr/libexec/blazeos-wayland-session-ready
%attr(0755,root,root) /usr/libexec/blazeos-welcome-helper
%attr(0755,root,root) /usr/libexec/livesys/sessions.d/livesys-solarui
%config(noreplace) /etc/udev/rules.d/60-blaze-io-schedulers.rules
/etc/dnf/libdnf5-plugins/actions.d/blaze-snapshot.actions

%changelog
* Sat Nov 28 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-3
- Use capability-only 0755 + cap_setuid/cap_setgid instead of SUID 4755 on newuidmap/newgidmap in blazeos-postinstall
* Thu Nov 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-2
- Ship unprivileged BlazeHouse desktop update notifier and trigger armored BERE vault refresh during offline updates
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Complete ownership of Blaze libexec helpers, livesys session hook, and udev I/O scheduler rules
* Mon Oct 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of blaze-tools for the Blaze repository
