# Akrep Esnek Koruma Kontrolleri ve Uyumluluk Yardımcıları — Tasarım Sözleşmesi (2026-11-26)

## 1. Amaç ve Gerekçe

Fedora 45 tabanlı Blaze SolarEvolution üzerinde oyun oynarken, yerel ağ (LAN) paylaşımı yaparken veya Android ROM (AOSP / LineageOS) / konteyner geliştirme gibi ağır iş yüklerini çalıştırırken kullanıcılar sistem güvenlik ve kaynak sınırlarına (SELinux, Firewalld, `systemd-oomd`, açık dosya limiti `nofile`, `android-udev-rules`, Flatpak kum havuzu ve Secure Boot MOK) takılabilmektedir.

Kullanıcının talebi:
- **Seçenek A + isteğe bağlı `"Ben tekrar değiştirene kadar"`**: Akrep arayüzünden güvenlik korumaları (SELinux modu, Firewalld) ve geliştirme engelleri (`systemd-oomd`, `nofile`, Android udev/kütüphaneler, konteyner klasör etiketi, Flatpak disk/kumanda izinleri) tek yerden kolayca açılıp kapatılabilmeli.
- Varsayılan olarak güvenli ve süreli (`15 dk`, `1 saat`, `Yeniden başlatmaya kadar`) gevşetme sunulmalı; ancak kullanıcı bilinçli olarak isterse **`Ben tekrar değiştirene kadar`** (`UntilChanged`) kalıcı seçeneğini de kullanabilmeli.

## 2. Değerlendirilen Yaklaşımlar ve Seçim

| Yaklaşım | Özet | Ödünleşim | Karar |
|---|---|---|---|
| **A (Seçilen + Kalıcı Seçenek)** | Süreli (`15 dk`, `1 saat`, `Yeniden başlatmaya kadar`) ve isteğe bağlı **`Ben tekrar değiştirene kadar`** manuel aç/kapa kontrolleri + gerçek oyun/ROM uyumluluk yardımcıları; `SELINUX=disabled` ve global Flatpak kaçışları kapalı kalır | Kullanıcı takılmaz ve isterse kalıcı kontrol alır; `SELINUX=permissive` etiketleri koruduğu için `Enforcing`'e dönüşte `autorelabel` maliyeti oluşmaz | **Kullanıcı tarafından seçildi** |
| B (Tam Uzman Modu) | Gizli Uzman Modu altında `SELINUX=disabled`, `filesystem=host`, `devices=all` dahil tüm kırmızı çizgileri kaldırır | Etiket bozulması ve sistem genelinde kum havuzu kırılması riski yaratır | Reddedildi |
| C (Yalnızca Hedefli Tarifler) | Global SELinux/Firewalld duraklatma hiç sunulmaz; yalnızca belirli senaryo reçeteleri verilir | Öngörülmeyen 3. parti yazılım sorunlarında kullanıcıyı terminale iter | Reddedildi |

## 3. Hedefler, Kapsam Dışı Olanlar ve Anti-Hedefler

### Hedefler
1. **SELinux Mod Kontrolü (`Enforcing` <-> `Permissive`):** `15 dk`, `1 saat`, `Yeniden başlatmaya kadar` ve `Ben tekrar değiştirene kadar` seçenekleriyle tek tıkla geçiş, gevşetilmiş durumdayken süreyi/kalıcılığı değiştirebilme, önizleme diyaloğu, Polkit onayı, denetim kaydı ve `Enforcing`'e dönüş.
2. **Firewalld Duraklatma / Açma:** `15 dk`, `1 saat`, `Yeniden başlatmaya kadar` ve `Ben tekrar değiştirene kadar` seçenekleriyle güvenlik duvarını duraklatma, gevşetilmiş durumdayken süreyi güncelleme ve yeniden etkinleştirme + yerel ağ / geliştirme hızlı şablonları (Samba, 3000/tcp, 8080/tcp, Jellyfin 8096/tcp).
3. **Geliştirici ve Android ROM Uyumluluk Yardımcıları (Sistem Koruması sayfasında yeni bölüm):**
   - **`systemd-oomd` kontrolü:** Ağır derlemelerde (`soong`, `java`, `rustc`) süreç sonlandırmayı önlemek için `1 saat`, `4 saat`, `Yeniden başlatmaya kadar` veya `Ben tekrar değiştirene kadar` duraklatma / yeniden açma.
   - **Açık dosya limiti (`nofile 65536 / 524288`):** Yönetilen drop-in dosyalarıyla (`/etc/security/limits.d/90-akrep-dev-nofile.conf` ve `/etc/systemd/user.conf.d/90-akrep-nofile.conf`) yumuşak limiti `65536`'ya yükseltip sert limiti Fedora 45 varsayılanı olan `524288` düzeyinde koruyarak (Wine/Proton `esync` gerilemesini önleyerek) aç/kapa.
   - **Konteyner / ROM derleme klasörü SELinux etiketi:** Yalnızca kullanıcı çalışma klasörlerinde (`/home/<kullanici>/<alt-klasor>`, `/mnt/<alt-klasor>`, `/run/media/<kullanici>/<alt-klasor>`) `chcon -R --no-dereference -t container_file_t` uygulama ve doğrudan yanında **Varsayılan etikete dön (`restorecon`)** düğmesi.
   - **İsteğe bağlı uyumluluk paketleri:** Sabit enum ile `android-udev-rules` (kurulum sonrası `udevadm control --reload-rules && udevadm trigger`) ve `ncurses-compat-libs` tek tıkla kurulum/durum denetimi.
4. **Flatpak Oyun ve Disk Kısayolları (`applications.rs`):**
   - Uygulama başına tek tıkla **"Oyun kumandası (`devices=input`)"** anahtarı (mevcut `models.rs` izin veriyor; `devices=all` yasaklı kalır).
   - Hızlı klasör izin düğmeleri (`/mnt`, `/run/media`, özel yol ekleme — mevcut `FlatpakSetOverride` ve yol doğrulaması korunur).
5. **Secure Boot ve Sürücü (MOK) Durum Kartı (`device_security.rs`):**
   - Polkit gerektirmeden `akrepd` `GetSnapshot` üzerinden anında görüntülenen Secure Boot, MOK anahtar özeti ve NVIDIA/üçüncü parti çekirdek modülü uyumluluk satırları (derin denetim yapıldığında `DeviceSecurityStatus` içinde ayrıntılandırılır) + `mokutil` rehberi.
6. **Canlı Görünürlük, Profil Etkileşimi ve 0 MiB Boşta Bellek Korunumu:**
   - Süreli zamanlayıcılar `akrep-privd` belleğinde değil, PID 1 yönetimindeki geçici `systemd` zamanlayıcıları (`akrep-revert-<alt-sistem>.timer`) olarak çalışır; `akrep-privd` 60 sn sonra `0 MiB RSS` ile çıkmaya devam eder.
   - Etkin gevşetmeler `/var/cache/akrep/active_overrides.json` (`0644`) üzerinden salt-okunur `akrepd` tarafından okunur; Genel Bakış ve üst çubuk etkin geçersiz kılmaları (`14 dk kaldı`, `Yeniden başlatmaya kadar`, `Elle açılana kadar`) anında gösterir.
   - Resmi güvenlik profillerinden biri (`Dengeli`, `Geliştirici`, `Oyun`, `Herkese Açık Ağ`) uygulandığında, eğer SELinux `Permissive` veya `firewalld` duraklatılmışsa profil işlemi otomatik olarak `SELinux Enforcing` ve `firewalld` etkin durumuna geri dönerek ilgili geçersiz kılma/zamanlayıcıları temizler (hata verip kullanıcıyı kilitlemez; değişiklik önizleme diyaloğunda açıkça gösterilir).

### Kapsam Dışı Olanlar ve Anti-Hedefler
- **`SELINUX=disabled` kesinlikle yasak kalır:** Fedora 45'te `SELINUX=disabled` dosya etiketlerini bozar ve geri dönüşte uzun `autorelabel` gerektirir. Kalıcı gevşetme daima `SELINUX=permissive` üzerinden yapılır.
- **Mevcut güvenlik kırmızı çizgileri korunur:** `audit2allow`, `semodule`, global Flatpak geçersiz kılmaları (`global`), blanket `filesystem=host`/`/`/`/etc`, `devices=all`, D-Bus kum havuzu kaçışları ve kabuk enjeksiyonu (`shell=True` yok) engellenmeye devam eder.
- **Salt-okunur `akrepd` sözleşmesi bozulmaz:** `akrepd` (`User=akrep`, `ProtectSystem=strict`) asla mutasyon yapmaz ve `akrep-privd`'yi salt görüntüleme için uyandırmaz.
- **Diğer BlazeOS uygulamalarına dokunulmaz:** FireHub, Blaze House, OOBE, SolarUI Settings, GameZone kapsam dışındadır.
- **Görsel tasarım sözleşmesi korunur:** [2026-11-26-blaze-ui-unslop-design.md](2026-11-26-blaze-ui-unslop-design.md) kurallarına (Grafit + Blaze ember, 4px ızgara, 8px durum noktası, 16px sembolik ikon, cümle düzeni Türkçe, sıfır emoji, sıfır `(Polkit)` düğme etiketi, `1060x720` taşmasız düzen) %100 uyulur.

---

## 4. Veri Modeli, Polkit ve D-Bus Sözleşmesi

### 4.1 `blaze-akrep-common/src/models.rs` Ekleri

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideSubsystem {
    Selinux,
    Firewalld,
    Oomd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverrideDuration {
    /// Automatically revert after `minutes` via transient systemd timer (validated: 1..=1440).
    Timed { minutes: u32 },
    /// Runtime-only; cleared automatically on next reboot (verified via kernel boot_id).
    UntilReboot,
    /// Persistent across reboots until the user explicitly re-enables protection.
    UntilChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelinuxDesiredMode {
    Enforcing,
    Permissive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatPackageGroup {
    AndroidUdevRules,
    AndroidLegacyLibs,
}
```

`ActionRequest` enumuna eklenecek yeni varyantlar:
- `SelinuxSetMode { mode: SelinuxDesiredMode, duration: OverrideDuration }`
  - `mode == Enforcing` iken `duration` yok sayılır (daima kalıcı `SELINUX=enforcing` + `setenforce 1` + zamanlayıcı iptali).
  - Polkit eylemi: `tr.com.blazeos.Akrep1.selinux.manage`.
  - Risk düzeyi: `Enforcing` -> `Low`, `Permissive + Timed/UntilReboot` -> `Medium`, `Permissive + UntilChanged` -> `High`.
- `FirewallSetState { enabled: bool, duration: OverrideDuration }`
  - `enabled == true` iken `firewalld.service` kalıcı etkinleştirilip başlatılır ve zamanlayıcı iptal edilir.
  - Polkit eylemi: `tr.com.blazeos.Akrep1.firewall.manage`.
  - Risk düzeyi: `enabled == true` -> `Low`, `Timed/UntilReboot` -> `Medium`, `UntilChanged` -> `High`.
- `OomdSetState { enabled: bool, duration: OverrideDuration }`
  - Polkit eylemi: `tr.com.blazeos.Akrep1.system.compat-manage`.
  - Risk düzeyi: `Low` (açma) / `Medium` (duraklatma).
- `DevLimitsSetNofile { enabled: bool, soft_limit: u32, hard_limit: u32 }`
  - Doğrulama: `1024 <= soft_limit <= hard_limit <= 1_048_576`.
  - UI varsayılan açma değeri: `(soft_limit: 65536, hard_limit: 524288)` — Fedora 45'in `524288` sert sınırını düşürmeden yumuşak sınırı yükseltir (Proton/Wine `esync` uyumluluğu korunur).
  - PAM drop-in (`/etc/security/limits.d/90-akrep-dev-nofile.conf`) `*` etki alanını (`* soft nofile <soft>`, `* hard nofile <hard>`), systemd kullanıcı drop-in'i (`/etc/systemd/user.conf.d/90-akrep-nofile.conf`) `DefaultLimitNOFILE=<soft>:<hard>` yazar.
  - Polkit eylemi: `tr.com.blazeos.Akrep1.system.compat-manage`.
  - Risk düzeyi: `Low`.
- `SelinuxLabelContainerDir { target_path: String, recursive: bool }`
  - Doğrulama: Mutlak yol, `..` yok, kabuk metakarakteri yok, yalnızca `/home/<varolan-kullanici>/<alt-klasor>`, `/mnt/<alt-klasor>` veya `/run/media/<varolan-kullanici>/<alt-klasor>` altında ve gerçek bir dizin olmalı (kök `/`, `/home`, `/home/<user>` kendisi, `/etc`, `/usr`, `/var`, `/boot`, `/root`, `/dev`, `/proc`, `/sys` reddedilir; Polkit yönetici onayı gerektirdiğinden herhangi bir geçerli kullanıcı alt klasörü desteklenir).
  - Uygulama: AOSP/LineageOS gibi 500k–1.5M+ dosyalık ağaçlarda zaman aşımı veya JSON şişmesi olmaması için dosya başına hash alınmaz; kanonik kök dizine `chcon --no-dereference [-R] -t container_file_t <dir>` uygulanır (`chcon` hızlıdır, SELinux politika deposuna dokunmaz; sembolik bağları takip etmez; 60 sn zaman aşımı korunur).
  - `pre_snapshot` / `post_fingerprints` ve geri alma: Yalnızca kök dizinin kendi SELinux bağlamı parmak izi olarak kaydedilir (derleme sırasında alt dosyaların değişmesi rollback'i `ExternalDriftDetected` ile bozmaz); geri alma (`RollbackTransaction`) kök dizin ve alt ağaç üzerinde `restorecon -h [-R] <dir>` çalıştırarak Fedora varsayılan bağlamına (`user_home_t` / `mnt_t`) döndürür. Ayrıca UI'da doğrudan `Varsayılan etikete dön` (`SelinuxRestorecon` veya ters işlem) düğmesi bulunur.
  - Polkit eylemi: `tr.com.blazeos.Akrep1.selinux.manage`.
  - Risk düzeyi: `Medium`.
- `InstallCompatibilityPackage { package_group: CompatPackageGroup }`
  - Polkit eylemi: `tr.com.blazeos.Akrep1.packages.install-optional`.
  - Risk düzeyi: `Low`.

### 4.2 Polkit, `AkrepSnapshot` Şeması ve D-Bus Değişiklikleri

- `blazeos_custom_apps/usr/share/polkit-1/actions/tr.com.blazeos.Akrep1.policy`:
  - Yeni `tr.com.blazeos.Akrep1.system.compat-manage` eylemi eklenir (`allow_any=auth_admin`, `allow_inactive=auth_admin`, `allow_active=auth_admin_keep`).
- **`akrepd` ve `AkrepSnapshot` sözleşmesi (geriye dönük uyumlu, 0 `akrep-privd` uyanması):**
  - `AKREP_VERSION = "1.0.0"` ve `AKREP_SCHEMA_VERSION = 2` korunur; `tr.com.blazeos.Akrep1.conf` içindeki 5 salt-okunur D-Bus metodu değiştirilmez.
  - `blaze-sentinel-core/src/models.rs` içindeki `AkrepSnapshot` yapısına yalnızca `#[serde(default)]` ile geriye dönük uyumlu üç alan eklenir:
    - `active_overrides: Vec<ActiveOverrideEntry>` (`/var/cache/akrep/active_overrides.json` + canlı `boot_id`/süre denetimi)
    - `system_compat: Option<SystemCompatStatus>` (`systemd-oomd` etkin/aktif durumu, `nofile` drop-in durumu + soft/hard değerleri, `android-udev-rules` ve `ncurses-compat-libs` kurulu durumu — RPM sorgusu 15 sn TTL ile önbelleklenerek `collect_snapshot` <250 ms bütçesi korunur)
    - `driver_mok: Option<DriverMokSummary>` (`SecureBoot` durumu, `/sys/module/nvidia` veya `akmod-nvidia` varlığı, `mokutil --list-enrolled` özet sayısı — salt-okunur, 15 sn TTL önbellekli)
  - Böylece Sistem Koruması ve Cihaz Güvenliği sayfaları açıldığında Polkit istemeden ve `akrep-privd`'yi uyandırmadan (`gate_akrep_v1.py` değişmezi) anında canlı durumu gösterir; ayrıca `InspectDeviceSecurity` derin denetimi de MOK ayrıntısını içerir. `akrep-privd` üzerine ekstra bir `InspectSystemCompat()` D-Bus metodu eklenmesine gerek kalmaz.
- **Şiddet derecesi ve bulgu uyumu (`akrepd`):**
  - `blaze-sentinel-core` ham güvenlik duruşunu dürüstçe raporlamayı sürdürür.
  - `akrepd`, `GetSnapshot` içinde etkin bir Akrep geçersiz kılması (`active_overrides`) varken ilgili bulguyu/bölümü kullanıcı niyetiyle uyumlu biçimde açıklar ve tekilleştirir:
    - `Timed` / `UntilReboot` gevşetmelerinde ilgili bölüm ve bulgu `Attention` (amber, kalan süre/yeniden başlatma notuyla) olarak raporlanır.
    - `UntilChanged` (`Ben tekrar değiştirene kadar`) seçildiyse: SELinux `Permissive` `Attention` (etiketler sağlam, denetleme sürüyor), Firewalld `Kapalı` ise `Critical` (kırmızı, gelen koruma kapalı) olarak raporlanır; Genel Bakış üstündeki **Etkin gevşetmeler** kartı her durumda görünür kalır.
- **`blaze-akrep-privd` D-Bus ve CLI:**
  - Mevcut `CreatePlan`, `CommitPlan`, `RollbackTransaction` yeni 6 `ActionRequest` varyantını destekler.
  - Tüm ayrıcalıklı değişiklikler (korumayı geri açma dahil) Akrep'in standart `CreatePlan` -> `present_plan_preview_dialog` -> `CommitPlan` iki adımlı önizleme + Polkit + işlem kaydı akışından geçer (gizli/sessiz root mutasyonu yoktur).
  - Süreler arası geçiş (`Timed` <-> `UntilReboot` <-> `UntilChanged`): Zaten gevşetilmiş bir alt sistemde `Timed` veya `UntilReboot` seçildiğinde, önceki `UntilChanged` kalıcı ayarları (`/etc/selinux/config` -> `SELINUX=enforcing`, `systemctl enable firewalld.service` / `systemd-oomd.service`) anında geri senkronize edilir ki yeniden başlatma veya zamanlayıcı gerçek korumayı eksiksiz geri getirsin.
  - Yeni CLI bayrağı (yalnızca root + `systemd` zamanlayıcısı tarafından çağrılır):
    `akrep-privd --revert-override <selinux|firewalld|oomd> --generation <uuid>`
    - Süresi dolan geçici gevşetmeyi uygular (`setenforce 1`, `systemctl start firewalld.service` veya `systemctl start systemd-oomd.service`), `/var/lib/akrep/overrides.json` ve `/var/cache/akrep/active_overrides.json` dosyalarını atomik günceller ve `/var/lib/akrep/audit.log` içine `override_auto_reverted` olayı yazar.
    - Eğer kullanıcı zamanlayıcı tetiklenmeden önce modu değiştirmiş veya `UntilChanged`'a çevirmişse (`generation_id` eşleşmiyorsa) zamanlayıcı güvenle no-op olarak çıkar (yarış koşulu koruması).

### 4.3 Geçici Zamanlayıcı ve Durum Önbelleği Tasarımı

- Yetkili durum dosyası: `/var/lib/akrep/overrides.json` (`root:root`, `0600`)
- Salt-okunur ortak önbellek: `/var/cache/akrep/active_overrides.json` (`root:root`, `0644`, atomik `tmp + fsync + rename`)
- Şema:
```json
{
  "schema_version": 1,
  "boot_id": "<current /proc/sys/kernel/random/boot_id>",
  "entries": {
    "selinux": {
      "subsystem": "selinux",
      "relaxed": true,
      "duration": { "kind": "timed", "minutes": 15 },
      "generation_id": "uuid-v4",
      "started_at": "2026-11-26T13:30:00Z",
      "expires_at": "2026-11-26T13:45:00Z",
      "timer_unit": "akrep-revert-selinux.timer"
    }
  }
}
```
- **Zamanlayıcı yönetimi:**
  - Yeni bir süreli gevşetme uygulanmadan önce mevcut `akrep-revert-<subsys>.timer` ve `.service` `systemctl stop` + `systemctl reset-failed` ile temizlenir.
  - Zamanlayıcı `systemd-run --unit=akrep-revert-<subsys> --on-active=<secs>s --timer-property=AccuracySec=1s --description="Akrep auto-revert <subsys>" /usr/bin/akrep-privd --revert-override <subsys> --generation <generation_id>` ile kurulur.
- **Yeniden başlatma sonrası doğruluk:**
  - `akrepd` `active_overrides.json` dosyasını okurken mevcut `/proc/sys/kernel/random/boot_id` ile karşılaştırır: `UntilReboot` veya `Timed` kayıtlarının `boot_id`'si farklıysa veya `expires_at` geçmişse geçersiz/süresi dolmuş sayar; gerçek çekirdek/servis durumu (`getenforce`, `firewalld`, `systemd-oomd`) daima birincil gerçektir (`single source of truth`).
- **Geri alma (`RollbackTransaction`) desteği:**
  - Her 6 yeni `ActionRequest` varyantı `pre_snapshot` kaydeder ve `RollbackTransaction` ile önceki duruma (önceki SELinux modu + `/etc/selinux/config` değeri, önceki `firewalld`/`oomd` enable/active durumu, önceki `nofile` drop-in varlığı, önceki klasör SELinux bağlamı) atomik geri döner.
- **Kırmızı çizgi doğrulaması (`transaction_engine.rs`):**
  - Mevcut `reject_forbidden_selinux_directives_in_json_value` metin alanlarında `audit2allow`, `semodule`, `selinux=disabled` yasaklarını %100 korur; `setenforce` kelimesinin yalnızca `ActionRequest::SelinuxSetMode` dışındaki serbest metin alanlarında yasak kalması sağlanır.

---

## 5. Arayüz Tasarımı (`blaze-akrep-ui`)

Tüm yeni bileşenler [2026-11-26-blaze-ui-unslop-design.md](2026-11-26-blaze-ui-unslop-design.md) kurallarına (Grafit + Blaze ember, tek birincil düğme, 4px ızgara, 8px durum noktası, 16px sembolik ikon, `changes-prevent-symbolic` ile sakin `Yönetici parolası istenir` notu, sıfır `(Polkit)`) uyar. Yeni sekme eklenmez (`nav_count: 10` değişmez).

1. **Genel Bakış (`overview.rs`) ve Üst Çubuk (`header_banner.rs`):**
   - Herhangi bir koruma duraklatılmış veya `Permissive` moddaysa Genel Bakış üst kartının hemen altında sakin bir **Etkin gevşetmeler** kartı görünür: alt sistem adı, amber nokta, kalan süre veya `Elle açılana kadar` bilgisi ve satır başına `Etkinleştir` düğmesi.
2. **Sistem Koruması (`system_protection.rs`):**
   - **SELinux modu kartı:** Mevcut durum göstergesinin yanına mod ve süre kontrolü eklenir:
     - `Enforcing` durumundayken: süre seçici (`15 dk`, `1 saat`, `Yeniden başlatmaya kadar`, `Ben tekrar değiştirene kadar`) + ikincil düğme `Permissive moduna geç` (önizleme diyaloğunda `Ben tekrar değiştirene kadar` seçiliyse açık yüksek-risk uyarısı gösterilir; `SELINUX=disabled` seçeneği yoktur).
     - `Permissive` durumundayken: kalan süre/mod satırı + kırık beyaz birincil düğme `Enforcing moduna dön` + ikincil süre değiştirme seçicisi (`Süreyi güncelle`).
   - **Geliştirici ve derleme uyumluluğu kartı (aynı sayfada):**
     - **Bellek yöneticisi (`systemd-oomd`):** Durum noktası (`Etkin` / `Duraklatıldı`), süre seçici (`1 saat`, `4 saat`, `Yeniden başlatmaya kadar`, `Ben tekrar değiştirene kadar`), `Duraklat` / `Etkinleştir` (duraklatılmışken `Süreyi güncelle` destekli).
     - **Açık dosya limiti (`nofile 65536`):** Mevcut durum (`Varsayılan` / `65536 / 524288 etkin`) + `Etkinleştir` / `Varsayılana dön` düğmesi.
     - **Konteyner / ROM derleme klasörü izni:** Klasör yolu girişi (örn. `~/android`) + özyinelemeli onay kutusu + `Etiket uygula` (`container_file_t`) ve `Varsayılan etikete dön` (`restorecon`) düğmeleri.
     - **Android geliştirme paketleri:** `android-udev-rules` (ADB/Fastboot) ve `ncurses-compat-libs` durum noktaları + eksikse `Kur` düğmesi.
3. **Güvenlik Duvarı (`firewall.rs`):**
   - **Güvenlik duvarı durumu kartı:**
     - `Etkin` durumundayken: süre seçici (`15 dk`, `1 saat`, `Yeniden başlatmaya kadar`, `Ben tekrar değiştirene kadar`) + ikincil `Duraklat` düğmesi.
     - `Duraklatıldı` / `Kapalı` durumundayken: kalan süre/mod bilgisi + kırık beyaz birincil `Etkinleştir` düğmesi + süre değiştirme seçicisi (`Süreyi güncelle`).
   - **Hazır şablonlar:** Mevcut `Minecraft`, `KDE Connect`, `SSH (yerel ağ)` yanına `Samba (yerel ağ)`, `Web geliştirme (3000/tcp)`, `Web geliştirme (8080/tcp)`, `Jellyfin (8096/tcp)` eklenir.
4. **Uygulama Güvenliği (`applications.rs`):**
   - Her Flatpak uygulamasının izin listesine **"Oyun kumandası (input)"** (`devices=input`) tek tık anahtarı eklenir.
   - Klasör izni ekleme satırının altına hızlı ekleme düğmeleri (`/mnt`, `/run/media`) eklenir (yönetici gerektirmeden kullanıcı Flatpak'larında anında çalışır).
5. **Cihaz Güvenliği (`device_security.rs`):**
   - Mevcut Aygıt Yazılımı kartının içine **Sürücü ve MOK uyumluluğu** satırları eklenir: Secure Boot açıkken kayıtlı MOK sayısı ve NVIDIA/üçüncü parti çekirdek modülü durumu + gerektiğinde sade `mokutil` MOK kayıt rehberi.

---

## 6. Test, Paketleme ve QEMU Doğrulama Planı

1. **Birim ve Entegrasyon Testleri (`cargo test --workspace` + `cargo clippy -p blaze-akrep-*`):**
   - `models.rs`: Yeni 6 `ActionRequest` için doğrulama, `SELINUX=disabled` reddi, yol kaçışı (`..`, `/`, `/home/<user>` kökü, `/etc`) reddi, `nofile` sınır kontrolü, `Timed { minutes: 0 | 2000 }` reddi, Polkit eşleme ve risk düzeyi testleri.
   - `blaze-akrep-privd`: Mock/tempdir üzerinde `SelinuxSetMode` (`/etc/selinux/config` atomik yazım + geri alma), `FirewallSetState`, `OomdSetState`, `DevLimitsSetNofile`, `SelinuxLabelContainerDir`, `overrides.json` + `active_overrides.json` generation-id yarış koruması ve rollback testleri.
   - `blaze-akrep-daemon`: `active_overrides.json` okuma, farklı `boot_id` durumunda `UntilReboot`/`Timed` düşürme, `UntilChanged` koruma testleri.
   - `blaze-akrep-ui`: `nav_count == 10` ve `--smoke-test` değişmezleri korunur.
2. **Audit Regresyonları (`tests/test_audit_regressions.py`):**
   - Yeni Polkit eylemi (`tr.com.blazeos.Akrep1.system.compat-manage`) ve mevcut tüm Akrep v1.0 değişmezleri doğrulanır (`selinux_required = true` profillerde korunur).
3. **Paketleme ve Zorunlu QEMU Doğrulaması (`AGENTS.md`):**
   - `--remap-path-prefix=${HOME}=/build` ile strip edilmiş `akrepd`, `akrep-privd`, `akrep-ui` ikilileri yenilenir (`0` `/home/` yolu).
   - `packaging/check_ownership.py`, `packaging/build-rpms.sh`, `packaging/verify-repo.sh` çalıştırılır.
   - Live ISO yeniden derlenir (`build_f45_final.sh`).
   - **QEMU Legacy BIOS + UEFI OVMF** `run_e2e --all --firmware both` (`79/79`) + canlı QEMU içinde Akrep yeni kontrollerinin (SELinux mod geçişi + geri dönüş, Firewalld duraklatma + geri dönüş, OOMD/nofile/klasör etiketi, Flatpak gamepad/disk, Overview etkin gevşetme kartı) gerçek çalıştırılması ve ekran görüntüsü doğrulaması yapılır.

## 7. Uygulayıcıya Bırakılan Ayrıntılar

- `systemd-run` veya `chcon`/`setenforce` sistem çağrılarının trait arkasında soyutlanması (mevcut `SelinuxOps` / `FirewallOps` kalıbına uygun olarak test edilebilirlik için).
- `akrepd` içindeki `system_compat` / `overrides` önbellek okuma aralığı (mevcut 2 sn snapshot yenileme döngüsüyle hizalanır).
- Yeni UI bileşenlerindeki birebir Türkçe/İngilizce metinler (`i18n.rs` ve `2026-11-26-blaze-ui-unslop-design.md` kurallarına uygun olarak).
