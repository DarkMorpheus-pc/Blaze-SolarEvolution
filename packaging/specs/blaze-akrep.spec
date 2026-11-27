%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none

Name:           blaze-akrep
Version:        1.0.0
Release:        3.blaze
Summary:        BlazeOS Akrep Security Center
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
ExclusiveArch:  x86_64

Requires:       blaze-akrep-daemon = %{version}-%{release}
Requires:       blaze-akrep-ui = %{version}-%{release}

%description
BlazeOS Akrep v1.0 Security Center metapackage installing the strictly
read-only akrepd telemetry service, the on-demand Polkit-authenticated
akrep-privd transaction daemon, declarative security profiles, and the
GTK4 Akrep Security Center desktop interface.

%package daemon
Summary:        BlazeOS Akrep telemetry and Polkit transaction daemons
Requires(pre):  shadow-utils
Requires(post): systemd
Requires(preun): systemd
Requires(postun): systemd
Recommends:     firewalld
Suggests:       clamav, clamd, clamav-freshclam

%description daemon
Three-tier security backend for BlazeOS Akrep v1.0: strictly read-only
telemetry daemon (akrepd on tr.com.blazeos.Akrep1) and on-demand Polkit
transaction daemon (akrep-privd on tr.com.blazeos.Akrep1.Privileged), with
zero GTK4 runtime dependency.

%package ui
Summary:        BlazeOS Akrep Security Center GTK4 desktop interface
Requires:       gtk4

%description ui
Unprivileged GTK4 Security Center desktop client (akrep-ui) with 10-section
navigation, bilingual TR/EN support, live telemetry, and transactional plan
preview/rollback controls.

%prep
%setup -c -T

%build

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}
tar -C %{buildroot} -xf %{SOURCE0}

%pre daemon
getent group akrep >/dev/null || groupadd -r akrep
getent passwd akrep >/dev/null || useradd -r -g akrep -d /run/akrep -s /sbin/nologin -c "BlazeOS Akrep Security Telemetry Service" akrep
exit 0

%post daemon
/usr/bin/systemd-tmpfiles --create /usr/lib/tmpfiles.d/blaze-akrep.conf >/dev/null 2>&1 || :
%systemd_post akrepd.service

%preun daemon
%systemd_preun akrepd.service
if [ $1 -eq 0 ] && [ -x /usr/bin/systemctl ]; then
    /usr/bin/systemctl stop akrep-privd.service >/dev/null 2>&1 || :
fi

%postun daemon
%systemd_postun_with_restart akrepd.service

%files

%files daemon
%attr(0755,root,root) /usr/bin/akrepd
%attr(0755,root,root) /usr/bin/akrep-privd
%attr(0644,root,root) /usr/lib/sysusers.d/blaze-akrep.conf
%attr(0644,root,root) /usr/lib/tmpfiles.d/blaze-akrep.conf
%attr(0644,root,root) /usr/lib/systemd/system/akrepd.service
%attr(0644,root,root) /usr/lib/systemd/system/akrep-privd.service
%attr(0644,root,root) /usr/lib/systemd/system-preset/90-blaze-akrep.preset
%attr(0644,root,root) /usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.conf
%attr(0644,root,root) /usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.Privileged.conf
%attr(0644,root,root) /usr/share/dbus-1/system-services/tr.com.blazeos.Akrep1.Privileged.service
%attr(0644,root,root) /usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy
%dir /usr/share/akrep
%dir /usr/share/akrep/profiles
%attr(0644,root,root) /usr/share/akrep/profiles/balanced.toml
%attr(0644,root,root) /usr/share/akrep/profiles/developer.toml
%attr(0644,root,root) /usr/share/akrep/profiles/gaming.toml
%attr(0644,root,root) /usr/share/akrep/profiles/public-network.toml

%files ui
%attr(0755,root,root) /usr/bin/akrep-ui
%attr(0644,root,root) /usr/share/applications/tr.com.blazeos.Akrep.desktop

%changelog
* Fri Nov 27 2026 BlazeOS Team <contact@blazeos.org> - 1.0.0-3.blaze
- Guard GTK4 icon theme resolution in akrep-ui so symbolic navigation icons fall back to Adwaita when configured theme is not installed
* Thu Nov 26 2026 BlazeOS Team <contact@blazeos.org> - 1.0.0-2.blaze
- Add timed and persistent flexible protection overrides, Android ROM/gaming compatibility helpers, Secure Boot/MOK card, and UI unslop polish
* Wed Nov 25 2026 BlazeOS Team <contact@blazeos.org> - 1.0.0-1.blaze
- Promote Akrep to v1.0 with akrep-privd Polkit transaction engine, 4 TOML security profiles, ClamAV Suggests, and 10-tab GTK4 Security Center UI
* Wed Nov 25 2026 BlazeOS Team <contact@blazeos.org> - 0.1.0-1.blaze
- Initial strictly read-only Akrep v0.1 daemon, UI, D-Bus & Polkit skeleton split
