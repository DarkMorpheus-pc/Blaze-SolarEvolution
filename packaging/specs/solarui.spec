%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           solarui
Version:        5.05
Release:        1%{?dist}
Summary:        SolarUI desktop shell manager, session and settings
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
ExclusiveArch:  x86_64
Requires:       gtk4

%description
Niri-based Wayland session manager, multi-shell switcher (Noctalia/Caelestia/end4-pC/We10X), omnibar and settings GUI.

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar

%files
/usr/bin/solar-shell
/usr/bin/solar-session
/usr/bin/solar-core
/usr/bin/solar-settings
/usr/share/quickshell/we10x
/usr/share/icons/We10X
/usr/share/backgrounds/blazeos/we10x

%changelog
* Wed Nov 18 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of SolarUI for the Blaze repository
