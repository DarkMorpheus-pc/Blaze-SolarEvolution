%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           blaze-tools
Version:        5.05
Release:        1%{?dist}
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

%changelog
* Mon Oct 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of blaze-tools for the Blaze repository
