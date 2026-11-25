%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
%global __requires_exclude ^/usr/bin/fish$

Name:           solarui-config
Version:        5.05.1
Release:        1%{?dist}
Summary:        SolarUI and BlazeOS system, compositor, skeleton, and service configurations
License:        GPL-3.0-or-later
URL:            https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution
Source0:        %{name}-%{version}.tar
BuildArch:      noarch
Requires:       solarui
Requires:       blaze-tools

%description
Blaze-authored system, Niri compositor, NetworkManager, sysctl, systemd unit,
Plymouth theme, Wayland session, and skeleton configuration defaults.

%prep
%setup -q -c

%build

%install
mkdir -p %{buildroot}
cp -a . %{buildroot}/
rm -f %{buildroot}/%{name}-%{version}.tar

%files
%config(noreplace) /etc/NetworkManager/conf.d/00-blaze-network.conf
%attr(0755,root,root) /etc/NetworkManager/dispatcher.d/99-blazeos-provision.sh
%config(noreplace) %attr(0600,root,root) /etc/NetworkManager/system-connections/10-wired-autoconnect.nmconnection
/etc/anaconda/conf.d/99-blazeos.conf
%config(noreplace) /etc/fastfetch/config.jsonc
/etc/flatpak/remotes.d/flathub.flatpakrepo
%dir /etc/solarui
%config(noreplace) /etc/solarui/niri.kdl
%config(noreplace) /etc/sysctl.d/99-blaze-optimization.conf
/etc/systemd/resolved.conf.d/fallback_dns.conf
/etc/systemd/system/blaze-boot-success.service
/etc/systemd/system/blaze-optimization.service
/etc/systemd/system/blazeos-apply-install-choices.service
/etc/systemd/system/blazeos-desktop-provision.service
/etc/systemd/system/blazeos-nvidia-firstboot.service
/etc/systemd/system/blazeos-oobe-hygiene.service
/etc/systemd/system/blazeos-plymouth-failsafe.service
/etc/systemd/system/blazeos-postinstall.service
/etc/systemd/system/dnf5-offline-transaction.service.d
/etc/systemd/system/emergency.service.d
/etc/systemd/system/rescue.service.d
/etc/systemd/system/sddm.service.d
/etc/systemd/system/graphical.target.wants/blazeos-apply-install-choices.service
/etc/systemd/system/graphical.target.wants/blazeos-desktop-provision.service
/etc/systemd/system/graphical.target.wants/blazeos-oobe-hygiene.service
/etc/systemd/system/multi-user.target.wants/blazeos-postinstall.service
/etc/systemd/system-preset/00-blazeos.preset
/etc/xdg/autostart/blaze-style-center-first-run.desktop
%config(noreplace) /etc/xdg/mimeapps.list
/etc/skel/.config
/etc/skel/.local
/etc/skel/Desktop
/etc/skel/Pictures
/etc/skel/Resimler
/usr/lib/modprobe.d/blazeos-usb-quirks.conf
%attr(0755,root,root) /usr/local/bin/gnome-control-center
/usr/share/blaze-kinetic-glass
/usr/share/applications/blaze-gamezone.desktop
/usr/share/applications/blazeos-control.desktop
/usr/share/applications/blazeos-waydroid-setup.desktop
/usr/share/applications/blazeos-welcome.desktop
/usr/share/applications/blaze-style-center.desktop
/usr/share/applications/blaze-system-monitor.desktop
/usr/share/applications/solar-settings.desktop
/usr/share/applications/solar-welcome.desktop
/usr/share/backgrounds/blazeos/default.png
/usr/share/backgrounds/blazeos/galaxy.png
/usr/share/backgrounds/blazeos/Wallpaper1s.jpg
/usr/share/backgrounds/blazeos/Wallpaper2s.jpg
/usr/share/backgrounds/blazeos/Wallpaper3s.jpg
/usr/share/backgrounds/blazeos/Wallpaper4s.jpg
/usr/share/backgrounds/blazeos/Wallpaper5s.jpg
/usr/share/backgrounds/blazeos/Wallpaperdefault.jpg
/usr/share/backgrounds/blazeos/wallpapergreen.jpeg
/usr/share/backgrounds/blazeos/wallpaperplant.jpeg
/usr/share/backgrounds/blazeos/wallpaperyellow.jpeg
/usr/share/blazeos/ascii-logo.txt
/usr/share/blazeos/fastfetch
/usr/share/blazeos/gdm
/usr/share/blazeos/grub-efi-stub.cfg
/usr/share/blazeos/icons
/usr/share/blazeos/overlay
/usr/share/blazeos/wallpapers
/usr/share/doc/blazeos
/usr/share/doc/blaze-style-center
/usr/share/licenses/blazeos-control
/usr/share/glib-2.0/schemas/99_blazeos-background.gschema.override
/usr/share/glib-2.0/schemas/99_blazeos-wm.gschema.override
/usr/share/icons/hicolor/128x128/apps/application-generic.png
/usr/share/icons/hicolor/128x128/apps/blaze-system-monitor.png
/usr/share/icons/hicolor/128x128/apps/solar-settings.png
/usr/share/icons/hicolor/256x256/apps/blaze-gamezone.png
/usr/share/icons/hicolor/256x256/apps/solarui.png
/usr/share/icons/hicolor/48x48/apps/solarui.png
/usr/share/icons/hicolor/scalable/apps/blazeos-logo.svg
/usr/share/icons/hicolor/scalable/apps/blazeos-symbolic.svg
/usr/share/icons/hicolor/scalable/apps/solarui.svg
/usr/share/pixmaps/blaze-logo.png
/usr/share/pixmaps/solar-logo.png
/usr/share/plymouth/themes/blazeos
/usr/share/wayland-sessions/solarui.desktop
/usr/share/xdg-desktop-portal/solarui-portals.conf

%changelog
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Split SolarUI and BlazeOS configuration defaults into solarui-config RPM
