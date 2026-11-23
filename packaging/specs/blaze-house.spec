%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           blaze-house
Version:        2.1.0
Release:        1%{?dist}
Summary:        BlazeHouse system control center
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
ExclusiveArch:  x86_64

%description
Tauri v2 system control center (displays, storage, optimization, ZRAM, GitHub update channel and major release upgrades).

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar

%files
/usr/bin/blaze-house
/usr/share/applications/blaze-house.desktop
/usr/share/icons/hicolor/*/apps/blaze-house.png

%changelog
* Mon Nov 23 2026 Blaze Release <release@blazeos.com.tr> - 2.1.0-1
- Add GitHub update channel integration and major Fedora release upgrade workflow
* Mon Oct 26 2026 Blaze Release <release@blazeos.com.tr> - 2.0.0-1
- Initial RPM packaging of blaze-house for the Blaze repository
