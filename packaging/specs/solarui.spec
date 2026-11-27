%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           solarui
Version:        5.05.1
Release:        3%{?dist}
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
/usr/bin/solar-session-wrapper
/usr/bin/solar-core
/usr/bin/solar-settings
/usr/bin/solar-lock
/usr/bin/solar-omnibar
/usr/bin/blaze-gamezone
/usr/bin/blaze-gamezone-browser
/usr/bin/blaze-style-center
/usr/bin/blaze-system-monitor
/usr/share/solarui
/usr/share/quickshell/we10x
/usr/share/icons/We10X
/usr/share/backgrounds/blazeos/we10x

%changelog
* Fri Nov 27 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-3
- Upgrade BlazeAI Gemini default to gemini-3.8-flash with live model discovery and HTTP 404 auto-retry, guard GTK4/session icon theme fallback to Adwaita, speed up solar-settings startup, migrate user solarui-portals.conf, and drop forced global GDK_BACKEND
* Thu Nov 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-2
- Add Solar Omnibar Persephone/Koz vector companion, 16-persona BlazeAI with local memory, UI unslop polish, and solar-settings --tab routing fix
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Own full SolarUI binary/helper set and /usr/share/solarui resources
* Wed Nov 18 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of SolarUI for the Blaze repository
