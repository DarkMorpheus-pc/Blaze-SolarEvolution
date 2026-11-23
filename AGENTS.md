# BlazeOS / BlazeFedora Çalışma Alanı Kuralları

## 1. Zorunlu Sanal Makine (QEMU) Doğrulaması
- Uygulama, kabuk (SolarUI / Noctalia / Caelestia / end4-pC / We10X), FireHub veya kurulum sihirbazı (Anaconda / OOBE) değişikliklerini **asla yalnızca host makinede çalıştırarak doğrulama**.
- Üretilen her ISO imajı tamamlandı sayılmadan önce mutlaka QEMU içinde hem **Legacy BIOS** hem **UEFI OVMF** modlarında canlı olarak açılmalı (`LC_ALL=tr_TR.UTF-8 python3 -m tests.e2e.run_e2e --all --firmware both`) ve ekran görüntüleri (Plymouth, SolarUI masaüstü, FireHub, Anaconda WebUI) görsel olarak kontrol edilmelidir.

## 2. Oturum, PAM/SELinux ve Paketleme Değişmezleri
- **Live Oturum (`livesys`):** Canlı ISO SDDM giriş ekranına düşmemeli; `etc/sysconfig/livesys` içinde `livesys_session="solarui"` tanımlı olmalı ve doğrudan SolarUI açılmalıdır.
- **PAM ve SELinux (`sddm-autologin`):** `blazeos_custom_apps/etc/pam.d/sddm-autologin` içinde `pam_selinux.so close` ve `pam_selinux.so open` satırları asla atlanmamalıdır (`liveuser` oturumunun `xdm_t` yerine `unconfined_t` alanına geçmesi `liveinst`/Polkit için zorunludur).
- **Workstation / GNOME Ayrımı:** Stok Fedora Workstation GNOME masaüstü ve gereksiz servisler (`gdm`, `gnome-shell*`, `libreoffice*` vb.) canlı çekirdek imajdan temizlenmeli; GNOME yalnızca Anaconda kurulum seçeneklerinde sunulmalıdır (`default_scheme = BTRFS`).
- **Fedora 45+ RPM & DNF5:** Depo tanımı `/usr/share/dnf5/repos.d/blaze.repo` yolunu kullanmalı; RPM 6.1+ EdDSA imza kontrollerinde `%{OPENPGP:pgpsig}` ve `rpm -K` zorunlu tutulmalıdır (`publish-repo.sh` imzasız pakette `exit 1` vermelidir).

## 3. Gizlilik, Taşınabilirlik ve Dokümantasyon Kuralları
- **Sıfır Geliştirici Yolu:** Rust kaynaklarında (`solarui/crates/`), betiklerde ve `.iso.sha256` dosyalarında asla `/home/<kullanici>` gibi yerel geliştirici yolları bulunmamalıdır (`.iso.sha256` yalnızca göreli dosya adını içermelidir).
- **Sıfır Emoji & Görsel/Link Koruması:** `README.md` güncellemelerinde ve git commit mesajlarında **kesinlikle emoji kullanılmaz**; mevcut tüm görsel etiketleri ve bağlantılar korunur.
- **Sırlar ve Büyük İkililer:** `*.iso`, çalışma rootfs dizinleri (`work_f45*/`) ve `blazeos_custom_apps/opt/FireHub/` `.gitignore` altında tutulur; GitHub'a göndermeden önce API anahtarı / kimlik bilgisi taraması yapılır.

## 4. Niri Dizgici ve Quickshell Kabuk Geliştirme Kuralları
- **Niri IPC Oran Ölçeği (`0.0..=100.0`):** `niri_ipc::SizeChange::SetProportion` ve `PositionChange::SetProportion` kesir (`0.5`) değil yüzdelik değer (`50.0`) bekler; sağ yarı ekran (`RightHalf`) konumlandırmasında `x = 50.0` verilmeli ve tam ekran (`Maximized`) pencere yarı ekrana yaslanmadan önce `MaximizeWindowToEdges` kapatılmalıdır.
- **Quickshell İkon Çözümleme:** Qt6 QML'de `Quickshell.iconPath(ad, true)` çağrısı `(QString, QString)` aşırı yüklemesine düşüp kırık ikon üretebildiğinden her zaman önce `Quickshell.hasThemeIcon(ad)` kontrol edilmeli ve kabuk başlatıcısında `QS_ICON_THEME` açıkça tanımlanmalıdır.
- **Wayland Girdi Maskesi ve Kabuk İzolasyonu:** Tam ekran şeffaf Quickshell `PanelWindow` katmanlarında masaüstü tıklamalarını yutmamak için `mask: Region` zorunludur; özel pencere/GTK kuralları uygulayan kabuklardan (`We10X` vb.) diğer kabuklara geçilirken `"background"` çalışma alanındaki küçültülmüş pencereler geri çağrılmalı ve önceki `niri.kdl`/GTK anlık görüntüsü kalıntısız geri yüklenmelidir.
