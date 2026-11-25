%global debug_package %{nil}

Name:           blaze-akrep
Version:        0.1.0
Release:        1.blaze
Summary:        BlazeOS Akrep Security Center (Read-Only v0.1 Meta)
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar

Requires:       blaze-akrep-daemon = %{version}-%{release}
Requires:       blaze-akrep-ui = %{version}-%{release}

%description
BlazeOS Akrep v0.1 Security Center metapackage installing both the strictly
read-only akrepd system telemetry D-Bus service and the GTK4 Akrep UI.

%package daemon
Summary:        BlazeOS Akrep strictly read-only D-Bus security telemetry daemon
Requires(pre):  shadow-utils
Requires(post): systemd
Requires(preun): systemd
Requires(postun): systemd
Recommends:     firewalld

%description daemon
System service (akrepd) exposing read-only security posture telemetry over
tr.com.blazeos.Akrep1 without any GTK4 runtime dependency.

%package ui
Summary:        BlazeOS Akrep Security Center GTK4 read-only interface
Requires:       gtk4

%description ui
Unprivileged GTK4 Security Center desktop client (akrep-ui) displaying live
read-only telemetry from akrepd or a clear degraded banner when offline.

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
%systemd_post akrepd.service

%preun daemon
%systemd_preun akrepd.service

%postun daemon
%systemd_postun_with_restart akrepd.service

%files

%files daemon
%attr(0755,root,root) /usr/bin/akrepd
%attr(0644,root,root) /usr/lib/sysusers.d/blaze-akrep.conf
%attr(0644,root,root) /usr/lib/systemd/system/akrepd.service
%attr(0644,root,root) /usr/lib/systemd/system-preset/90-blaze-akrep.preset
%attr(0644,root,root) /usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.conf
%attr(0644,root,root) /usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy

%files ui
%attr(0755,root,root) /usr/bin/akrep-ui
%attr(0644,root,root) /usr/share/applications/tr.com.blazeos.Akrep.desktop

%changelog
* Wed Nov 25 2026 BlazeOS Team <contact@blazeos.org> - 0.1.0-1.blaze
- Initial strictly read-only Akrep v0.1 daemon, UI, D-Bus & Polkit skeleton split
