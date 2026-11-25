"""Isolated regressions; never execute privileged operations on the host."""
import ast
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
OVERLAY = ROOT / 'blazeos_custom_apps'


class AuditRegressions(unittest.TestCase):
    def test_installer_starts_with_locale_and_persists_usage_profile(self):
        configs = [
            OVERLAY / 'etc/anaconda/conf.d/99-blazeos.conf',
            OVERLAY / 'etc/anaconda/anaconda.conf',
            OVERLAY / 'etc/anaconda/profile.d/fedora-kde.conf',
            OVERLAY / 'etc/anaconda/profile.d/fedora-workstation.conf',
        ]
        for config in configs:
            self.assertNotIn('    anaconda-screen-language', config.read_text())
        webui = (OVERLAY / 'usr/share/cockpit/anaconda-webui/index.js').read_text()
        self.assertIn('var BZReview=', webui)
        self.assertIn('BZI18N=', webui)
        for language in ('tr', 'en', 'de', 'es', 'ru'):
            self.assertIn(f'{language}:{{title:', webui)
        finalizer = (OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py').read_text()
        self.assertIn('etc/blazeos-usage-profile', finalizer)

    def test_end4_has_brand_assets_and_runtime_payloads(self):
        vendor = ROOT / 'solarui/vendor/end4-pC'
        self.assertTrue((vendor / 'assets/icons/blazeos-symbolic.svg').is_file())
        self.assertIn('case "blazeos":', (vendor / 'services/SystemInfo.qml').read_text())
        self.assertIn('/usr/share/blazeos/wallpapers/galaxy.png', (vendor / 'services/FirstRunExperience.qml').read_text())
        thumbnail = (vendor / 'modules/common/widgets/ThumbnailImage.qml').read_text()
        self.assertIn('source = sourcePath', thumbnail)
        build = (ROOT / 'build_f45_final.sh').read_text()
        for package in ('matugen', 'grim', 'slurp', 'swappy', 'cava', 'brightnessctl', 'playerctl'):
            self.assertIn(package, build)

    def test_niri_publishes_wayland_environment_for_portals(self):
        config = (OVERLAY / 'etc/solarui/niri.kdl').read_text()
        skel_config = (OVERLAY / 'etc/skel/.config/solarui/niri.kdl').read_text()
        helper = (OVERLAY / 'usr/libexec/blazeos-wayland-session-ready').read_text()
        self.assertIn('spawn-at-startup "/usr/libexec/blazeos-wayland-session-ready"', config)
        self.assertIn('spawn-at-startup "/usr/libexec/blazeos-wayland-session-ready"', skel_config)
        self.assertIn('import-environment WAYLAND_DISPLAY NIRI_SOCKET', helper)
        self.assertIn('dbus-update-activation-environment --systemd WAYLAND_DISPLAY NIRI_SOCKET', helper)
        self.assertIn('stop xdg-desktop-portal.service xdg-desktop-portal-gnome.service xdg-desktop-portal-gtk.service', helper)
        self.assertNotIn('restart xdg-desktop-portal.service', helper)

    def test_installer_choices_run_before_oobe_and_display_manager(self):
        service = (OVERLAY / 'etc/systemd/system/blazeos-apply-install-choices.service').read_text()
        helper = (OVERLAY / 'usr/bin/blazeos-apply-install-choices').read_text()
        provision_service = (OVERLAY / 'etc/systemd/system/blazeos-desktop-provision.service').read_text()
        provision = (OVERLAY / 'usr/bin/blazeos-desktop-provision').read_text()
        self.assertIn('Before=display-manager.service sddm.service blazeos-postinstall.service', service)
        self.assertNotIn('environment install', helper)
        self.assertIn('desktop-provision-pending', helper)
        self.assertIn('blazeos-apply-install-choices.service', provision_service)
        self.assertIn('After=network-online.target local-fs.target display-manager.service', provision_service)
        self.assertNotIn('Before=display-manager.service', provision_service)
        self.assertIn('kde:minimal)', provision)
        self.assertIn('plasma-systemsettings', provision)
        self.assertNotIn('plasma-workspace-wayland', provision)
        self.assertNotIn('kwin-wayland', provision)
        self.assertIn('kde:full)', provision)
        self.assertIn('@kde-desktop-environment', provision)
        self.assertIn('gnome:full)', provision)
        self.assertIn('@workstation-product-environment', provision)
        self.assertIn('gnome-session-wayland-session', provision)
        self.assertIn('upgrade_required gsettings-desktop-schemas gnome-desktop3 gnome-desktop4', provision)
        self.assertIn('XSession=', provision)
        self.assertIn('/var/lib/sddm/state.conf', provision)
        self.assertIn('@sway-desktop-environment', provision)
        self.assertIn('/etc/blazeos-selected-session', helper)
        self.assertIn('DONE="$STATE_DIR/install-choices-applied"', helper)

    def test_installer_exposes_profile_and_safe_solarui_removal(self):
        webui = (OVERLAY / 'usr/share/cockpit/anaconda-webui/index.js').read_text()
        provision = (OVERLAY / 'usr/bin/blazeos-desktop-provision').read_text()
        remover = (OVERLAY / 'usr/bin/blazeos-remove-solarui').read_text()
        self.assertIn('Yalnızca masaüstü ortamı', webui)
        self.assertIn('Tüm masaüstü uygulamaları', webui)
        self.assertIn('SolarUI kaldırılsın mı?', webui)
        self.assertIn('/tmp/anaconda_desktop_profile', webui)
        self.assertIn('/tmp/anaconda_remove_solarui', webui)
        self.assertIn('/tmp/anaconda_usage_profile', webui)
        for profile in ('light', 'daily', 'office', 'gaming', 'standard', 'developer'):
            self.assertIn(f'"{profile}"', webui)
        self.assertNotIn('{id:"hyprland",name:"Hyprland"', webui)
        self.assertIn('/usr/bin/blazeos-remove-solarui', provision)
        self.assertIn('Refusing to remove SolarUI without a validated replacement session.', remover)
        self.assertNotIn('/usr/bin/blaze-recovery', remover)

    def test_oobe_uses_anaconda_selected_session(self):
        source = (OVERLAY / 'usr/bin/blaze-setup').read_text()
        self.assertIn('/etc/blazeos-selected-session', source)
        self.assertIn('Session={selected_desktop}', source)
        self.assertIn('Session={selected_session}', source)

    def test_live_boot_exits_before_account_or_gdm_mutations(self):
        source = (OVERLAY / 'usr/bin/blazeos-postinstall').read_text()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / 'cmdline').write_text('quiet root=live:CDLABEL=Blaze-SE-5 rd.live.image\n')
            # Only the read-only live-boot input is replaced. A successful early
            # exit must happen before the first log redirect or system mutation.
            source = source.split('LOG="/var/log/blazeos-postinstall.log"', 1)[0]
            source += '\necho "UNSAFE_SETUP_REACHED"; exit 99\n'
            source = source.replace('/proc/cmdline', str(root / 'cmdline'))
            result = subprocess.run(['bash', '-c', source], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('installed-system setup skipped', result.stdout)

    def test_missing_nvidia_module_does_not_retire_nouveau(self):
        source = (OVERLAY / 'usr/bin/blazeos-nvidia-setup').read_text()
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            bindir = root / 'bin'
            bindir.mkdir()
            stubs = {
                'lspci': 'echo "VGA NVIDIA"', 'rpm': 'echo 45',
                'dnf': 'exit 0', 'seq': 'echo 1', 'sleep': 'exit 0',
                'modinfo': 'exit 1',
                'grubby': f'echo touched >> "{root}/mutations"',
                'dracut': f'echo touched >> "{root}/mutations"',
            }
            for name, body in stubs.items():
                p = bindir / name
                p.write_text('#!/bin/sh\n' + body + '\n')
                p.chmod(0o755)
            for old, new in [('/var/lib/blazeos', str(root / 'state')),
                             ('/var/log/blazeos-nvidia-setup.log', str(root / 'log')),
                             ('/etc/modprobe.d', str(root / 'modprobe'))]:
                source = source.replace(old, new)
            result = subprocess.run(['bash', '-c', source], env={**os.environ, 'PATH': f'{bindir}:/usr/bin:/bin'}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertFalse((root / 'state/nvidia-setup-done').exists())
            self.assertFalse((root / 'mutations').exists())
            self.assertFalse((root / 'modprobe/blazeos-nouveau-blacklist.conf').exists())

    def test_control_log_is_bounded(self):
        tree = ast.parse((OVERLAY / 'usr/bin/blazeos-control').read_text())
        cls = next(n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == 'BlazeControlWindow')
        method = next(n for n in cls.body if isinstance(n, ast.FunctionDef) and n.name == 'append_log')
        ns = {}
        exec(compile(ast.Module(body=[method], type_ignores=[]), '<append_log>', 'exec'), ns)

        class Buffer:
            text = ''
            def insert(self, _, text): self.text += text
            def get_end_iter(self): return len(self.text)
            def get_start_iter(self): return 0
            def get_char_count(self): return len(self.text)
            def get_iter_at_offset(self, offset): return offset
            def delete(self, start, end): self.text = self.text[:start] + self.text[end:]

        obj = type('Window', (), {'log_buffer': Buffer()})()
        for i in range(100):
            ns['append_log'](obj, 'x' * 4096)
        ns['append_log'](obj, 'sonuç')
        self.assertEqual(len(obj.log_buffer.text), 200_000)
        self.assertTrue(obj.log_buffer.text.endswith('sonuç'))

    def test_session_sync_replaces_all_session_keys(self):
        source = (OVERLAY / 'usr/bin/blazeos-postinstall').read_text()
        block = source.split('# 4. Synchronize', 1)[1].split('# 5.', 1)[0]
        self.assertIn('/etc/blazeos-selected-session', block)
        self.assertIn('s/^Session=.*/Session=${SESSION_NAME}/', block)
        self.assertIn('s/^XSession=.*/XSession=${SESSION_NAME}/', block)
        self.assertIn('s/^SessionType=.*/SessionType=${SESSION_TYPE}/', block)

    def test_blazehouse_update_uses_fixed_offline_transaction(self):
        helper = (OVERLAY / 'usr/libexec/blazeos-update').read_text()
        wrapper = (OVERLAY / 'usr/libexec/blazeos-offline-update').read_text()
        dropin = (OVERLAY / 'etc/systemd/system/dnf5-offline-transaction.service.d/10-blazeos-ui.conf').read_text()
        theme = (OVERLAY / 'usr/share/plymouth/themes/blazeos/blazeos.script').read_text()
        self.assertIn('upgrade --offline -y', helper)
        self.assertIn('dnf5 -y offline reboot', helper)
        self.assertIn('dnf5 -y offline clean', helper)
        self.assertNotIn('eval ', helper)
        self.assertNotIn('sh -c', helper)
        self.assertIn('/usr/bin/dnf5 offline _execute', wrapper)
        self.assertIn('dnf_status=${PIPESTATUS[0]}', wrapper)
        self.assertIn('ExecStart=/usr/libexec/blazeos-offline-update', dropin)
        self.assertIn('Plymouth.SetSystemUpdateFunction', theme)
        self.assertIn('Evren güncelleniyor, daha da genişliyor…', theme)
        self.assertIn('Lütfen bilgisayarınızı kapatmayın', theme)
        self.assertIn('update_progress + "%"', theme)

    def test_graphical_polkit_agent_is_pinned_and_started_after_wayland(self):
        helper = (OVERLAY / 'usr/libexec/blazeos-wayland-session-ready').read_text()
        self.assertIn('/usr/libexec/polkit-mate-authentication-agent-1', helper)
        expected = {
            'mate-polkit-1.28.1-9.fc45.x86_64.rpm': '6e16a21f6c0c755a106a009fd84e5388709a9f02f8ea5fb7d424f6a558ddde28',
            'libappindicator-gtk3-12.10.1-11.fc45.x86_64.rpm': '7527f45d311ddadfe23c70567716104d517d55fc61a1240fada633a43564a8a7',
            'libdbusmenu-16.04.0-32.fc45.x86_64.rpm': '53a5856835e3da24812420b2849632a291a6b102362e5aa91f750af75d96a382',
            'libdbusmenu-gtk3-16.04.0-32.fc45.x86_64.rpm': 'fcaf227fbc3c212f1c1b203d504a6fe585eb65740d16f25a0c4087f67b0d0eba',
        }
        for name, digest in expected.items():
            payload = (ROOT / 'packages' / name).read_bytes()
            self.assertEqual(hashlib.sha256(payload).hexdigest(), digest)

    def test_style_center_is_only_offered_after_gnome_or_kde_selection(self):
        provision = (OVERLAY / 'usr/bin/blazeos-desktop-provision').read_text()
        launcher = (OVERLAY / 'usr/libexec/blaze-style-center-first-run').read_text()
        autostart = (OVERLAY / 'etc/xdg/autostart/blaze-style-center-first-run.desktop').read_text()
        source = (OVERLAY / 'usr/bin/blaze-style-center').read_text()
        compile(source, 'blaze-style-center', 'exec')
        self.assertIn('kde|gnome) touch "$STATE_DIR/style-center-pending"', provision)
        self.assertNotIn('solarui) touch "$STATE_DIR/style-center-pending"', provision)
        self.assertIn('[ -f /etc/blazeos-setup-completed ]', launcher)
        self.assertIn('X-KDE-autostart-after=panel', autostart)
        self.assertIn('/etc/blazeos-selected-session', launcher)
        self.assertIn('flock -n', launcher)
        self.assertIn('backup_once', source)
        self.assertIn('def restore(', source)
        self.assertIn('plasma-apply-lookandfeel', source)
        self.assertIn('dash-to-dock@micxgx.gmail.com', source)

    def test_appearance_is_integrated_and_dock_uses_session_helper(self):
        control = (OVERLAY / 'usr/bin/blazeos-control').read_text()
        style = (OVERLAY / 'usr/bin/blaze-style-center').read_text()
        desktop = (OVERLAY / 'usr/share/applications/blaze-style-center.desktop').read_text()
        license_notice = OVERLAY / 'usr/share/licenses/blazeos-control/PARDUS-GNOME-GREETER-LICENSE'
        compile(control, 'blazeos-control', 'exec')
        compile(style, 'blaze-style-center', 'exec')
        self.assertIn('self.build_appearance_page()', control)
        self.assertIn('BLAZEOS_CONTROL_PAGE', control)
        self.assertIn('def apply_appearance(', control)
        self.assertIn('"set-dock", action', control)
        self.assertNotIn('dash-to-dock@vswitch.com', control)
        self.assertIn('def command_line(', style)
        self.assertIn('NoDisplay=true', desktop)
        launcher = (OVERLAY / 'usr/libexec/blaze-style-center-first-run').read_text()
        self.assertIn('exec /usr/bin/blazeos-control', launcher)
        self.assertIn('pardus/pardus-gnome-greeter', license_notice.read_text())

    def test_core_image_trim_keeps_profiles_explicit(self):
        build = (ROOT / 'build_f45_final.sh').read_text()
        provision = (OVERLAY / 'usr/bin/blazeos-desktop-provision').read_text()
        self.assertIn('Trimming Workstation host payload', build)
        self.assertIn('libreoffice($|-)', build)
        self.assertIn('qemu-guest-agent', build)  # retained by the documented exclusion
        standard = provision.split('    standard)', 1)[1].split('    developer)', 1)[0]
        office = provision.split('    office)', 1)[1].split('    gaming)', 1)[0]
        self.assertNotIn('libreoffice', standard)
        self.assertIn('libreoffice-writer', office)
        preset = (OVERLAY / 'etc/systemd/system-preset/00-blazeos.preset').read_text()
        self.assertIn('disable NetworkManager-wait-online.service', preset)
        self.assertIn('disable avahi-daemon.service', preset)

    def test_desktop_provision_and_end4_use_fedora45_abi(self):
        provision = (OVERLAY / 'usr/bin/blazeos-desktop-provision').read_text()
        provider = (ROOT / 'solarui/crates/solar-shell/src/providers/end4_pc.rs').read_text()
        final_build = (ROOT / 'build_f45_final.sh').read_text()
        offline_build = (ROOT / 'build_f45_offline.sh').read_text()
        self.assertNotIn('--setopt=updates-testing-modular', provision)
        self.assertIn('DNF_STABLE=(--setopt=updates-testing.enabled=0)', provision)
        self.assertIn('/usr/bin/quickshell-fedora', provider)
        for build in (final_build, offline_build):
            self.assertNotIn('cp -a --no-preserve=ownership /usr/lib64/qt6/qml/Qt5Compat', build)
            self.assertIn('quickshell-fedora', build)
            self.assertIn('Qt_6.11_PRIVATE_API', build)
        expected = {
            'qt6-qt5compat-6.11.2-1.fc45.x86_64.rpm': '36677b8a807c93e12b0fb1ee9036f925bede904938ad7d7e8d9bb241b945e068',
            'quickshell-0.2.1_git20260209.dacfa9d-5.fc45.x86_64.rpm': '3f926860e9495029953a47b193428b98ccb2b339428ee18c37a4450edb0e3536',
            'jemalloc-5.3.0-14.fc44.x86_64.rpm': '3159390bdbcf46c3269fd499855e95421f92b3b6a155b68d5b641fb6cc7f7a88',
        }
        for name, digest in expected.items():
            payload = (ROOT / 'packages' / name).read_bytes()
            self.assertEqual(hashlib.sha256(payload).hexdigest(), digest)


class BerpBereSolarUISecurityAuditTests(unittest.TestCase):
    def test_berp_and_sentinel_cli_and_no_eager_canary(self):
        import json
        recovery_bin = OVERLAY / 'usr/bin/blaze-recovery'
        sentinel_bin = OVERLAY / 'usr/bin/blaze-sentinel'
        self.assertTrue(recovery_bin.is_file())
        self.assertTrue(sentinel_bin.is_file())

        for flag in ('--help', '--version', '--status', '--list-snapshots', '--verify'):
            res = subprocess.run([str(recovery_bin), flag], capture_output=True, text=True, timeout=10)
            self.assertEqual(res.returncode, 0, f'blaze-recovery {flag} failed: {res.stderr}')

        # Ensure /var/tmp/.blaze_canary is never created by status or --help
        var_tmp_canary = Path('/var/tmp/.blaze_canary')
        if var_tmp_canary.exists():
            try:
                var_tmp_canary.unlink()
            except OSError:
                pass

        for args in (['--help'], ['--version'], ['status'], ['status', '--json'], ['policy', 'list'], ['verify', '--json']):
            res = subprocess.run([str(sentinel_bin), *args], capture_output=True, text=True, timeout=10)
            self.assertEqual(res.returncode, 0, f'blaze-sentinel {args} failed: {res.stderr}')
            if '--json' in args and 'status' in args:
                data = json.loads(res.stdout)
                self.assertEqual(data.get('total'), 7)
                self.assertEqual(len(data.get('controls', [])), 7)

        self.assertFalse(var_tmp_canary.exists(), '/var/tmp/.blaze_canary must not be created!')

    def test_bere_grub_and_snapshot_hardening(self):
        # 11_berp must not exist in work_f45/rootfs
        self.assertFalse((ROOT / 'work_f45/rootfs/etc/grub.d/11_berp').exists())
        grub_99 = (OVERLAY / 'etc/grub.d/99_berp').read_text()
        self.assertIn('systemd.mask=sddm.service', grub_99)
        self.assertIn('systemd.mask=display-manager.service', grub_99)

        bere_gui = (OVERLAY / 'usr/bin/berp-recovery-gui').read_text()
        compile(bere_gui, 'berp-recovery-gui', 'exec')
        self.assertIn('strip_btrfs_source', bere_gui)
        self.assertIn('tempfile.mkdtemp', bere_gui)
        self.assertNotIn('?token={API_TOKEN}', bere_gui)
        self.assertNotIn('chmod -R 777', bere_gui)
        self.assertIn('userdel', bere_gui)
        self.assertIn('99-blaze-setup', bere_gui)
        self.assertIn('gen_grub_cfgstub', bere_gui)
        self.assertIn('efibootmgr', bere_gui)
        self.assertIn('resolve_target_context', bere_gui)
        self.assertIn('ensure_target_initramfs_has_death_screen', bere_gui)
        self.assertNotIn('recovery-initramfs.img', bere_gui)
        self.assertNotIn('"xorriso"', bere_gui)
        for endpoint in (
            '"delete_snapshot"',
            '"disk_mklabel"',
            '"part_create"',
            '"part_format"',
            '"part_delete"',
            '"part_fsck"',
            '"switch-to-limine"',
        ):
            self.assertIn(endpoint, bere_gui)

        bere_html = (OVERLAY / 'usr/share/berp-recovery/index.html').read_text()
        bere_js = (OVERLAY / 'usr/share/berp-recovery/app.js').read_text()
        for html_id in ('btn-disk-mklabel', 'btn-part-create', 'btn-repair-limine', 'btn-boot-status'):
            self.assertIn(html_id, bere_html)
            self.assertIn(html_id, bere_js)

        death_screen = OVERLAY / 'usr/bin/blaze-death-screen'
        self.assertTrue(death_screen.is_file())
        death_text = death_screen.read_text()
        self.assertTrue(death_text.startswith('#!/usr/bin/sh'))
        self.assertIn('CRITICAL SYSTEM FAULT / EMERGENCY RECOVERY', death_text)
        res_sh = subprocess.run(['sh', '-n', str(death_screen)], capture_output=True, text=True, timeout=10)
        self.assertEqual(res_sh.returncode, 0, res_sh.stderr)

        dracut_mod = OVERLAY / 'usr/lib/dracut/modules.d/99blaze-bere/module-setup.sh'
        dracut_conf = OVERLAY / 'etc/dracut.conf.d/99-blaze-bere.conf'
        self.assertTrue(dracut_mod.is_file())
        self.assertTrue(dracut_conf.is_file())
        self.assertIn('blaze-bere', dracut_conf.read_text())

        snap_script = (OVERLAY / 'usr/bin/blaze-snapshot').read_text()
        self.assertIn("sed 's/\\[.*//'", snap_script)
        self.assertIn('ro false', snap_script)
        self.assertIn('dnf-pre', snap_script)

    def test_solarui_shells_omnibar_and_niri_validation(self):
        # Validate all niri.kdl files
        niri_files = [
            ROOT / 'solarui/data/niri.kdl',
            OVERLAY / 'etc/solarui/niri.kdl',
            OVERLAY / 'usr/share/solarui/niri.kdl',
            OVERLAY / 'etc/skel/.config/solarui/niri.kdl',
        ]
        niri_bin = str(OVERLAY / 'usr/bin/niri') if (OVERLAY / 'usr/bin/niri').is_file() else 'niri'
        for nf in niri_files:
            self.assertTrue(nf.is_file(), f'Missing {nf}')
            res = subprocess.run([niri_bin, 'validate', '-c', str(nf)], capture_output=True, text=True, timeout=10)
            self.assertEqual(res.returncode, 0, f'niri validate failed for {nf}: {res.stderr}')

        # Verify SolarEnginePage.qml consistency and zero 'hybrid' references
        qml_paths = [
            ROOT / 'solarui/vendor/caelestia-shell/modules/nexus/pages/solarui/SolarEnginePage.qml',
            OVERLAY / 'etc/skel/.config/quickshell/caelestia/modules/nexus/pages/solarui/SolarEnginePage.qml',
            OVERLAY / 'etc/xdg/quickshell/caelestia/modules/nexus/pages/solarui/SolarEnginePage.qml',
        ]
        qml_contents = [p.read_text() for p in qml_paths]
        self.assertEqual(qml_contents[0], qml_contents[1])
        self.assertEqual(qml_contents[0], qml_contents[2])
        self.assertIn('"end4-pc"', qml_contents[0])
        self.assertNotIn('"hybrid"', qml_contents[0])

        # Verify icons and fastfetch logos exist
        self.assertTrue((OVERLAY / 'usr/share/solarui/icons/blaze-logo-256.png').is_file())
        self.assertTrue((OVERLAY / 'usr/share/solarui/icons/blaze-logo-wide.png').is_file())
        self.assertTrue((OVERLAY / 'usr/share/solarui/icons/living-flow/nav-shell.png').is_file())
        self.assertTrue((OVERLAY / 'usr/share/blazeos/ascii-logo.txt').is_file())
        self.assertTrue((OVERLAY / 'usr/share/blazeos/fastfetch/logo.ans').is_file())

        # Test solar-shell omnibar CLI status/disable/enable in isolated HOME
        solar_shell = OVERLAY / 'usr/bin/solar-shell'
        with tempfile.TemporaryDirectory() as tmp:
            env = {**os.environ, 'HOME': tmp, 'XDG_RUNTIME_DIR': tmp}
            r1 = subprocess.run([str(solar_shell), 'omnibar', 'disable'], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(r1.returncode, 0)
            r2 = subprocess.run([str(solar_shell), 'omnibar', 'status'], env=env, capture_output=True, text=True, timeout=10)
            self.assertIn('enabled=false', r2.stdout)
            r3 = subprocess.run([str(solar_shell), 'omnibar', 'enable'], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(r3.returncode, 0)
            r4 = subprocess.run([str(solar_shell), 'omnibar', 'status'], env=env, capture_output=True, text=True, timeout=10)
            self.assertIn('enabled=true', r4.stdout)

    def test_oobe_multide_firehub_and_continuum_hardening(self):
        setup_py = (OVERLAY / 'usr/bin/blaze-setup').read_text()
        compile(setup_py, 'blaze-setup', 'exec')
        self.assertIn('allow_reuse_address = True', setup_py)
        self.assertIn('tempfile.mkdtemp', setup_py)

        setup_js = (OVERLAY / 'usr/share/blaze-setup/app.js').read_text()
        self.assertIn('let desktopStarting = false', setup_js)

        plymouth_svc = (OVERLAY / 'etc/systemd/system/blazeos-plymouth-failsafe.service').read_text()
        self.assertNotIn('Before=display-manager.service', plymouth_svc)

        anaconda_conf = (OVERLAY / 'etc/anaconda/conf.d/99-blazeos.conf').read_text()
        self.assertIn('efi_dir = fedora', anaconda_conf)

        self.assertTrue(os.access(OVERLAY / 'usr/bin/blaze-system-monitor', os.X_OK))
        self.assertTrue(os.access(OVERLAY / 'usr/bin/solar-settings', os.X_OK))
        self.assertTrue(os.access(OVERLAY / 'usr/bin/blaze-house', os.X_OK))
        self.assertTrue((OVERLAY / 'opt/FireHub/resources/app.asar').is_file())

        continuum_bin = OVERLAY / 'usr/bin/blaze-continuum'
        res = subprocess.run([str(continuum_bin), 'check'], capture_output=True, text=True, timeout=10)
        self.assertEqual(res.returncode, 0, res.stderr)

    def test_welcome_firehub_license_and_oobe_autologin_integrity(self):
        # 1. Welcome GUI integrity: no wide logo and no 'Sistem Hazır' badge
        welcome_rs = (ROOT / 'solarui/crates/solar-shell/src/welcome_gui.rs').read_text()
        self.assertNotIn('LOGO_WIDE_PNG', welcome_rs)
        self.assertNotIn('FluidCloud::new("Sistem Hazır")', welcome_rs)
        self.assertIn('SolarUI Masaüstü Rehberi', welcome_rs)

        # 2. FireHub license integrity: GPL-3.0 present, GitHub repo linked, no proprietary claims
        firehub_asar = (OVERLAY / 'opt/FireHub/resources/app.asar').read_bytes()
        self.assertIn(b'GNU General Public License v3.0', firehub_asar)
        self.assertIn(b'DarkMorpheus-pc/Blaze-Fire-Hub', firehub_asar)
        self.assertNotIn(b'proprietary and confidential', firehub_asar)

        # 3. OOBE and SDDM autologin integrity
        oobe_sddm = (OVERLAY / 'etc/sddm.conf.d/00-blaze-oobe.conf').read_text()
        self.assertIn('Session=blaze-oobe.desktop', oobe_sddm)
        self.assertIn('User=blaze-setup', oobe_sddm)
        self.assertIn('MinimumUid=900', oobe_sddm)
        self.assertIn('RememberLastUser=false', oobe_sddm)

        main_sddm = (OVERLAY / 'etc/sddm.conf').read_text()
        self.assertIn('HideUsers=blaze-setup,liveuser', main_sddm)
        self.assertNotIn('\nMinimumUid=1000', main_sddm)

        sddm_pam = (OVERLAY / 'etc/pam.d/sddm-autologin').read_text()
        self.assertIn('user in blaze-setup:liveuser', sddm_pam)
        self.assertIn('pam_selinux.so close', sddm_pam)
        self.assertIn('pam_selinux.so open', sddm_pam)
        self.assertNotIn('pam_oo7.so', sddm_pam)

        # Verify SDDM wayland-session splits multi-arg strings (exec $@, NOT exec "$@") and OOBE wrapper exists
        ws_script = OVERLAY / 'etc/sddm/wayland-session'
        ws_txt = ws_script.read_text()
        self.assertNotIn('exec "$@"' , ws_txt)
        self.assertIn('exec $@', ws_txt)
        oobe_wrapper = OVERLAY / 'usr/libexec/blaze-oobe-session'
        self.assertTrue(os.access(oobe_wrapper, os.X_OK), 'Missing executable /usr/libexec/blaze-oobe-session')
        oobe_desktop = (OVERLAY / 'usr/share/wayland-sessions/blaze-oobe.desktop').read_text()
        self.assertIn('Exec=/usr/libexec/blaze-oobe-session', oobe_desktop)
        niri_setup_kdl = (OVERLAY / 'usr/share/blaze-setup/niri-setup.kdl').read_text()
        self.assertIn('layout "tr,tr,us,de"', niri_setup_kdl)
        with tempfile.TemporaryDirectory() as tmp:
            split_res = subprocess.run(
                ['sh', str(ws_script), 'echo BLAZE_SDDM_SPLIT_OK --flag'],
                env={'HOME': tmp, 'PATH': os.environ.get('PATH', '/usr/bin:/bin'), 'SHELL': '/bin/sh'},
                cwd=tmp,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(split_res.returncode, 0, split_res.stderr)
            self.assertIn('BLAZE_SDDM_SPLIT_OK --flag', split_res.stdout)

        postinstall_svc = (OVERLAY / 'etc/systemd/system/blazeos-postinstall.service').read_text()
        self.assertIn('Before=display-manager.service gdm.service sddm.service', postinstall_svc)
        self.assertIn('After=local-fs.target', postinstall_svc)
        self.assertNotIn('After=NetworkManager.service', postinstall_svc)

        postinstall_sh = (OVERLAY / 'usr/bin/blazeos-postinstall').read_text()
        self.assertIn('blaze-setup:!*:20726::::::', postinstall_sh)
        self.assertIn('blaze-setup:!*:'.strip(), postinstall_sh)
        self.assertNotIn('blaze-setup::', postinstall_sh)
        self.assertIn('/var/lib/sddm/state.conf', postinstall_sh)

        inst_py = (OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py').read_text()
        self.assertIn('blazeos-apply-install-choices.service', inst_py)
        self.assertIn('etc/sddm.conf.d/autologin.conf', inst_py)
        self.assertIn('_ensure_oobe_account_integrity', inst_py)
        self.assertIn('_provision_bere_vault', inst_py)

    def test_bere_immutable_vault_and_usb_iso_manager_integrity(self):
        # 1. Physical BLAZE_REC GPT PartSpec (unencrypted, outside Btrfs/LVM)
        auto_utils = (
            OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/storage/partitioning/automatic/utils.py'
        ).read_text()
        self.assertIn('mountpoint="/boot/bere-vault"', auto_utils)
        self.assertIn('Size("2560MiB")', auto_utils)
        self.assertIn('lv=False', auto_utils)
        self.assertIn('btr=False', auto_utils)
        self.assertIn('encrypted=False', auto_utils)

        # 2. Udev desktop hiding rule + vault provisioner GPT RequiredPartition/ReadOnly lock
        udev_rule = (OVERLAY / 'etc/udev/rules.d/99-blaze-recovery-vault.rules').read_text()
        self.assertIn('ENV{ID_FS_LABEL}=="BLAZE_REC"', udev_rule)
        self.assertIn('ENV{UDISKS_IGNORE}="1"', udev_rule)

        prov_sh = (OVERLAY / 'usr/libexec/blazeos-provision-bere-vault').read_text()
        self.assertIn('e2label "$VAULT_DEV" BLAZE_REC', prov_sh)
        self.assertIn('RequiredPartition,60', prov_sh)

        # 3. Bootloaders & Crash Screen kexec into BLAZE_REC
        grub_berp = (OVERLAY / 'etc/grub.d/99_berp').read_text()
        self.assertIn('blkid -L BLAZE_REC', grub_berp)
        self.assertIn('vmlinuz-bere', grub_berp)

        death_sh = (OVERLAY / 'usr/bin/blaze-death-screen').read_text()
        self.assertIn('BLAZE_REC', death_sh)
        self.assertIn('kexec -l /run/bere_vault/boot/vmlinuz-bere', death_sh)

        # 4. BERE GUI vault lock + USB Explorer + ISO Pull + Multi-Distro + rd.live.ram + kexec
        bere_py = (OVERLAY / 'usr/bin/berp-recovery-gui').read_text()
        for sym in (
            'is_protected_recovery_device',
            'endpoint == "usb_devices"',
            'endpoint == "usb_browse"',
            'endpoint == "iso_targets"',
            'endpoint == "iso_pull"',
            'endpoint == "iso_pull_status"',
            'endpoint == "iso_delete"',
            'endpoint == "iso_inspect"',
            'rd.live.ram=1',
            'copytoram=y',
            '"engine_used": "kexec"',
        ):
            self.assertIn(sym, bere_py)

        bere_html = (OVERLAY / 'usr/share/berp-recovery/index.html').read_text()
        self.assertIn('id="usb-device-list"', bere_html)
        self.assertIn('id="btn-pull-iso"', bere_html)
        self.assertIn('id="iso-copy-to-ram"', bere_html)
        self.assertIn('id="iso-boot-engine"', bere_html)

    def test_we10x_shell_assets_oobe_and_zero_sddm_integrity(self):
        # 1. SolarEnginePage.qml 3-copy parity includes we10x
        qml_txt = (ROOT / 'solarui/vendor/caelestia-shell/modules/nexus/pages/solarui/SolarEnginePage.qml').read_text()
        self.assertIn('"we10x"', qml_txt)

        # 2. All 5 We10XOS wallpapers, We10X icon theme, and Quickshell QML modules exist with ZERO sddm content
        for variant in ('dark', 'light', 'black', 'green', 'purple'):
            wp = OVERLAY / f'usr/share/backgrounds/blazeos/we10x/We10XOS-{variant}.png'
            self.assertTrue(wp.is_file(), f'Missing We10X wallpaper: {wp}')

        self.assertTrue((OVERLAY / 'usr/share/icons/We10X/index.theme').is_file())
        for qml_mod in (
            'shell.qml',
            'UPSTREAM.md',
            'LICENSE',
            'modules/Taskbar.qml',
            'modules/StartMenu.qml',
            'modules/ActionCenter.qml',
            'modules/CalendarFlyout.qml',
            'modules/DesktopIcons.qml',
            'modules/AltTabSwitcher.qml',
            'modules/AeroSnapOverlay.qml',
        ):
            self.assertTrue((ROOT / 'solarui/vendor/we10x-shell' / qml_mod).is_file(), f'Missing {qml_mod}')
            self.assertTrue((OVERLAY / 'usr/share/quickshell/we10x' / qml_mod).is_file(), f'Missing overlay {qml_mod}')

        for tree in (
            ROOT / 'solarui/vendor/we10x-shell',
            OVERLAY / 'usr/share/quickshell/we10x',
            OVERLAY / 'usr/share/icons/We10X',
            OVERLAY / 'usr/share/backgrounds/blazeos/we10x',
        ):
            for child in tree.rglob('*'):
                self.assertNotIn('sddm', child.name.lower(), f'Forbidden sddm asset in We10X tree: {child}')

        # 3. SOURCES.md documents GPL-3.0 We10XOS-kde upstream
        sources_md = (ROOT / 'SOURCES.md').read_text()
        self.assertIn('We10XOS-kde', sources_md)
        self.assertIn('GPL-3.0', sources_md)

        # 4. OOBE 2x2 card, JS engineLabel mapping, offline/RPM build parity, and CLI provision-user roundtrip
        oobe_html = (OVERLAY / 'usr/share/blaze-setup/index.html').read_text()
        self.assertIn('data-engine="we10x"', oobe_html)
        self.assertIn("WINDOWS'TAN GEÇENLER İÇİN", oobe_html)

        oobe_js = (OVERLAY / 'usr/share/blaze-setup/app.js').read_text()
        self.assertIn('setupData.layout === "we10x" ? "We10X"', oobe_js)

        oobe_bin = (OVERLAY / 'usr/bin/blaze-setup').read_text()
        self.assertIn('"we10x-config", "provision-user"', oobe_bin)

        we10x_state_qml = (ROOT / 'solarui/vendor/we10x-shell/modules/We10xState.qml').read_text()
        self.assertIn('Quickshell.hasThemeIcon(', we10x_state_qml)
        self.assertNotIn('Quickshell.iconPath(iconName, true)', we10x_state_qml)
        for qml_shell in (
            ROOT / 'solarui/vendor/we10x-shell/shell.qml',
            OVERLAY / 'usr/share/quickshell/we10x/shell.qml',
        ):
            qml_src = qml_shell.read_text()
            self.assertNotIn('sourceComponent:', qml_src)
            self.assertIn('component: StartMenu {}', qml_src)

        offline_sh = (ROOT / 'build_f45_offline.sh').read_text()
        self.assertIn('we10x-shell', offline_sh)
        build_rpm_sh = (ROOT / 'solarui/scripts/build-rpm.sh').read_text()
        self.assertIn('/usr/share/quickshell/we10x', build_rpm_sh)

        solar_shell = OVERLAY / 'usr/bin/solar-shell'
        probe = subprocess.run([str(solar_shell), 'switch', '__invalid_probe__'], capture_output=True, text=True, timeout=10)
        self.assertNotEqual(probe.returncode, 0)
        self.assertIn('noctalia, caelestia, end4-pc, we10x', probe.stdout + probe.stderr)

        with tempfile.TemporaryDirectory() as tmp:
            tmp_home = Path(tmp) / 'newuser'
            tmp_home.mkdir()
            prov = subprocess.run(
                [str(solar_shell), 'we10x-config', 'provision-user', str(tmp_home), 'dark'],
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(prov.returncode, 0, prov.stderr)
            self.assertIn('SOLARUI-WE10X-PROFILE-ACTIVE', (tmp_home / '.config/solarui/niri.kdl').read_text())
            self.assertTrue((tmp_home / '.local/state/solarui/pre-we10x-snapshot.json').is_file())
            self.assertIn('#e81123', (tmp_home / '.config/gtk-4.0/gtk.css').read_text())
            self.assertTrue((tmp_home / '.config/niri/config.kdl').is_symlink())

    def test_wallpaper_firehub_titlebar_and_display_personalization_integrity(self):
        # 1. Default BlazeOS & We10X Niri configs both enable 30px server-titlebar for FireHub & SSD apps
        sys_niri = (OVERLAY / 'etc/solarui/niri.kdl').read_text()
        skel_niri = (OVERLAY / 'etc/skel/.config/solarui/niri.kdl').read_text()
        self.assertEqual(sys_niri, skel_niri)
        self.assertIn('server-titlebar {', sys_niri)
        self.assertIn('allow-minimize false', sys_niri)
        self.assertIn('match app-id=r#"^(FireHub|firehub)$"#', sys_niri)

        wm_override = (OVERLAY / 'usr/share/glib-2.0/schemas/99_blazeos-wm.gschema.override').read_text()
        self.assertIn("button-layout='appmenu:maximize,close'", wm_override)
        self.assertNotIn('minimize', wm_override)

        we10x_rs = (ROOT / 'solarui/crates/solar-shell/src/we10x_profile.rs').read_text()
        self.assertIn('match app-id=r#"^(FireHub|firehub)$"#', we10x_rs)
        self.assertIn('build_we10x_niri_kdl_with_options', we10x_rs)
        self.assertIn('allow-minimize true', we10x_rs)
        self.assertIn('ShellEngine::We10x', we10x_rs)
        shell_main_rs = (ROOT / 'solarui/crates/solar-shell/src/main.rs').read_text()
        self.assertIn('cfg.shell.engine != engine::ShellEngine::We10x', shell_main_rs)

        lf_rs = (ROOT / 'solarui/crates/solar-shell/src/living_flow.rs').read_text()
        self.assertIn('Button::with_label("□")', lf_rs)

        # 2. SolarUI Settings Display Resolution/Hz + Wallpaper + We10X Red/Custom Hex Personalization
        settings_rs = (ROOT / 'solarui/crates/solar-shell/src/settings_gui.rs').read_text()
        for sym in (
            'build_personalize_tab',
            'build_we10x_customization_card',
            'apply_per_output_mode_dynamically',
            'rewrite_niri_kdl_output_mode',
            'ColorDialogButton',
            '"#e81123"',
            'Yenileme Hızı (Hz):',
        ):
            self.assertIn(sym, settings_rs)

        # 3. CLI round-trip for wallpaper list & We10X custom accent color
        solar_shell = OVERLAY / 'usr/bin/solar-shell'
        wp_list = subprocess.run([str(solar_shell), 'wallpaper', 'list'], capture_output=True, text=True, timeout=10)
        self.assertEqual(wp_list.returncode, 0, wp_list.stderr)

        with tempfile.TemporaryDirectory() as tmp:
            env = {**os.environ, 'HOME': tmp}
            set_acc = subprocess.run(
                [str(solar_shell), 'we10x-config', 'set-accent', '#E81123'],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(set_acc.returncode, 0, set_acc.stderr)
            set_col = subprocess.run(
                [str(solar_shell), 'we10x-config', 'set-colorize', 'true'],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(set_col.returncode, 0, set_col.stderr)
            get_cfg = subprocess.run(
                [str(solar_shell), 'we10x-config', 'get'],
                env=env,
                capture_output=True,
                text=True,
                timeout=10,
            )
            self.assertEqual(get_cfg.returncode, 0, get_cfg.stderr)
            import json as _json
            parsed = _json.loads(get_cfg.stdout)
            self.assertEqual(parsed['accent_color'], '#e81123')
            self.assertTrue(parsed['colorize_surfaces'])

    def test_blazehouse_live_progress_and_offline_status(self):
        bh_root = Path(os.environ.get('BLAZE_HOUSE_SRC') or (ROOT.parent / 'blaze-house/modern-ui'))
        if bh_root.is_dir():
            actions_rs = (bh_root / 'src-tauri/src/commands/system_actions.rs').read_text()
            lib_rs = (bh_root / 'src-tauri/src/lib.rs').read_text()
            update_view = (bh_root / 'src/views/SystemUpdateView.tsx').read_text()
            bridge_ts = (bh_root / 'src/services/tauriBridge.ts').read_text()
            self.assertIn('offline-update-progress', actions_rs)
            self.assertIn('get_offline_update_status', actions_rs)
            self.assertIn('get_offline_update_status', lib_rs)
            self.assertIn('role="progressbar"', update_view)
            self.assertIn('subscribeOfflineUpdateProgress', update_view)
            self.assertIn('getOfflineUpdateStatus', bridge_ts)
        helper = (OVERLAY / 'usr/libexec/blazeos-update').read_text()
        self.assertIn('/usr/bin/script', helper)
        self.assertIn('/usr/bin/dnf5 --refresh upgrade --offline -y', helper)

    def test_bere_and_berp_mandatory_auth_and_optical_media_detection(self):
        bere_gui = (OVERLAY / 'usr/bin/berp-recovery-gui').read_text()
        compile(bere_gui, 'berp-recovery-gui', 'exec')
        app_js = (OVERLAY / 'usr/share/berp-recovery/app.js').read_text()
        idx_html = (OVERLAY / 'usr/share/berp-recovery/index.html').read_text()
        death_sh = (OVERLAY / 'usr/bin/blaze-death-screen').read_text()
        berp_rs = (ROOT / 'solarui/crates/blaze-recovery/src/main.rs').read_text()

        # No optional password labels anywhere in BERE
        self.assertNotIn('İsteğe Bağlı', app_js)
        self.assertNotIn('Şifresi (İsteğe Bağlı)', idx_html)
        self.assertIn('bere-lock-overlay', idx_html)
        self.assertIn('modal-action-auth', idx_html)
        self.assertIn('auth_unlock', app_js)
        self.assertIn('DVD/ISO', app_js)
        self.assertIn('Kurulum Medyası (DVD/ISO)', bere_gui)
        self.assertIn('media://', bere_gui)
        self.assertIn('_require_action_password', bere_gui)
        self.assertIn('--verify-auth', death_sh)
        self.assertIn('--verify-auth', berp_rs)
        self.assertIn('collect_admin_hashes_at', berp_rs)

        # Functional test of offline shadow hash verification (policy A1)
        ns = {}
        from importlib.machinery import SourceFileLoader
        mod = SourceFileLoader('berp_gui_test', str(OVERLAY / 'usr/bin/berp-recovery-gui')).load_module()
        with tempfile.TemporaryDirectory() as tmp:
            etc = Path(tmp) / 'etc'
            etc.mkdir()
            import ctypes
            libcrypt = ctypes.CDLL('libcrypt.so.2')
            libcrypt.crypt.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
            libcrypt.crypt.restype = ctypes.c_char_p
            wh = libcrypt.crypt(b'AdminPass#2026', b'$6$salt1234$').decode()
            gh = libcrypt.crypt(b'GuestPass#2026', b'$6$salt5678$').decode()
            (etc / 'group').write_text('root:x:0:\nwheel:x:10:blazeuser\nusers:x:100:\n')
            (etc / 'passwd').write_text('root:x:0:0:root:/root:/bin/bash\nblazeuser:x:1000:1000:B:/home/blazeuser:/bin/bash\nguest:x:1001:100:G:/home/guest:/bin/bash\n')
            (etc / 'shadow').write_text(f'root:!:19000:0:99999:7:::\nblazeuser:{wh}:19000:0:99999:7:::\nguest:{gh}:19000:0:99999:7:::\n')

            orig_resolve = mod.resolve_target_context
            mod.resolve_target_context = lambda auto_mount=True: (tmp, False, '/dev/vda1', 'root', 'ext4', False)
            try:
                entries, req, users = mod.get_target_admin_hashes(auto_mount=False)
                self.assertTrue(req)
                self.assertEqual(users, ['blazeuser'])
                self.assertEqual(mod.verify_target_admin_password('AdminPass#2026', entries), 'blazeuser')
                self.assertEqual(mod.verify_target_admin_password('GuestPass#2026', entries), '')
                self.assertEqual(mod.verify_target_admin_password('wrong', entries), '')
                self.assertEqual(mod.verify_target_admin_password('', entries), '')
            finally:
                mod.resolve_target_context = orig_resolve

    def test_deep_identity_persistence_and_anaconda_reinstall_integrity(self):
        id_dir = OVERLAY / 'usr/share/blazeos/identity'
        for name in ('os-release', 'fedora-release', 'blazeos-release', 'system-release-cpe', 'issue', 'issue.net', 'buildstamp'):
            self.assertTrue((id_dir / name).is_file(), f'Missing canonical identity file: {name}')

        self.assertEqual((OVERLAY / 'etc/os-release').read_text(), (id_dir / 'os-release').read_text())
        self.assertEqual((OVERLAY / 'usr/lib/os-release').read_text(), (id_dir / 'os-release').read_text())
        self.assertEqual((OVERLAY / 'usr/lib/fedora-release').read_text().strip(), 'Blaze SolarEvolution release 5.05.1 (SolarEvolution)')
        self.assertIn('GRUB_DISTRIBUTOR="Blaze SolarEvolution"', (OVERLAY / 'etc/default/grub').read_text())
        self.assertEqual((OVERLAY / 'etc/dnf/vars/releasever').read_text().strip(), '45')
        self.assertIn('excludepkgs=fedora-release*', (OVERLAY / 'etc/dnf/libdnf5.conf.d/10-blaze-identity.conf').read_text())
        self.assertIn('profile_id = blazeos-solarevolution', (OVERLAY / 'etc/anaconda/profile.d/blazeos-solarevolution.conf').read_text())
        self.assertIn('Product=Blaze SolarEvolution', (OVERLAY / '.buildstamp').read_text())

        for link_rel, target in (
            ('etc/fedora-release', '../usr/lib/fedora-release'),
            ('etc/redhat-release', 'fedora-release'),
            ('etc/system-release', 'fedora-release'),
        ):
            p = OVERLAY / link_rel
            self.assertTrue(p.is_symlink(), f'{link_rel} must be a symlink')
            self.assertEqual(os.readlink(p), target)

        webui_js = (OVERLAY / 'usr/share/cockpit/anaconda-webui/index.js').read_text()
        self.assertIn('n["os-name"].v.includes("Blaze")', webui_js)
        self.assertIn('m["os-name"].v.includes("Blaze")', webui_js)
        self.assertIn('bztBrand(', webui_js)

        sync_bin = OVERLAY / 'usr/bin/blazeos-sync-identity'
        self.assertTrue(os.access(sync_bin, os.X_OK))

        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            import shutil as _shutil
            _shutil.copytree(id_dir, root / 'usr/share/blazeos/identity')
            for d in ('etc/default', 'etc/dnf/vars', 'etc/dnf/libdnf5.conf.d', 'etc/dnf/protected.d', 'etc/anaconda/profile.d', 'usr/lib', 'boot/loader/entries'):
                (root / d).mkdir(parents=True, exist_ok=True)
            (root / 'usr/lib/os-release').write_text('NAME="Fedora Linux"\nVERSION_ID=45\nPRETTY_NAME="Fedora Linux 45 (Workstation Edition)"\n')
            (root / 'usr/lib/fedora-release').write_text('Fedora release 45 (Forty Five)\n')
            (root / 'etc/default/grub').write_text('GRUB_TIMEOUT=10\nGRUB_DISTRIBUTOR="$(sed \'s, release .*$,,g\' /etc/system-release)"\nGRUB_ENABLE_BLSCFG=true\n')
            (root / 'boot/loader/entries/abc-7.3.4-300.fc45.x86_64.conf').write_text(
                'title Fedora Linux (7.3.4-300.fc45.x86_64) 45 (Workstation Edition)\n'
                'version 7.3.4-300.fc45.x86_64\n'
                'linux /vmlinuz-7.3.4-300.fc45.x86_64\n'
                'grub_class fedora\n'
            )
            (root / 'boot/loader/entries/abc-7.2.11-300.fc45.x86_64~debug.conf').write_text(
                'title Fedora Linux (7.2.11-300.fc45.x86_64) 45 (Workstation Edition) with debugging\n'
                'version 7.2.11-300.fc45.x86_64~debug\n'
                'linux /vmlinuz-7.2.11-300.fc45.x86_64\n'
                'grub_class fedora\n'
            )
            (root / 'boot/loader/entries/abc-snapshot.conf').write_text(
                'title Blaze snapshot 20261123 (kernel 7.2.11-300.fc45.x86_64)\n'
                'version 7.2.11-300.fc45.x86_64\n'
            )

            res = subprocess.run([str(sync_bin), f'--sysroot={root}'], capture_output=True, text=True, timeout=15)
            self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
            self.assertIn('PRETTY_NAME="Blaze SolarEvolution 5.05.1"', (root / 'usr/lib/os-release').read_text())
            self.assertEqual((root / 'usr/lib/fedora-release').read_text().strip(), 'Blaze SolarEvolution release 5.05.1 (SolarEvolution)')
            self.assertIn('BLAZE_BASE_ID=fedora', (root / 'usr/lib/os-release').read_text())
            self.assertIn('BLAZE_BASE_VERSION_ID=45', (root / 'usr/lib/os-release').read_text())
            self.assertNotIn('PLATFORM_ID=', (root / 'usr/lib/os-release').read_text())
            self.assertIn('GRUB_DISTRIBUTOR="Blaze SolarEvolution"', (root / 'etc/default/grub').read_text())
            self.assertEqual((root / 'etc/dnf/vars/releasever').read_text().strip(), '45')

            bls1 = (root / 'boot/loader/entries/abc-7.3.4-300.fc45.x86_64.conf').read_text()
            self.assertIn('title Blaze SolarEvolution (7.3.4-300.fc45.x86_64) 5.05.1', bls1)
            self.assertIn('grub_class blazeos', bls1)
            bls2 = (root / 'boot/loader/entries/abc-7.2.11-300.fc45.x86_64~debug.conf').read_text()
            self.assertIn('title Blaze SolarEvolution (7.2.11-300.fc45.x86_64) 5.05.1 with debugging', bls2)
            snap_bls = (root / 'boot/loader/entries/abc-snapshot.conf').read_text()
            self.assertIn('title Blaze snapshot 20261123 (kernel 7.2.11-300.fc45.x86_64)', snap_bls)

            res2 = subprocess.run([str(sync_bin), f'--sysroot={root}', '--check'], capture_output=True, text=True, timeout=15)
            self.assertEqual(res2.returncode, 0, res2.stdout + res2.stderr)
            self.assertNotIn('Identity sync completed', res2.stdout)

            # Verify major releasever transition preservation (e.g. 45 -> 46)
            chan_dir = root / 'etc/blazeos'
            chan_dir.mkdir(parents=True, exist_ok=True)
            (chan_dir / 'update-channel.conf').write_text(
                '[channel]\ndistro_family=solar-evolution\ncurrent_releasever=46\n'
            )
            res3 = subprocess.run([str(sync_bin), f'--sysroot={root}', '--force'], capture_output=True, text=True, timeout=15)
            self.assertEqual(res3.returncode, 0, res3.stdout + res3.stderr)
            self.assertEqual((root / 'etc/dnf/vars/releasever').read_text().strip(), '46')

    def test_github_channel_and_major_release_upgrade_contract(self):
        for rel in ('etc/blazeos/update-channel.conf', 'usr/share/blazeos/identity/update-channel.conf'):
            txt = (OVERLAY / rel).read_text(encoding='utf-8')
            self.assertIn('DarkMorpheus-pc/Blaze-SolarEvolution', txt)
            self.assertIn('repo/channel.json', txt)

        upd_sh = (OVERLAY / 'usr/libexec/blazeos-update').read_text(encoding='utf-8')
        self.assertIn('system-upgrade-download', upd_sh)
        self.assertIn('system-upgrade-reboot', upd_sh)
        self.assertIn('blaze-snapshot', upd_sh)
        self.assertNotIn('eval ', upd_sh)

        off_sh = (OVERLAY / 'usr/libexec/blazeos-offline-update').read_text(encoding='utf-8')
        self.assertIn('pending-system-upgrade-releasever', off_sh)
        self.assertIn('blazeos-sync-identity', off_sh)

        pub_sh = (ROOT / 'packaging/publish-repo.sh').read_text(encoding='utf-8')
        self.assertIn('channel.json', pub_sh)
        self.assertIn('upgrade_target', pub_sh)

        # Ensure Blaze-Galaxy references are completely retired from tracked manifests & docs
        for p in (
            ROOT / 'README.md',
            ROOT / 'SOURCES.md',
            ROOT / 'packaging/blaze.repo',
            OVERLAY / 'usr/share/dnf5/repos.d/blaze.repo',
            OVERLAY / 'etc/os-release',
        ):
            self.assertNotIn('Blaze-Galaxy', p.read_text(encoding='utf-8'), f'Stale Blaze-Galaxy URL in {p}')

        bh_root = Path(os.environ.get('BLAZE_HOUSE_SRC') or (ROOT.parent / 'blaze-house/modern-ui'))
        if bh_root.is_dir():
            updates_rs = (bh_root / 'src-tauri/src/commands/updates.rs').read_text(encoding='utf-8')
            actions_rs = (bh_root / 'src-tauri/src/commands/system_actions.rs').read_text(encoding='utf-8')
            lib_rs = (bh_root / 'src-tauri/src/lib.rs').read_text(encoding='utf-8')
            view_tsx = (bh_root / 'src/views/SystemUpdateView.tsx').read_text(encoding='utf-8')
            for sym in ('get_update_channel', 'Blaze-SolarEvolution', 'Blaze-Universe-'):
                self.assertIn(sym, updates_rs)
            self.assertIn('prepare_system_upgrade', actions_rs)
            self.assertIn('reboot_to_system_upgrade', actions_rs)
            self.assertIn('get_update_channel', lib_rs)
            self.assertIn('prepare_system_upgrade', lib_rs)
            self.assertIn('getUpdateChannel', view_tsx)

    def test_optional_unified_bere_vault_and_boot_wipe_immunity(self):
        webui_js = (OVERLAY / 'usr/share/cockpit/anaconda-webui/index.js').read_text(encoding='utf-8')
        for marker in ('anaconda_bere_vault_mode', 'vaultFull', 'vaultMinimal', 'vaultNone'):
            self.assertIn(marker, webui_js)

        auto_utils = (
            OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/storage/partitioning/automatic/utils.py'
        ).read_text(encoding='utf-8')
        self.assertIn('anaconda_bere_vault_mode', auto_utils)
        self.assertIn('Size("4400MiB")', auto_utils)
        self.assertIn('Size("2560MiB")', auto_utils)

        prov_sh = (OVERLAY / 'usr/libexec/blazeos-provision-bere-vault').read_text(encoding='utf-8')
        self.assertIn('/run/initramfs/live/LiveOS/squashfs.img', prov_sh)
        self.assertIn('full_os_embedded', prov_sh)
        self.assertIn('--refresh-kernel-only', prov_sh)
        self.assertIn('blockdev --setro', prov_sh)

        udev_rule = (OVERLAY / 'etc/udev/rules.d/99-blaze-recovery-vault.rules').read_text(encoding='utf-8')
        self.assertIn('blockdev --setro $devnode', udev_rule)

        cfgstub = (OVERLAY / 'usr/bin/gen_grub_cfgstub').read_text(encoding='utf-8')
        self.assertIn('BLAZE_RECOVERY_VAULT.json', cfgstub)

        inst_py = (OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py').read_text(encoding='utf-8')
        self.assertIn('EFI/BlazeRecovery', inst_py)
        self.assertIn('BlazeOS Recovery (BERE Vault)', inst_py)

        # Functional test of SHA-256 tamper detection in inspect_blaze_rec_vault
        from importlib.machinery import SourceFileLoader
        mod = SourceFileLoader('berp_gui_vault_test', str(OVERLAY / 'usr/bin/berp-recovery-gui')).load_module()
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            (t / 'LiveOS').mkdir()
            (t / 'boot').mkdir()
            (t / 'LiveOS/squashfs.img').write_bytes(b'squash-payload')
            (t / 'boot/vmlinuz-bere').write_bytes(b'vmlinuz-payload')
            (t / 'boot/initrd-bere.img').write_bytes(b'initrd-payload')
            import hashlib as _hl, json as _json
            manifest = {
                'label': 'BLAZE_REC',
                'mode': 'full',
                'full_os_embedded': True,
                'sha256': {
                    'squashfs': _hl.sha256(b'squash-payload').hexdigest(),
                    'vmlinuz': _hl.sha256(b'vmlinuz-payload').hexdigest(),
                    'initrd': _hl.sha256(b'initrd-payload').hexdigest(),
                },
            }
            (t / 'BLAZE_RECOVERY_VAULT.json').write_text(_json.dumps(manifest), encoding='utf-8')

            orig_co = mod.subprocess.check_output
            orig_exists = mod.os.path.exists
            try:
                def fake_co(cmd, *a, **kw):
                    if cmd[:2] == ['blkid', '-L']:
                        return '/dev/fakevault\n'
                    if cmd[:3] == ['blkid', '-s', 'UUID']:
                        return 'dead-beef\n'
                    if cmd[:2] == ['findmnt', '-n']:
                        return f'{t}\n'
                    return orig_co(cmd, *a, **kw)

                mod.subprocess.check_output = fake_co
                mod.os.path.exists = lambda p: True if p == '/dev/fakevault' else orig_exists(p)

                ok_info = mod.inspect_blaze_rec_vault(verify_hashes=True)
                self.assertTrue(ok_info['present'])
                self.assertTrue(ok_info['full_os_embedded'])
                self.assertTrue(ok_info['hash_verified'])
                self.assertIsNone(ok_info['tamper_error'])

                # Tamper squashfs payload -> must detect SHA-256 mismatch
                (t / 'LiveOS/squashfs.img').write_bytes(b'tampered-payload')
                bad_info = mod.inspect_blaze_rec_vault(verify_hashes=True)
                self.assertFalse(bad_info['hash_verified'])
                self.assertIsNotNone(bad_info['tamper_error'])
                self.assertIn('SHA-256', bad_info['tamper_error'])
            finally:
                mod.subprocess.check_output = orig_co
                mod.os.path.exists = orig_exists

    def test_ventoy_initrd_and_live_nvidia_compatibility(self):
        patch_sh = (ROOT / 'scripts/patch_f45_initrd.sh').read_text(encoding='utf-8')
        self.assertRegex(patch_sh, r'BASE_DISTRO=.*Fedora Linux 45')
        self.assertIn('../../../var/lib/dracut/hooks usr/lib/dracut/hooks', patch_sh)
        self.assertIn('var/lib/dracut/hooks/initqueue/timeout', patch_sh)
        self.assertIn('plymouth quit', (patch_sh.split('00-blaze-death-screen.sh')[1]))
        self.assertIn('99-blaze-ventoy-compat.rules', patch_sh)
        self.assertIn('cancel_wait_for_dev /dev/ventoy', patch_sh)
        self.assertIn('blkid -s TYPE -o value', patch_sh)
        self.assertIn('os.path.getsize(path) % 4', patch_sh)

        usb_quirks = (OVERLAY / 'usr/lib/modprobe.d/blazeos-usb-quirks.conf').read_text(encoding='utf-8')
        self.assertIn('346d:5678:u', usb_quirks)
        self.assertIn('152d:0578:u', usb_quirks)

        nvidia_svc = (OVERLAY / 'etc/systemd/system/blazeos-nvidia-firstboot.service').read_text(encoding='utf-8')
        self.assertIn('ConditionKernelCommandLine=!rd.live.image', nvidia_svc)

    def test_portable_x86_64_shells_and_gamezone_safety_focus_and_shell_restore(self):
        import re
        # 1. Verify noctalia, cliphist, Caelestia plugins & bundled libraries have zero unconditional VEX/BMI2 instructions in .init / startup and no x86-64-v3 ISA needed ELF property
        for rel in (
            'usr/bin/noctalia',
            'usr/bin/cliphist',
            'usr/bin/seatd',
            'usr/lib64/libtomlplusplus.so.3.4.0',
            'usr/lib64/libical.so.4.0.5',
            'usr/lib64/libqalculate.so.23.3.13',
            'usr/lib64/quickshell/libQt6Core.so.6.11.2',
            'usr/lib64/quickshell/libcava.so.1.0.0',
            'usr/lib64/quickshell/qml/M3Shapes/libm3shapes.so',
        ):
            bin_path = OVERLAY / rel
            self.assertTrue(bin_path.exists(), f'Missing binary {bin_path}')
            notes = subprocess.check_output(
                ['readelf', '-n', str(bin_path)],
                text=True,
                stderr=subprocess.DEVNULL,
            )
            self.assertNotIn('x86 ISA needed: x86-64-baseline, x86-64-v2, x86-64-v3', notes, f'{rel} still carries x86-64-v3 ISA needed ELF note!')
            out = subprocess.check_output(
                ['sh', '-c', f'objdump -d "{bin_path}" | head -n 1500'],
                text=True,
                stderr=subprocess.DEVNULL,
            )
            vex_hits = re.findall(r'\b(vzeroupper|vmovdqa|vmovdqu|vpxor|shrx|shlx|sarx|rorx|pdep|pext)\b', out)
            self.assertEqual(vex_hits, [], f'{rel} still contains x86-64-v3 VEX/BMI2 instructions in startup code: {vex_hits[:5]}')

        # 2. Verify quickshell wrapper & NoctaliaProvider software-GL fallback
        qs_sh = (OVERLAY / 'usr/bin/quickshell').read_text(encoding='utf-8')
        self.assertIn('QSG_RENDER_LOOP', qs_sh)
        self.assertNotIn('\nexport QSG_RENDER_LOOP=threaded\n', qs_sh)
        noct_rs = (ROOT / 'solarui/crates/solar-shell/src/providers/noctalia.rs').read_text(encoding='utf-8')
        self.assertIn('LIBGL_ALWAYS_SOFTWARE', noct_rs)
        self.assertIn('llvmpipe', noct_rs)

        # 3. Verify GameZone 18+ filter (stock OFF), dynamic rotation, browser focus, and Super+Q shell restore
        gz_live = (ROOT / 'solarui/crates/solar-shell/src/gamezone_live.rs').read_text(encoding='utf-8')
        self.assertIn('pub fn is_adult_or_nsfw_game(', gz_live)
        self.assertIn('pub fn is_steam_appdetails_adult(', gz_live)
        self.assertIn('AdultFiltered {', gz_live)
        self.assertIn('featuredcategories', gz_live)

        gz_gui = (ROOT / 'solarui/crates/solar-shell/src/gamezone_gui.rs').read_text(encoding='utf-8')
        self.assertIn('pub fn is_adult_content_allowed() -> bool', gz_gui)
        self.assertIn('pub fn set_adult_content_allowed(allow: bool)', gz_gui)
        self.assertIn('18 Yaşından Büyüküm: KAPALI (Güvenli)', gz_gui)
        self.assertIn('18 Yaşından Büyüküm: AÇIK (+18 Göster)', gz_gui)
        self.assertIn('Masaüstüne Dön', gz_gui)
        self.assertIn('REBUILDING_FOR_LANG', gz_gui)
        self.assertIn('focus_external_window_over_gamezone', gz_gui)

        # 4. Verify blaze-gamezone-browser and niri.kdl window-rule prevent snapping back to fullscreen GameZone
        gz_browser = (OVERLAY / 'usr/bin/blaze-gamezone-browser').read_text(encoding='utf-8')
        self.assertNotIn('self.maximize()', gz_browser)
        self.assertIn('focus_browser_in_niri', gz_browser)
        for niri_rel in (
            OVERLAY / 'etc/solarui/niri.kdl',
            OVERLAY / 'etc/skel/.config/solarui/niri.kdl',
            OVERLAY / 'etc/skel/.config/niri/config.kdl',
            OVERLAY / 'usr/share/solarui/niri.kdl',
            ROOT / 'solarui/data/niri.kdl',
        ):
            kdl = niri_rel.read_text(encoding='utf-8')
            self.assertIn('org\\.blazeos\\.gamezone\\.browser|blaze-gamezone-browser', kdl)
            self.assertIn('open-focused true', kdl)

    def test_gate1_transactional_grub_efi_repair(self):
        from importlib.machinery import SourceFileLoader
        mod = SourceFileLoader('berp_gui_g1', str(OVERLAY / 'usr/bin/berp-recovery-gui')).load_module()

        with tempfile.TemporaryDirectory() as tmp:
            troot = Path(tmp)
            esp = troot / 'boot/efi'
            efi_fedora = esp / 'EFI/fedora'
            efi_boot = esp / 'EFI/BOOT'
            bls = troot / 'boot/loader/entries'
            grub2 = troot / 'boot/grub2'
            for d in (efi_fedora, efi_boot, bls, grub2, troot / 'etc'):
                d.mkdir(parents=True, exist_ok=True)

            (efi_fedora / 'shimx64.efi').write_bytes(b'SHIM_ORIG')
            (efi_fedora / 'grubx64.efi').write_bytes(b'GRUB_ORIG')
            (efi_boot / 'BOOTX64.EFI').write_bytes(b'BOOTX64_ORIG')
            (troot / 'boot/vmlinuz-7.2.11-300.fc45.x86_64').write_bytes(b'VMLINUZ')
            (troot / 'boot/initramfs-7.2.11-300.fc45.x86_64.img').write_bytes(b'INITRD')
            (troot / 'etc/machine-id').write_text('abc123machid\n', encoding='utf-8')
            (efi_fedora / 'grub.cfg').write_text('ORIG_STUB\n', encoding='utf-8')
            (grub2 / 'grub.cfg').write_text('ORIG_GRUB_CFG\n', encoding='utf-8')

            # 1. Verify samefile detection skips identical source/dest cleanly without SameFileError
            tx_same = mod.FileRepairTransaction(backup_roots=[str(esp)])
            self.assertTrue(mod.is_same_target_file(str(efi_boot / 'BOOTX64.EFI'), str(efi_boot / 'BOOTX64.EFI')))
            self.assertEqual(
                tx_same.copy_file(str(efi_boot / 'BOOTX64.EFI'), str(efi_boot / 'BOOTX64.EFI')),
                'skipped_samefile',
            )
            tx_same.cleanup()

            # 2. Verify multi-file rollback restores ALL EFI + BLS + GRUB files if late stage fails
            tx_rb = mod.FileRepairTransaction(backup_roots=[str(esp), str(troot / 'boot')])
            tx_rb.write_text(str(efi_boot / 'BOOTX64.EFI'), 'MODIFIED_EFI')
            tx_rb.write_text(str(bls / 'abc123machid-7.2.11.conf'), 'NEW_BLS_ENTRY\n')
            tx_rb.write_text(str(grub2 / 'grub.cfg'), 'NEW_GRUB_CFG\n')
            self.assertEqual((efi_boot / 'BOOTX64.EFI').read_text(encoding='utf-8'), 'MODIFIED_EFI')
            self.assertTrue((bls / 'abc123machid-7.2.11.conf').exists())
            errs = tx_rb.rollback()
            self.assertEqual(errs, [])
            self.assertEqual((efi_boot / 'BOOTX64.EFI').read_bytes(), b'BOOTX64_ORIG')
            self.assertFalse((bls / 'abc123machid-7.2.11.conf').exists())
            self.assertEqual((grub2 / 'grub.cfg').read_text(encoding='utf-8'), 'ORIG_GRUB_CFG\n')

            # 3. Verify _repair_efi_binaries when source == destination path
            tx_efi = mod.FileRepairTransaction(backup_roots=[str(esp)])
            try:
                log_efi = mod._repair_efi_binaries(tx_efi, str(esp))
                self.assertIn('skipped_samefile', log_efi)
                self.assertEqual((efi_boot / 'BOOTX64.EFI').read_bytes(), b'BOOTX64_ORIG')
            finally:
                tx_efi.cleanup()

    def test_gate2_blaze_setup_locked_and_pam_autologin(self):
        checked_paths = [
            OVERLAY / 'usr/bin/blazeos-postinstall',
            OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py',
            OVERLAY / 'usr/bin/berp-recovery-gui',
            ROOT / 'build_f45_final.sh',
            ROOT / 'build_f45_offline.sh',
        ]
        for p in checked_paths:
            txt = p.read_text(encoding='utf-8')
            self.assertNotIn('blaze-setup::', txt, f'Found unlocked empty blaze-setup shadow entry in {p}')
            self.assertIn('blaze-setup:!*:20726::::::', txt, f'Missing locked blaze-setup shadow entry in {p}')

        build_final = (ROOT / 'build_f45_final.sh').read_text(encoding='utf-8')
        self.assertIn('($2 == "" || $2 == "::")', build_final)

        pam_autologin = (OVERLAY / 'etc/pam.d/sddm-autologin').read_text(encoding='utf-8')
        self.assertIn('pam_permit.so', pam_autologin)
        self.assertIn('pam_succeed_if.so user in blaze-setup:liveuser', pam_autologin)
        self.assertIn('pam_selinux.so close', pam_autologin)
        self.assertIn('pam_selinux.so open', pam_autologin)

        setup_backend = (OVERLAY / 'usr/bin/blaze-setup').read_text(encoding='utf-8')
        self.assertIn('if not password:', setup_backend)
        self.assertIn('Boş parola kullanılamaz', setup_backend)
        self.assertIn('"--no-block"', setup_backend)
        self.assertNotIn('"--on-active=2s"', setup_backend)

    def test_gate3_ownership_manifest_and_scap_compat(self):
        # 1. Verify 100% overlay ownership manifest coverage
        chk = subprocess.run(
            ['python3', str(ROOT / 'packaging/check_ownership.py')],
            capture_output=True, text=True, timeout=30,
        )
        self.assertEqual(chk.returncode, 0, chk.stdout + chk.stderr)

        # 2. Verify canonical identity contract (BLAZE_BASE_ID/VERSION_ID, no PLATFORM_ID)
        for rel in ('etc/os-release', 'usr/lib/os-release', 'usr/share/blazeos/identity/os-release'):
            osr = (OVERLAY / rel).read_text(encoding='utf-8')
            self.assertIn('ID=blazeos', osr)
            self.assertIn('ID_LIKE=fedora', osr)
            self.assertIn('BLAZE_BASE_ID=fedora', osr)
            self.assertIn('BLAZE_BASE_VERSION_ID=45', osr)
            self.assertIn('PRETTY_NAME="Blaze SolarEvolution 5.05.1"', osr)
            self.assertNotIn('PLATFORM_ID=', osr)

        # 3. Verify CI .grype.yaml exists cleanly scoped
        grype_cfg = (ROOT / '.grype.yaml').read_text(encoding='utf-8')
        self.assertIn('distro: "fedora:45"', grype_cfg)

        # 4. Verify blazeos-scap-adapt cleanly injects BlazeOS CPE/OVAL without touching rule platforms
        from importlib.machinery import SourceFileLoader
        import importlib.util, xml.etree.ElementTree as ET
        loader = SourceFileLoader('scap_adapt', str(OVERLAY / 'usr/bin/blazeos-scap-adapt'))
        spec = importlib.util.spec_from_loader('scap_adapt', loader)
        mod = importlib.util.module_from_spec(spec)
        loader.exec_module(mod)

        sample_ds = '''<?xml version="1.0" encoding="UTF-8"?>
<ds:data-stream-collection xmlns:ds="http://scap.nist.gov/schema/scap/source/1.2"
  xmlns:cpe="http://cpe.mitre.org/dictionary/2.0"
  xmlns:xccdf="http://checklists.nist.gov/xccdf/1.2"
  xmlns:oval="http://oval.mitre.org/XMLSchema/oval-definitions-5"
  xmlns:ind="http://oval.mitre.org/XMLSchema/oval-definitions-5#independent"
  xmlns:lin="http://oval.mitre.org/XMLSchema/oval-definitions-5#linux">
  <ds:component id="scap_org.open-scap_comp_ssg-fedora-cpe-dictionary.xml" timestamp="2026-01-01T00:00:00">
    <cpe:cpe-list>
      <cpe:cpe-item name="cpe:/o:fedoraproject:fedora:45"><cpe:title xml:lang="en-us">Fedora 45</cpe:title></cpe:cpe-item>
    </cpe:cpe-list>
  </ds:component>
  <ds:component id="scap_org.open-scap_comp_ssg-fedora-xccdf.xml" timestamp="2026-01-01T00:00:00">
    <xccdf:Benchmark id="xccdf_org.ssgproject.content_benchmark_FEDORA">
      <xccdf:platform idref="cpe:/o:fedoraproject:fedora:45"/>
      <xccdf:Rule id="xccdf_org.ssgproject.content_rule_example"><xccdf:platform idref="#grub2"/></xccdf:Rule>
    </xccdf:Benchmark>
  </ds:component>
  <ds:component id="scap_org.open-scap_comp_ssg-fedora-cpe-oval.xml" timestamp="2026-01-01T00:00:00">
    <oval:oval_definitions>
      <oval:definitions>
        <oval:definition id="oval:ssg-installed_OS_is_part_of_Unix_family:def:1" version="1" class="inventory"><oval:metadata><oval:title>Unix</oval:title></oval:metadata><oval:criteria/></oval:definition>
      </oval:definitions>
      <oval:tests>
        <lin:rpminfo_test id="oval:ssg-test_fedora_release_rpm:tst:1" version="1" check="all" comment="fedora-release"><lin:object object_ref="oval:ssg-object_fedora_release_rpm:obj:1"/></lin:rpminfo_test>
      </oval:tests>
      <oval:objects>
        <lin:rpminfo_object id="oval:ssg-object_fedora_release_rpm:obj:1" version="1"><lin:name>fedora-release</lin:name></lin:rpminfo_object>
      </oval:objects>
    </oval:oval_definitions>
  </ds:component>
</ds:data-stream-collection>'''
        with tempfile.TemporaryDirectory() as tmp:
            src = Path(tmp) / 'ssg-fedora-ds.xml'
            dst = Path(tmp) / 'ssg-blazeos-fedora45-ds.xml'
            src.write_text(sample_ds, encoding='utf-8')
            mod.adapt_fedora_datastream(src, dst)
            out_txt = dst.read_text(encoding='utf-8')
            self.assertIn('cpe:/o:blazeos:solarevolution:5.05.1', out_txt)
            self.assertIn('cpe:/o:fedoraproject:fedora:45', out_txt)
            self.assertIn('oval:blazeos-installed_OS_is_solarevolution_fedora45:def:1', out_txt)
            self.assertIn('^BLAZE_BASE_ID=fedora$', out_txt)
            self.assertIn('^BLAZE_BASE_VERSION_ID=45$', out_txt)
            self.assertIn('#grub2', out_txt)


    def test_gate4_audit_log_gshadow_sync_and_initrd_blockdev(self):
        import importlib.machinery
        import importlib.util
        import json as _json
        import stat as _stat
        import tempfile
        from unittest import mock

        gui_path = str(OVERLAY / 'usr/bin/berp-recovery-gui')
        loader = importlib.machinery.SourceFileLoader('berp_recovery_gui_gate4', gui_path)
        spec = importlib.util.spec_from_loader('berp_recovery_gui_gate4', loader)
        mod = importlib.util.module_from_spec(spec)
        loader.exec_module(mod)

        # 1. Persistent structured audit log with secret redaction + dual-target write + 0600 perms
        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            rt_log = t / 'runtime/var/log/bere-recovery.log'
            target_root = t / 'target_root'
            (target_root / 'var/log').mkdir(parents=True)
            with mock.patch.dict(
                os.environ,
                {
                    'BLAZE_BERE_AUDIT_LOG': str(rt_log),
                    'BLAZE_BERE_TARGET_ROOT': str(target_root),
                },
                clear=False,
            ):
                mod.append_recovery_audit_event(
                    action='auth_unlock',
                    outcome='denied',
                    target='session',
                    details={'password': 'UltraSecretPassword!', 'reason': 'bad_pw'},
                    actor='unauthenticated',
                    client_ip='127.0.0.1',
                )
                mod.append_recovery_audit_event(
                    action='boot_repair',
                    outcome='ok',
                    target='grub',
                    details={'token': 'SecretTok99', 'command': 'grub2-mkconfig --password=LeakMe'},
                    actor='adminuser',
                    client_ip='127.0.0.1',
                )
                events, paths = mod.read_recovery_audit_events(limit=10)
                self.assertEqual(len(events), 2)
                self.assertEqual([e['action'] for e in events], ['auth_unlock', 'boot_repair'])
                self.assertEqual(events[0]['outcome'], 'denied')
                self.assertEqual(events[1]['outcome'], 'ok')

                # Verify locked API call records an audit event via handle_api
                handler = mod.RecoveryAPIHandler.__new__(mod.RecoveryAPIHandler)
                handler.client_address = ('127.0.0.1', 54321)
                with (
                    mock.patch.object(mod, 'get_target_admin_hashes', return_value=([], True, ['adminuser'])),
                    mock.patch.dict(mod._AUTH_STATE, {'unlocked': False, 'unlocked_user': ''}),
                ):
                    res = handler.handle_api('boot_repair', {'target': 'grub', 'password': 'NeverLogThisPassword'})
                    self.assertEqual(res['status'], 'locked')

                with (
                    mock.patch.object(mod, 'get_target_admin_hashes', return_value=([], False, ['adminuser'])),
                    mock.patch.object(mod, 'resolve_target_context', return_value=(str(target_root), True, '/dev/vda4', '@', 'btrfs', False)),
                ):
                    res_list = handler.handle_api('audit_log', {'limit': 10})
                    self.assertEqual(res_list['status'], 'ok')
                    self.assertEqual(res_list['count'], 3)
                    self.assertEqual(res_list['events'][-1]['action'], 'boot_repair')
                    self.assertEqual(res_list['events'][-1]['outcome'], 'locked')

            mirror_log = target_root / 'var/log/bere-recovery.log'
            for log_p in (rt_log, mirror_log):
                self.assertTrue(log_p.is_file(), f'Missing audit log: {log_p}')
                self.assertEqual(_stat.S_IMODE(log_p.stat().st_mode), 0o600)
                raw = log_p.read_text(encoding='utf-8')
                self.assertNotIn('UltraSecretPassword!', raw)
                self.assertNotIn('SecretTok99', raw)
                self.assertNotIn('LeakMe', raw)
                self.assertNotIn('NeverLogThisPassword', raw)
                self.assertIn('[REDACTED]', raw)
                parsed = [_json.loads(ln) for ln in raw.splitlines() if ln.strip()]
                self.assertEqual(len(parsed), 3)

        # 2. /etc/group and /etc/gshadow synchronized helper (grpck -r parity)
        inst_py = (
            OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py'
        ).read_text(encoding='utf-8')
        self.assertIn('_sync_gshadow_with_groups', inst_py)
        post_sh = (OVERLAY / 'usr/bin/blazeos-postinstall').read_text(encoding='utf-8')
        self.assertIn('AUTH-9216', post_sh)
        self.assertIn('/etc/gshadow', post_sh)

        with tempfile.TemporaryDirectory() as tmp:
            t = Path(tmp)
            etc = t / 'etc'
            etc.mkdir()
            (etc / 'group').write_text(
                'root:x:0:\nwheel:x:10:alice\nvideo:x:39:\naudio:x:63:\ninput:x:104:\nrender:x:105:\nextragrp:x:999:alice\n',
                encoding='utf-8',
            )
            gshadow_p = etc / 'gshadow'
            gshadow_p.write_text(
                'root::: \nwheel:!::\nvideo:!::\naudio:!::\ninput:!::\n'.replace(' ', ''),
                encoding='utf-8',
            )
            os.chmod(gshadow_p, 0o400)

            # Emulate installation.py _add_user_to_groups logic directly without importing pyanaconda
            ns = {}
            class DummyLog:
                def warning(self, *args, **kwargs):
                    pass
            def join_paths(a, b):
                return str(Path(a) / b)
            # Parse _sync_gshadow_with_groups and _add_user_to_groups cleanly via ast
            import ast
            tree = ast.parse(inst_py)
            fn_src = []
            for node in ast.walk(tree):
                if isinstance(node, ast.FunctionDef) and node.name in ('_sync_gshadow_with_groups', '_add_user_to_groups'):
                    fn_src.append(ast.get_source_segment(inst_py, node).lstrip())
            # Dedent class methods to module functions
            import textwrap
            src_block = '\n\n'.join(textwrap.dedent('    ' + s) for s in fn_src)
            helper_globals = {'os': os, 'join_paths': join_paths, 'log': DummyLog()}
            exec(src_block, helper_globals)

            class FakeTask:
                _sysroot = str(t)
            FakeTask._sync_gshadow_with_groups = helper_globals['_sync_gshadow_with_groups']
            FakeTask._add_user_to_groups = helper_globals['_add_user_to_groups']
            FakeTask()._add_user_to_groups('bob', ['wheel', 'video', 'audio', 'input', 'render'])

            self.assertEqual(_stat.S_IMODE(gshadow_p.stat().st_mode), 0o400)
            os.chmod(gshadow_p, 0o600)
            grp_map = {
                ln.split(':')[0]: ln.strip().split(':')[3]
                for ln in (etc / 'group').read_text(encoding='utf-8').splitlines() if ln
            }
            gsh_map = {
                ln.split(':')[0]: ln.strip().split(':')[3]
                for ln in gshadow_p.read_text(encoding='utf-8').splitlines() if ln
            }
            self.assertEqual(grp_map, gsh_map)
            for req_g in ('wheel', 'video', 'audio', 'input', 'render'):
                self.assertIn('bob', grp_map[req_g].split(','))

        # 3. Initrd blockdev callout & RO=1 udev rule
        mod_setup = (
            OVERLAY / 'usr/lib/dracut/modules.d/99blaze-bere/module-setup.sh'
        ).read_text(encoding='utf-8')
        self.assertIn('blockdev', mod_setup)
        self.assertIn('/sbin/blockdev', mod_setup)

        patch_sh = (ROOT / 'scripts/patch_f45_initrd.sh').read_text(encoding='utf-8')
        self.assertIn('blockdev', patch_sh)
        self.assertIn('sbin/blockdev', patch_sh)

        vault_rule = (OVERLAY / 'etc/udev/rules.d/99-blaze-recovery-vault.rules').read_text(encoding='utf-8')
        self.assertIn('ENV{ID_FS_LABEL}=="BLAZE_REC"', vault_rule)
        self.assertIn('/sbin/blockdev --setro $devnode', vault_rule)


    def test_gate45_akrep_readonly_contract_and_specs(self):
        cargo_toml = (ROOT / 'solarui/Cargo.toml').read_text(encoding='utf-8')
        for member in (
            'blaze-sentinel-core',
            'blaze-akrep-common',
            'blaze-akrep-privd',
            'blaze-akrep-daemon',
            'blaze-akrep-ui',
        ):
            self.assertIn(member, cargo_toml)

        daemon_cargo = (ROOT / 'solarui/crates/blaze-akrep-daemon/Cargo.toml').read_text(encoding='utf-8')
        privd_cargo = (ROOT / 'solarui/crates/blaze-akrep-privd/Cargo.toml').read_text(encoding='utf-8')
        for forbidden_gui in ('gtk4', 'gdk4', 'libadwaita'):
            self.assertNotIn(forbidden_gui, daemon_cargo)
            self.assertNotIn(forbidden_gui, privd_cargo)

        dbus_src = (ROOT / 'solarui/crates/blaze-akrep-daemon/src/dbus.rs').read_text(encoding='utf-8')
        for ro_fn in ('fn get_version', 'fn get_capabilities', 'fn get_snapshot', 'fn get_findings', 'fn get_section_detail'):
            self.assertIn(ro_fn, dbus_src)
        for forbidden_mutator in (
            'fn set_',
            'fn toggle_',
            'fn apply_',
            'fn execute',
            'fn run_shell',
            'fn write_',
            'fn create_plan',
            'fn commit_plan',
        ):
            self.assertNotIn(forbidden_mutator, dbus_src)

        priv_dbus_src = (ROOT / 'solarui/crates/blaze-akrep-privd/src/dbus.rs').read_text(encoding='utf-8')
        for priv_fn in (
            'fn create_plan',
            'fn commit_plan',
            'fn rollback_transaction',
            'fn get_history',
            'fn diagnose_selinux',
            'fn inspect_firewall',
            'fn list_profiles',
            'fn preview_profile',
        ):
            self.assertIn(priv_fn, priv_dbus_src)

        ui_src = (ROOT / 'solarui/crates/blaze-akrep-ui/src/main.rs').read_text(encoding='utf-8')
        self.assertIn('spawn_local', ui_src)
        self.assertIn('present_plan_preview_dialog', ui_src)
        ui_dbus = (ROOT / 'solarui/crates/blaze-akrep-ui/src/dbus_client.rs').read_text(encoding='utf-8')
        self.assertIn('async_channel', ui_dbus)
        self.assertIn('AKREP_BUS_NAME', ui_dbus)
        self.assertIn('AKREP_PRIV_BUS_NAME', ui_dbus)

        nav_src = (ROOT / 'solarui/crates/blaze-akrep-ui/src/components/nav_rail.rs').read_text(encoding='utf-8')
        for page_id in (
            '"overview"',
            '"threat_protection"',
            '"system_protection"',
            '"firewall"',
            '"applications"',
            '"device_security"',
            '"privacy"',
            '"profiles"',
            '"history"',
            '"settings"',
        ):
            self.assertIn(page_id, nav_src)

        self.assertTrue((OVERLAY / 'usr/lib/sysusers.d/blaze-akrep.conf').is_file())
        tmpfiles_cfg = (OVERLAY / 'usr/lib/tmpfiles.d/blaze-akrep.conf').read_text(encoding='utf-8')
        self.assertIn('d /var/lib/akrep 0700 root root -', tmpfiles_cfg)
        self.assertIn('d /var/cache/akrep 0755 root root -', tmpfiles_cfg)
        svc = (OVERLAY / 'usr/lib/systemd/system/akrepd.service').read_text(encoding='utf-8')
        self.assertIn('User=akrep', svc)
        self.assertIn('BusName=tr.com.blazeos.Akrep1', svc)
        priv_svc = (OVERLAY / 'usr/lib/systemd/system/akrep-privd.service').read_text(encoding='utf-8')
        self.assertIn('BusName=tr.com.blazeos.Akrep1.Privileged', priv_svc)
        self.assertIn('--idle-timeout-secs 60', priv_svc)
        self.assertTrue((OVERLAY / 'usr/lib/systemd/system-preset/90-blaze-akrep.preset').is_file())

        dbus_pol = (OVERLAY / 'usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.conf').read_text(encoding='utf-8')
        self.assertIn('<deny own="tr.com.blazeos.Akrep1"/>', dbus_pol)
        self.assertIn('<allow own="tr.com.blazeos.Akrep1"/>', dbus_pol)
        priv_dbus_pol = (OVERLAY / 'usr/share/dbus-1/system.d/tr.com.blazeos.Akrep1.Privileged.conf').read_text(encoding='utf-8')
        self.assertIn('<deny own="tr.com.blazeos.Akrep1.Privileged"/>', priv_dbus_pol)
        self.assertIn('<allow own="tr.com.blazeos.Akrep1.Privileged"/>', priv_dbus_pol)

        polkit_pol = (OVERLAY / 'usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy').read_text(encoding='utf-8')
        self.assertIn('auth_admin_keep', polkit_pol)
        for pk_act in (
            'tr.com.blazeos.Akrep1.selinux.diagnose',
            'tr.com.blazeos.Akrep1.selinux.manage',
            'tr.com.blazeos.Akrep1.firewall.manage',
            'tr.com.blazeos.Akrep1.flatpak.system-manage',
            'tr.com.blazeos.Akrep1.quarantine.manage',
            'tr.com.blazeos.Akrep1.profile.apply',
            'tr.com.blazeos.Akrep1.packages.install-optional',
        ):
            self.assertIn(pk_act, polkit_pol)

        desktop = (OVERLAY / 'usr/share/applications/tr.com.blazeos.Akrep.desktop').read_text(encoding='utf-8')
        self.assertIn('Exec=akrep-ui', desktop)

        control_src = (OVERLAY / 'usr/bin/blazeos-control').read_text(encoding='utf-8')
        self.assertIn('subprocess.Popen(["/usr/bin/akrep-ui"])', control_src)
        self.assertIn('Akrep Security Center', control_src)

        omnibar = (ROOT / 'solarui/crates/solar-shell/src/omnibar.rs').read_text(encoding='utf-8')
        self.assertIn('"akrep-ui"', omnibar)

        spec = (ROOT / 'packaging/specs/blaze-akrep.spec').read_text(encoding='utf-8')
        for sub in ('%package daemon', '%package ui', 'Requires:       gtk4', '%systemd_post akrepd.service'):
            self.assertIn(sub, spec)


if __name__ == '__main__':
    unittest.main()
