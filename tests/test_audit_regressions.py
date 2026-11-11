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
        self.assertIn('restart xdg-desktop-portal.service', helper)

    def test_installer_choices_run_before_oobe_and_display_manager(self):
        service = (OVERLAY / 'etc/systemd/system/blazeos-apply-install-choices.service').read_text()
        helper = (OVERLAY / 'usr/local/bin/blazeos-apply-install-choices').read_text()
        provision_service = (OVERLAY / 'etc/systemd/system/blazeos-desktop-provision.service').read_text()
        provision = (OVERLAY / 'usr/local/bin/blazeos-desktop-provision').read_text()
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
        provision = (OVERLAY / 'usr/local/bin/blazeos-desktop-provision').read_text()
        remover = (OVERLAY / 'usr/local/bin/blazeos-remove-solarui').read_text()
        self.assertIn('Yalnızca masaüstü ortamı', webui)
        self.assertIn('Tüm masaüstü uygulamaları', webui)
        self.assertIn('SolarUI kaldırılsın mı?', webui)
        self.assertIn('/tmp/anaconda_desktop_profile', webui)
        self.assertIn('/tmp/anaconda_remove_solarui', webui)
        self.assertIn('/tmp/anaconda_usage_profile', webui)
        for profile in ('light', 'daily', 'office', 'gaming', 'standard', 'developer'):
            self.assertIn(f'"{profile}"', webui)
        self.assertNotIn('{id:"hyprland",name:"Hyprland"', webui)
        self.assertIn('/usr/local/bin/blazeos-remove-solarui', provision)
        self.assertIn('Refusing to remove SolarUI without a validated replacement session.', remover)
        self.assertNotIn('/usr/bin/blaze-recovery', remover)

    def test_oobe_uses_anaconda_selected_session(self):
        source = (OVERLAY / 'usr/bin/blaze-setup').read_text()
        self.assertIn('/etc/blazeos-selected-session', source)
        self.assertIn('Session={selected_desktop}', source)
        self.assertIn('Session={selected_session}', source)

    def test_live_boot_exits_before_account_or_gdm_mutations(self):
        source = (OVERLAY / 'usr/local/bin/blazeos-postinstall').read_text()
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
        source = (OVERLAY / 'usr/local/bin/blazeos-nvidia-setup').read_text()
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
        tree = ast.parse((OVERLAY / 'usr/local/bin/blazeos-control').read_text())
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
        source = (OVERLAY / 'usr/local/bin/blazeos-postinstall').read_text()
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
        self.assertIn('offline reboot', helper)
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
        provision = (OVERLAY / 'usr/local/bin/blazeos-desktop-provision').read_text()
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
        control = (OVERLAY / 'usr/local/bin/blazeos-control').read_text()
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
        self.assertIn('exec /usr/local/bin/blazeos-control', launcher)
        self.assertIn('pardus/pardus-gnome-greeter', license_notice.read_text())

    def test_core_image_trim_keeps_profiles_explicit(self):
        build = (ROOT / 'build_f45_final.sh').read_text()
        provision = (OVERLAY / 'usr/local/bin/blazeos-desktop-provision').read_text()
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
        provision = (OVERLAY / 'usr/local/bin/blazeos-desktop-provision').read_text()
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

        snap_script = (OVERLAY / 'usr/local/bin/blaze-snapshot').read_text()
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
        for nf in niri_files:
            self.assertTrue(nf.is_file(), f'Missing {nf}')
            res = subprocess.run(['niri', 'validate', '-c', str(nf)], capture_output=True, text=True, timeout=10)
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

        sddm_pam = (OVERLAY / 'etc/pam.d/sddm-autologin').read_text()
        self.assertIn('user in blaze-setup:liveuser', sddm_pam)

        postinstall_svc = (OVERLAY / 'etc/systemd/system/blazeos-postinstall.service').read_text()
        self.assertIn('Before=display-manager.service gdm.service sddm.service', postinstall_svc)
        self.assertIn('After=local-fs.target', postinstall_svc)
        self.assertNotIn('After=NetworkManager.service', postinstall_svc)

        inst_py = (OVERLAY / 'usr/lib64/python3.15/site-packages/pyanaconda/modules/boss/installation.py').read_text()
        self.assertIn('blazeos-apply-install-choices.service', inst_py)
        self.assertIn('target_autologin', inst_py)


if __name__ == '__main__':
    unittest.main()
