# Kaynaklar ve Lisanslar / Sources & Licenses

Bu belge `blazeos_custom_apps/` içindeki ikili dosyaların nereden geldiğini ve hangi lisansla dağıtıldığını kayıt altına alır. **Durum** sütunu dürüsttür: doğrulanmamış olanlar "Doğrulanacak" olarak işaretlidir.

GPL/LGPL bileşenler için kaynak kodu teklifi: upstream bağlantısı ve sürüm aşağıdadır. İstek üzerine tam kaynak ağacı `DarkMorpheus-pc` GitHub hesabından sağlanır.

## Kendi yazdığımız (Blaze) / First-party

| İkili | Kaynak | Lisans | Derleme |
|---|---|---|---|
| `blaze-sentinel` | [Blaze-Universe-/crates/blaze-sentinel](https://github.com/DarkMorpheus-pc/Blaze-Universe-) | MIT/GPL (depo `LICENSE`) | `cargo build --release -p blaze-sentinel` |
| `blaze-continuum` | Blaze-Universe-/crates/blaze-continuum | MIT/GPL | `cargo build --release -p blaze-continuum` |
| `blaze-recovery`, `solar-core` | [SolarUI](https://github.com/DarkMorpheus-pc/SolarUI) `crates/` | Depo lisansı | `cargo build --release` |
| `blaze-house` | [BlazeHouse-](https://github.com/DarkMorpheus-pc/BlazeHouse-) (`modern-ui` + `src-tauri`) | GPL-3.0 | `npm run build && cargo build --release` |
| `firehub` | [Blaze-Fire-Hub](https://github.com/DarkMorpheus-pc/Blaze-Fire-Hub) | Depo lisansı | `npm run dist` (Electron) |
| Python/Bash araçları (`blaze-setup`, `blaze-style-center`, `blaze-ghost-overlay`, `blaze-death-screen`, `berp-recovery-gui`, `blazeos-*`) | Bu depo | GPL-3.0 | Kaynak betik |

## Üçüncü taraf / Third-party

| İkili | Upstream | Lisans | Durum |
|---|---|---|---|
| `niri` | https://github.com/YaLTeR/niri (26.04 + BlazeOS `niri-src` minimize & We10X server-titlebar yamaları) | GPL-3.0 | Yerel `niri-src/` ağacından derlenmektedir |
| `noctalia`, `quickshell.bin`, `qs` | https://github.com/noctalia-dev/noctalia ve Quickshell | GPL-3.0 / LGPL-3.0 | Doğrulanacak (derleme kaynağı) |
| `caelestia` ve QML/C++ eklentileri | https://github.com/caelestia-dots/shell | GPL-3.0 | Yerel Türkçe yama ve yeniden derleme; yamalar `solarui/vendor/caelestia-shell` içinde |
| `we10x-shell` (`We10X` görsel varlıkları, duvar kâğıtları ve ikon teması; `sddm/` hariç) | https://github.com/yeyushengfan258/We10XOS-kde (`270f0d7`) ve https://github.com/yeyushengfan258/We10X-icon-theme (`0f52ff2`) | GPL-3.0 | Quickshell QML portu `solarui/vendor/we10x-shell`, ikonlar `usr/share/icons/We10X` içinde |
| `sddm`, `sddm-greeter-qt6` | https://github.com/sddm/sddm (0.21) | GPL-2.0+ | Doğrulanacak |
| `seatd`, `weston*` | https://sr.ht/~kennylevinsen/seatd, https://gitlab.freedesktop.org/wayland/weston | MIT | Doğrulanacak |
| `limine` | https://github.com/limine-bootloader/limine (12.9.0) | BSD-2-Clause | Resmi sürüm |
| `cliphist` | https://github.com/sentriz/cliphist | GPL-3.0 | Doğrulanacak |
| `protonup-qt` | https://github.com/DavidoTek/ProtonUp-Qt | GPL-3.0 | Doğrulanacak |
| Kitty | https://github.com/kovidgoyal/kitty | GPL-3.0 | Fedora paketinden |

## Bilinen risk / Known risk

Üçüncü taraf ikililerin bir kısmı Arch/CachyOS üzerinde derlenmiş görünmektedir (derleyici ve kütüphane dizeleri bunu gösteriyor). Fedora'da çalışmaları test edilmiştir, ancak **Fedora konteynerinde kaynaktan yeniden derleme** (RPM paketleme) tamamlanana kadar bu satırlar "Doğrulanacak" kalır.
