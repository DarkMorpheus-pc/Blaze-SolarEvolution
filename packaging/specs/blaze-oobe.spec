%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
# Binary repack of artefacts built from the repositories listed in SOURCES.md
Name:           blaze-oobe
Version:        5.05
Release:        1%{?dist}
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

%changelog
* Wed Nov 18 2026 Blaze Release <release@blazeos.com.tr> - 5.05-1
- Initial RPM packaging of BlazeOS OOBE for the Blaze repository
