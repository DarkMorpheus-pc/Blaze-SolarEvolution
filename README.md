# Blaze SolarEvolution 5.05

[**Upgrade to BlazeOS (ISO)**](https://archive.org/details/blaze-solar-evolution-5-test)

<h3 align="center">Next-Generation Hybrid Desktop Ecosystem & High-Performance Linux Distribution</h3>

<img width="1209" height="752" alt="image" src="https://github.com/user-attachments/assets/e888deac-5b91-4ea9-98e2-8026f75c3bcf" />
<img width="1208" height="757" alt="image" src="https://github.com/user-attachments/assets/f79b5aea-0fb7-4933-a5ca-b52321aa7ab0" />
<img width="1208" height="754" alt="image" src="https://github.com/user-attachments/assets/7b3e685d-8caf-4270-a9d6-da6324638f0b" />

<p align="center">
  <b>"From evolution to expansion — welcome to the galaxy."</b>
</p>

---

## Surum Notlari (Release Notes) — Blaze SolarEvolution 5.05

Bu surum (`5.05`), SolarEvolution ekosistemine FireHub 1.9.0 BCDL Kinetic Glass ve GPL-3.0 lisans uyumunu, saglamlastirilmis Anaconda OOBE / SDDM ilk acilis mimarisini, arindirilmis ve modernlestirilmis SolarUI Masaustu Rehberini, optimize edilmis BlazeHouse kontrol merkezini, guncel stok Noctalia arama motorunu, coklu masaustu provizyonunu ve dayanikli kurtarma (BERP/BERE) katmanini kazandirir:

### 1. FireHub 1.9.0 & Acik Kaynak GPL-3.0 Uyumu
- **GPL-3.0 Lisans Hizalanmasi:** FireHub Hakkinda ve Lisans bolumu resmi GNU General Public License Version 3 (GPL-3.0) metnine ve resmi kaynak deposuna (`https://github.com/DarkMorpheus-pc/Blaze-Fire-Hub`) baglandi.
- **BCDL Kinetic Glass Tasarim Dili:** Akrilik cam yuzeyler, dinamik bulaniklik ve modern sekme arayuzu ile akici magazacilik deneyimi.
- **Blaze Vault Kriptografik Zirh:** 3-parcali Shamir/XOR bayt parcalama, bellek temizleme (`Uint8Array.fill(0)`), AST kod karartma ve runtime anti-debug kilitleri ile guvenli API altyapisi.
- **Guncelleme Merkezi (Updates Hub):** Canli Flatpak ve sistem guncelleme takibi, surum karsilastirmasi ve tek tikla toplu guncelleme.
- **Yerlesik Cevrimdisi Ikon Paketi:** 90+ populer uygulamanin 128x128 ikonlari dogrudan paket icine gomulerek harici CDN bagimliligi ve ag gecikmesi sifirlandi.

### 2. SolarUI Masaustu Rehberi, Kisisellestirme ve Ekran Yonetimi
- **Arindirilmis Baslik ve Tam Pencere Kontrolleri:** Masaustu rehberi ve SolarUI / Electron / SSD uygulamalarinda (FireHub, ucbirimler vb.) 30px baslik cubugu uzerinde arka plana kucultme, tam ekran/buyutme ve kapatma kontrolleri tam uyumlu hale getirildi (`99_blazeos-wm.gschema.override` ve Niri `server-titlebar`).
- **Evrensel Duvar Kagidi Koprusu:** SolarUI Ayarlari (`Duvar Kagidi ve Tema`) ve Dosya Gezgini (`Nautilus`) uzerinden secilen duvar kagitlari Noctalia, We10X, Caelestia ve end4-pC kabuklarina canli olarak senkronize edilir.
- **Manuel Cozunurluk ve Yenileme Hizi (Hz) Secici:** Bagli her ekran icin donanim EDID cozunurlukleri, tazeleme hizlari (60 Hz, 75 Hz, 120 Hz, 144 Hz, 240 Hz vb.), bagimsiz DPI olcegi ve ozel cozunurluk/Hz tanimlama araci SolarUI Ayarlarina eklendi.
- **We10X (Tanidik Klasik Duzen) Kisisellestirme Paketi:** Koyu, acik ve tam siyah (OLED) gorunum modlari; kirmizi, mavi, koyu antrasit, zumrut yesili, mor, kehribar hazir renkleri; sinirsiz RGB/Hex ozel renk secici ve gorev cubugu/Baslat menusu/baslik cubugu renklendirme destegi eklendi.
- **Dinamik Kisayol Haritasi:** Mod + Space (Omnibar), Mod + G (GameZone), Mod + I (Ayarlar), Mod + S (Kontrol Merkezi) ve elastik pencere fizigi kisayollari aninda erisilebilir hale getirildi.
- **Cift Terminal ve Fastfetch:** Kitty GPU hizlandirmali ucbirim ile Alacritty destegi, ozel SolarUI ASCII sanati ve donanim telemetrisi ile entegre edildi.

### 3. Anaconda Kurulum Sonrasi OOBE ve SDDM Otomatik Giris Guvenligi
- **Ilk Acilis Parola Kilitlemesi Giderildi:** Kurulum sonrasi SDDM oturum yoneticisinin `blaze-setup` kullanicisinda parola sormasina yol acan `/etc/shadow`, `MinimumUid` ve PAM autologin zinciri guclendirildi (`00-blaze-oobe.conf` ve `sddm-autologin`).
- **Kesintisiz Profil Olusturma:** Kurulum bittikten sonra tam ekran OOBE sihirbazi parola sormadan dogrudan acilarak kullanici adi, parola, kabuk tercihi ve sistem ayarlari adim adim tamamlanir.
- **Atomik Hijyen:** Kurulum sihirbazi tamamlandiginda gecici servisler, autologin kayitlari ve gecici yapilandirmalar otomatik olarak sistemden temizlenir; SDDM dogrudan olusturulan kullanici hesabini karsilar.

### 4. BlazeHouse Sistem Kontrol Merkezi ve Donanim Telemetrisi
- **BCDL Kinetic Glass Arayuzu:** Kullanici deneyimi korunan klasik ve ergonomik tasarim duzeni, anlik donanim on belleklemesi ile sekme gecislerindeki gecikmeleri sifirlar.
- **Tabular Rakam Telemetrisi:** CPU, GPU, ZRAM bellek kullanimi, fan devri ve sicaklik degerleri mikro-saniyelik periyotlarla izlenir.
- **Fedora / DNF5 Guvenli Cevrimdisi Guncelleme:** Sistem calisirken paket degisimi yapilmaz; guncellemeler hazirlanir ve Plymouth acilisinda dogrulanmis olarak uygulanir.

### 5. Coklu Masaustu Ortami (DE) ve Kesintisiz SDDM Provizyonu
- **Secilebilir Masaustu Ortamlari:** Anaconda uzerinden secilen KDE Plasma, GNOME, COSMIC, XFCE, Cinnamon ve Sway ortamlari icin cekirdek paketler hazirlanir.
- **Bloklamayan Arka Plan Provizyonu:** Paket indirmeleri SDDM baslangicini engellemez; kullanici SolarUI oturumuna girerken arka planda tamamlanir ve SDDM aninda yenilenir.
- **Minimal veya Tam Profil:** Temel bilesenler veya tam Fedora uygulama grubu tercihi desteklenir.

### 6. Caelestia Tam Turkce Entegrasyonu ve Canli Dil Secimi
- **Yuzde 100 Turkce Yerellestirme:** Caelestia kaynak kodundaki 670+ arayuz dizesi (Nexus Ayarlari, Yan Panel, Sistem Tepsisi, Bildirimler, Kilit Ekrani, Medya Oynatici, Hizli Anahtarlar, Saat, Ag ve Guc Menuleri) Turkcelestirildi.
- **Yerel Qt Ceviri Motoru:** C++ `CaelestiaQTranslator` ile calisma aninda O(1) bellek erisimiyle Turkce dizeler cozulur.
- **Nexus Ayarlarinda Canli Dil Secici:** Dil ve Bolge sayfasinda Sistem Dili, Turkce ve English secenekleri arasinda oturumu yeniden baslatmadan aninda gecis yapilabilir.

### 7. BERP, Zirhli BLAZE_REC Kurtarma Bolumu ve BERE USB ISO Yoneticisi
- **Fiziksel Zirhli Kurtarma Bolumu (`BLAZE_REC`):** Anaconda otomatik bolumlemesi sirasinda kok dosya sisteminden bagimsiz 2.5 GiB salt-okunur fiziksel GPT kurtarma bolumu (`RequiredPartition` + `ReadOnly bit 60`, `UDISKS_IGNORE=1`) olusturulur; kok sistem tamamen silinse dahi tam grafiksel Wayland BERE ayakta kalir.
- **BERE USB Gezgini ve Coklu Motorlu ISO Baslatici:** Takili USB belleklerdeki (`FAT32`, `exFAT`, `NTFS`, `ext4`, `Btrfs`) ISO kalip dosyalari salt-okunur taranip dahili diske (`/home/.blaze-isos/`) cekilebilir ve `kexec` aninda gecis veya GRUB2 uzerinden `RAM'e Yukle` (`rd.live.ram=1` / `toram` / `copytoram=y`) secenegiyle baslatilabilir.
- **BERP (Blaze Emergency Recovery Platform) & Blaze Sentinel:** Bagimsiz ramdisk konsol onarim araci (TUI), Btrfs Zaman Makinesi ve arka plan SELinux/donanim guvenlik nobetcisi birlikte calisir.

### 8. Ag, Guvenlik ve Sistem Optimizasyonlari
- **Telemetri ve Gecikme Onleme:** NetworkManager captive portal sorgusu devreden cikarildi; sistem loglarini ve disk G/C'sini yoran gereksiz arka plan servisleri (`auditd`, `sssd`, `stratisd`, `mcelog`) arindirildi.
- **ZRAM ve Dusuk Gecikme Sistem Yapilandirmasi:** Yuksek sikistirma algoritmali ZRAM takas alani ve dusuk gecikmeli `sysctl` / zamanlayici iyilestirmeleri uygulandi.

---

## Blaze Katmani — Fedora Tabanindan Farklar

SolarEvolution, Fedora uzerine basit bir tema giydirmesi degildir; ozel olarak gelistirilen Rust, Python ve C++ bilesenlerini barindirir: **SolarUI** (Niri tabanli Wayland oturum ve dinamik kabuk motoru), **BlazeHouse** (18 sayfali BCDL sistem yonetim merkezi), **FireHub** (GPL-3.0 uyumlu magaza ve guncelleme motoru), **BERE** (grafiksel kurtarma ortami), **BERP** (TUI acil durum kurtarma), **Blaze Sentinel** (SELinux, guvenlik duvari ve donanim nobetcisi) ve **Blaze GameZone** (tam ekran oyun ve konsol modu).

- Dogrulanmis ozellik tablosu ve durumlar: **[docs/FEATURES.md](docs/FEATURES.md)**
- Bilesen kaynaklari ve lisans dokumu: **[SOURCES.md](SOURCES.md)**
- Sistem guncelleme mimarisi: **[UPDATE-SYSTEM-5.05.md](docs/UPDATE-SYSTEM-5.05.md)**

---

## About (Hakkinda)

**Blaze SolarEvolution 5.05** is a high-performance Linux operating system ecosystem built on Fedora Workstation, specifically optimized for gamers, developers, and content creators.

The system ships **SolarUI (Niri Wayland + Noctalia + Caelestia)** as its offline stock desktop. Six downloadable desktop environments and the bundled standalone Niri session can be selected when the installer verifies access to the official Fedora repositories. Each downloadable desktop offers a minimal or full application profile. GRUB2 and the bundled upstream Limine release are available as bootloader choices on supported firmware.

---

## Key Features (Temel Ozellikler)

- **SolarUI Desktop Ecosystem:** A clean, fluid desktop experience with glassmorphism support, built on the Niri Wayland window manager, the fast C++ Noctalia shell, and the elegant Caelestia shell.
<p align="center">
  <img width="1205" height="753" alt="image" src="https://github.com/user-attachments/assets/beac43d9-0943-44c9-b951-712629f23456" />
</p>

- **SolarUI and 7 Optional Desktop Sessions:**
  - **SolarUI:** The default fluid Niri desktop with independently selectable complete shell engines (Available Offline)
  - **GNOME:** Installed on demand from the official Fedora repositories
  - **Niri (Standalone):** An optional standalone session using SolarUI's bundled Niri compositor
  - **KDE Plasma:** Highly customizable Plasma 6 on Wayland
  - **COSMIC:** An independent, modern, Rust-based desktop by System76
  - **Cinnamon:** A classic and practical desktop layout
  - **XFCE:** An extremely lightweight and fast X11 environment
  - **Sway:** An i3-compatible, keyboard-driven tiling environment
- **Limine & GRUB2 Bootloader Support:** GRUB2 remains the Secure Boot compatible default. UEFI installations can select the bundled official Limine release without third-party repository dependencies.
- **BlazeHouse & System Control:** Modern BCDL Kinetic Glass control center featuring real-time hardware telemetry with tabular numerals, instant cached navigation, and ZRAM tuning.
- **FireHub 1.9.0 Software Center:** BCDL Kinetic Glass interface, Updates Hub for Flatpak & system packages, and cryptographic Blaze Vault armor under GPL-3.0.
- **Low Latency & ZRAM Improvements:** DNF5 configurations for faster downloads, default ZRAM optimizations, and low-latency `sysctl` tuning settings.
- **Online & Offline ISO Architecture:** Build support for a lightweight 3.5 GB Online ISO and a fully bundled 10 GB+ Offline ISO.

---

## Architecture (Sistem Yapisi)

```text
Blaze-SolarEvolution 5.05
|-- build_f45_final.sh          # Online ISO build script (Squashfs + xorriso + SELinux)
|-- build_f45_offline.sh        # Offline ISO build script
|-- blazeos_custom_apps/        # Custom-developed system tools and desktop configurations
|   |-- etc/                    # Systemd, SDDM, PAM, and Anaconda configs
|   |-- usr/local/bin/
|   |   |-- blazeos-control     # GTK4 Control Center
|   |   |-- blazeos-welcome     # Welcome and initial setup wizard
|   |   |-- blaze-bootloader    # Limine / GRUB2 switching tool
|   |   |-- blazeos-postinstall # Post-installation automation service
|   |   `-- blaze-optimize      # ZRAM & kernel optimizations
|   |-- usr/bin/
|   |   |-- firehub             # FireHub smart Wayland wrapper
|   |   |-- berp-recovery-gui   # Emergency recovery launcher
|   |   |-- blaze-house         # Blaze House control center
|   |   |-- solar-shell         # SolarUI desktop manager binary
|   |   `-- solar-settings      # SolarUI configuration GUI
|   |-- usr/share/caelestia/    # Caelestia translations (tr.mo)
|   `-- usr/lib64/quickshell/   # Quickshell Caelestia I18n and plugin modules
|-- solarui/                    # SolarUI Rust crates (solar-shell, session, common)
|   `-- vendor/caelestia-shell/ # Caelestia shell QML and C++ I18n plugin code
|-- tests/                      # System audit & regression verification suite
|-- Blaze-SolarEvolution-5-x86_64.iso.sha256 # Official build checksum
`-- README.md                   # Release documentation (5.05)
```

---

## Build Requirements (Derleme Gereksinimleri)

To build the ISO image locally, the following packages and tools must be installed on your system:

- **Operating System:** Fedora 40+, Arch Linux, CachyOS, or a RHEL-based 64-bit Linux distribution
- **Disk Space:** At least 30 GB of free tmpfs / disk space
- **Required Tools:**
  ```bash
  # On Fedora / RHEL:
  sudo dnf install -y xorriso isolinux squashfs-tools isomd5sum libattr-devel

  # On Arch Linux / CachyOS:
  sudo pacman -S --needed xorriso squashfs-tools attr
  ```

---

## How to Build (ISO Derleme Adimlari)

### 1. Clone the Repository
```bash
git clone https://github.com/DarkMorpheus-pc/Blaze-SolarEvolution.git
cd Blaze-SolarEvolution
```

### 2. Prepare the Base ISO Image
Place the Fedora Workstation 45 / 44 Live ISO image in the `f45_base/` directory.

### 3. Run the ISO Build Script

- **Online ISO Build (3.5 GB):**
  ```bash
  chmod +x build_f45_final.sh
  sudo ./build_f45_final.sh
  ```

- **Full Offline ISO Build (10 GB+):**
  ```bash
  chmod +x build_f45_offline.sh
  sudo ./build_f45_offline.sh
  ```

Once the build is complete, your ISO image will be available in the repository root directory (`Blaze-SolarEvolution-5-x86_64.iso`) and will pass the MD5 verification check (`checkisomd5 PASS`).

Official Checksum (SHA-256):
```text
baa6dce0af43741588f9c5973cecff2f85eb9b085717a80a972c622a4ac4a624  Blaze-SolarEvolution-5-x86_64.iso
```

---

## SolarUI Keyboard Shortcuts (Kisayol Tuslari)

| Shortcut | Action (Islev) |
| :--- | :--- |
| `Mod + Return` | Terminal (Kitty / Ptyxis / Alacritty) Ac |
| `Mod + Space` | Solar Omnibar — Uygulama, Arama ve Hesaplama |
| `Mod + G` | Blaze GameZone — Konsol & Oyun Modu |
| `Mod + S` | SolarUI Kontrol Merkezi |
| `Mod + I` | Masaustu ve Kabuk Ayarlari (Solar Settings) |
| `Mod + Alt + R` | Masaustu Kabugunu Yeniden Baslat (Supervisor) |
| `Mod + T` / `Mod + F` | Yuzen Pencere / Tam Ekran Gecisi |
| `Mod + Q` / `Alt + F4` | Aktif Pencereyi Kapat |
| `Mod + Shift + E` | Oturumu Kapat / Cikis Menusu |

---

## License & Contributing (Lisans ve Katki)

This project is released under the **GPL-3.0** license. You can contribute by opening a pull request or reporting an issue.

### Tesekkurler ve Katkilar

SolarUI ve Blaze SolarEvolution asagidaki acik kaynak projelerin calismalarindan faydalanir. Proje adlari, lisanslari ve upstream kimlikleri korunur:

- [Noctalia](https://github.com/noctalia-dev/noctalia)
- [Caelestia Shell](https://github.com/caelestia-dots/shell)
- [end4-pC](https://github.com/pctrade/end4-pC)
- [We10XOS-kde](https://github.com/yeyushengfan258/We10XOS-kde) & [We10X-icon-theme](https://github.com/yeyushengfan258/We10X-icon-theme) — GPL-3.0 klasik masaustu gorsel varliklari, duvar kagitlari ve ikon temasi
- [illogical-impulse / dots-hyprland](https://github.com/end-4/dots-hyprland)
- [Pardus GNOME Greeter](https://github.com/pardus/pardus-gnome-greeter) — LGPL-3.0 gorunum ve ilk oturum deneyimi ilhami

Designed & Crafted for **BlazeOS & CachyOS** by **DarkMorpheus**.
