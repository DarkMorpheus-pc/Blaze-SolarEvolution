%global debug_package %{nil}
%global __strip /bin/true
%global _build_id_links none
%global __requires_exclude ^/usr/bin/fish$

Name:           solarui-config
Version:        5.05.1
Release:        3%{?dist}
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
# Deduplicate byte-identical assets (wallpapers, etc.) via intra-tree hardlinks
# so RPM payloads stay well under GitHub Pages' 100 MiB file limit without
# ever creating cross-filesystem hardlinks between /etc and /usr.
python3 - "%{buildroot}" <<'PY'
import hashlib, os, stat, sys
root = sys.argv[1]
for subtree in ("usr/share", "etc/skel"):
    base = os.path.join(root, subtree)
    if not os.path.isdir(base):
        continue
    seen = {}
    for dp, _dns, fns in os.walk(base):
        for fn in sorted(fns):
            p = os.path.join(dp, fn)
            st = os.lstat(p)
            if not stat.S_ISREG(st.st_mode) or st.st_size < 4096:
                continue
            h = hashlib.sha256()
            with open(p, "rb") as fh:
                for chunk in iter(lambda: fh.read(1048576), b""):
                    h.update(chunk)
            key = (st.st_size, st.st_mode & 0o7777, h.hexdigest())
            if key in seen:
                os.unlink(p)
                os.link(seen[key], p)
            else:
                seen[key] = p
PY

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
/usr/lib/systemd/user/blazeos-update-notifier.service
/usr/lib/systemd/user/blazeos-update-notifier.timer
/usr/lib/systemd/user/timers.target.wants/blazeos-update-notifier.timer
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
%attr(0755,root,root) /usr/libexec/blazeos-wallpaper-portal
/usr/share/dbus-1/services/org.blazeos.WallpaperPortal.service
/usr/share/wayland-sessions/solarui.desktop
/usr/share/xdg-desktop-portal/portals/blazeos.portal
/usr/share/xdg-desktop-portal/solarui-portals.conf

%posttrans
# Ensure existing systems have Nautilus registered for inode/directory
if [ -f /etc/xdg/mimeapps.list ] && ! grep -q '^inode/directory=' /etc/xdg/mimeapps.list 2>/dev/null; then
    sed -i '/^\[Default Applications\]/a inode/directory=org.gnome.Nautilus.desktop' /etc/xdg/mimeapps.list 2>/dev/null || :
fi

%changelog
* Fri Nov 27 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-3
- Add native blazeos-wallpaper-portal backend for org.freedesktop.impl.portal.Wallpaper, fix solarui-portals.conf FileChooser/GTK routing, and set Nautilus as default inode/directory handler
* Thu Nov 26 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-2
- Enable dynamic Omnibar sizing in Niri config and ship unprivileged BlazeHouse update notifier user timer
* Tue Nov 24 2026 Blaze Release <release@blazeos.com.tr> - 5.05.1-1
- Split SolarUI and BlazeOS configuration defaults into solarui-config RPM
