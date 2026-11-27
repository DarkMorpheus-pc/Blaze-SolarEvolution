%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           blaze-oobe
Version:        5.05.1
Release:        2%{?dist}
Summary:        BlazeOS Out-Of-Box Experience first-boot wizard
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
BuildArch:      noarch
Requires:       python3

%description
Token-authenticated first-boot setup assistant and post-OOBE hygiene helper.

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar

%files
/usr/bin/blaze-setup
/usr/share/blaze-setup
/usr/libexec/blaze-oobe-session
/usr/libexec/blazeos-oobe-hygiene
%config(missingok) /etc/sddm.conf.d/00-blaze-oobe.conf
%config(missingok) /usr/share/wayland-sessions/blaze-oobe.desktop
%config(missingok) %attr(0440,root,root) /etc/sudoers.d/99-blaze-setup

%changelog
* Fri Nov 27 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-2
- Use Adwaita icon theme fallback in OOBE GTK/dconf baseline when Papirus-Dark is not installed
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Track one-shot OOBE SDDM/sudoers drop-ins with missingok and harden post-OOBE transition
* Wed Nov 18 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of BlazeOS OOBE for the Blaze repository
