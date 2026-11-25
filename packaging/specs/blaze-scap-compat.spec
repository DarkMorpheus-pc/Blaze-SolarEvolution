%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none

Name:           blaze-scap-compat
Version:        5.05.1
Release:        1%{?dist}
Summary:        Blaze SolarEvolution CPE/OVAL compatibility adapter for OpenSCAP SSG
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
BuildArch:      noarch
Requires:       python3
Requires:       blaze-release

%description
Preserves upstream Fedora scap-security-guide files untouched while providing
a CPE/OVAL applicability adapter for Blaze SolarEvolution (ID=blazeos,
BLAZE_BASE_ID=fedora, BLAZE_BASE_VERSION_ID=45).

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar
mkdir -p %{buildroot}/usr/share/blazeos/scap
touch %{buildroot}/usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml

%post
if [ -x /usr/bin/blazeos-scap-adapt ]; then
    /usr/bin/blazeos-scap-adapt --quiet || :
fi

%triggerin -- scap-security-guide
if [ -x /usr/bin/blazeos-scap-adapt ]; then
    /usr/bin/blazeos-scap-adapt --quiet || :
fi

%files
%attr(0755,root,root) /usr/bin/blazeos-scap-adapt
%dir /usr/share/blazeos/scap
/usr/share/blazeos/scap/blazeos-solarevolution-fedora45-cpe.xml
%ghost /usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml
/usr/share/doc/blaze-scap-compat/README.md

%changelog
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Add BlazeOS CPE/OVAL SCAP adapter preserving upstream scap-security-guide
