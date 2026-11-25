%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
%global __requires_exclude_from ^/opt/FireHub/.*$
%global __provides_exclude_from ^/opt/FireHub/.*$

Name:           blaze-firehub
Version:        5.05.1
Release:        1%{?dist}
Summary:        FireHub unified application store and update companion for BlazeOS
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
ExclusiveArch:  x86_64
Requires:       gtk3
Requires:       nss
Requires:       alsa-lib
Requires:       libX11

%description
Production packaging of FireHub (/opt/FireHub, desktop entry, MIME association,
and application icon), excluding local development caches and environment secrets.

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar
find %{buildroot}/opt/FireHub -name '.env*' -o -name '.git*' -o -name '*.log' -o -name '*.map' -delete 2>/dev/null || :
chmod 0755 %{buildroot}/usr/bin/firehub %{buildroot}/opt/FireHub/firehub 2>/dev/null || :
chmod 4755 %{buildroot}/opt/FireHub/chrome-sandbox 2>/dev/null || :

%files
/opt/FireHub
%attr(0755,root,root) /usr/bin/firehub
/usr/share/applications/firehub.desktop
/usr/share/mime/packages/firehub.xml
/usr/share/icons/hicolor/512x512/apps/firehub.png

%changelog
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Initial clean RPM packaging of FireHub without secrets or dev caches
