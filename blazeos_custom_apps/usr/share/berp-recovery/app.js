// BlazeOS BERE / BERP Recovery Frontend Controller
// Strict zero emoji rule enforced.

const TRANSLATIONS = {
    tr: {
        status_active: "Kurtarma modu etkin",
        nav_install: "ISO yükle",
        nav_backup: "Yedekle",
        nav_restore: "Geri yükle",
        nav_wipe: "Sıfırla",
        nav_partitions: "Diskler",
        nav_boot: "Önyükleme",
        nav_terminal: "Terminal",
        nav_reboot: "Yeniden başlat",

        install_title: "ISO kurulum ve kurtarma",
        install_desc: "USB belleklerdeki veya dahili diskteki ISO kalıplarını doğrudan başlatın ya da dahili kasaya kopyalayın",
        detected_isos: "Algılanan ISO kalıpları",
        btn_rescan: "Yeniden tara",
        scanning_isos: "ISO kalıpları taranıyor...",
        no_isos_found: "Sistemde ISO kalıbı bulunamadı. Downloads klasörünü veya harici diski kontrol edin",
        selected_iso_action: "Başlatma ayarları",
        slider_iso_desc: "Seçili kalıbı başlatmak için sürgüyü kaydırın",
        slider_iso_label: "Kaydırarak ISO başlat",
        iso_configured_msg: "ISO başlatma hazırlandı, yeniden başlatılıyor...",

        backup_title: "Anlık görüntü yedekleme",
        backup_desc: "Btrfs CoW ile diskinizde hızlı kurtarma noktası oluşturun",
        create_snapshot_title: "Yeni anlık görüntü",
        backup_name_label: "Yedek adı",
        backup_name_placeholder: "guncelleme-oncesi",
        backup_root_label: "Sistem kök dizini (/)",
        backup_home_label: "Kullanıcı verileri (/home)",
        btn_create_snapshot: "Anlık görüntü al",
        snapshot_creating: "Anlık görüntü alınıyor...",
        snapshot_success: "Kurtarma noktası kaydedildi",

        restore_title: "Sistem geri yükleme",
        restore_desc: "Kayıtlı bir kurtarma noktasına geri dönün",
        restore_points_title: "Kurtarma noktaları",
        btn_refresh_list: "Yenile",
        scanning_snapshots: "Kurtarma noktaları aranıyor...",
        no_snapshots_found: "Kayıtlı Btrfs anlık görüntüsü bulunamadı",
        slider_restore_desc: "Seçili yedeğe geri dönmek için kaydırın",
        slider_restore_label: "Kaydırarak geri yükle",
        restoring_msg: "Geri yükleniyor...",
        restore_success_msg: "Geri yüklendi, sistem yeniden başlatılıyor...",

        wipe_title: "Temizle ve sıfırla",
        wipe_desc: "Önbellekleri temizleyin veya sistemi sıfırlayın. İşlem yalnızca sürgü kaydırıldığında başlar",
        wipe_select_title: "Temizlenecek alanlar",
        wipe_cache_title: "Sistem ve paket önbellekleri (/var/cache)",
        wipe_cache_desc: "DNF, Flatpak ve geçici sistem dosyalarını temizler. Kullanıcı dosyalarına dokunmaz.",
        wipe_user_cache_title: "Kullanıcı önbellekleri (~/.cache)",
        wipe_user_cache_desc: "Tarayıcı ve masaüstü önbelleklerini temizler. Belgeleri ve parolaları korur.",
        wipe_home_title: "Kullanıcı hesapları ve verileri (/home)",
        wipe_home_desc: "Tüm kullanıcı belgeleri ve hesapları silinir. Sistem ilk kurulum ekranına (OOBE) döner.",
        wipe_factory_title: "Fabrika sıfırlaması",
        wipe_factory_desc: "Kullanıcı hesaplarını ve önbellekleri temizler, sistem kökünü başlangıç durumuna döndürür.",
        wipe_hard_reset_title: "Tam sistem temizliği (rootfs sil)",
        wipe_hard_reset_desc: "Kök dosya sistemini siler; /boot/bere ve Btrfs anlık görüntüleri korunur. Onay kodu ve parola gerekir.",
        wipe_slider_warn: "Seçilen alanlar kalıcı olarak silinir",
        wipe_slider_label: "Kaydırarak temizle",
        wipe_running: "Temizleniyor...",
        wipe_running_hard: "Kök dosya sistemi siliniyor (/boot/bere hariç)...",
        wipe_complete: "Temizleme tamamlandı",
        wipe_success_msg: "Temizleme tamamlandı",
        wipe_hard_reset_success: "Sistem temizlendi, kurtarma ortamı yeniden başlatılıyor...",
        wipe_select_warn: "En az bir alan seçin",

        modal_hard_reset_title: "Tam sistem temizliği onayı",
        modal_hard_reset_msg: "Bu işlem geri alınamaz",
        modal_hard_reset_submsg: "Sistem kök dizinindeki tüm dosyalar kalıcı olarak silinecektir. Devam etmek için SIFIRLA veya YES yazın ve yönetici parolanızı girin.",
        modal_confirm_label: "Onay kodu (SIFIRLA veya YES)",
        modal_pass_label: "Yönetici (wheel) veya root parolası",
        btn_cancel: "İptal",
        btn_confirm_wipe: "Onayla ve kilidi aç",
        modal_err_yes: "Onaylamak için SIFIRLA veya YES yazın",
        modal_err_pass: "Yönetici (wheel) veya root parolası gerekir",

        part_title: "Disk ve bölüm yönetimi",
        part_desc: "Bölüm tablosu (GPT/MBR), biçimlendirme, bağlama, dosya sistemi denetimi ve SMART sağlığı",
        mounted_parts_title: "Algılanan bölümler",
        btn_refresh_parts: "Yenile",
        th_device: "Aygıt",
        th_fs: "Dosya sistemi ve etiket",
        th_size: "Boyut",
        th_mount: "Bağlama noktası",
        th_action: "İşlemler",
        fetching_parts: "Disk bilgileri alınıyor...",
        no_parts_found: "Bölüm bulunamadı",
        btn_mount: "Bağla",
        btn_unmount: "Ayır",
        not_mounted: "Bağlı değil",
        smart_title: "Disk sağlığı (SMART / NVMe)",
        analyzing_smart: "Disk sağlık verileri okunuyor...",

        boot_title: "Önyükleme onarımı",
        boot_desc: "GRUB2 veya Limine yapılandırmasını yenileyin, EFI kayıtlarını düzeltin ve initramfs üretin",
        grub_card_title: "GRUB2 onar",
        grub_card_desc: "EFI/MBR ikililerini doğrular, /etc/grub.d menüsünü ve EFI yönlendiricisini yeniden üretir",
        btn_repair_grub: "GRUB2 onar",
        efi_card_title: "EFI NVRAM onar",
        efi_card_desc: "UEFI NVRAM tablosundaki yinelenen kayıtları temizler ve BlazeOS girişini öne alır",
        btn_repair_efi: "EFI onar",
        dracut_card_title: "Initramfs yeniden derle",
        dracut_card_desc: "Kurulu çekirdekler için donanım sürücülerini ve acil kurtarma modülünü içeren initramfs üretir",
        btn_repair_dracut: "Initramfs yenile",
        repair_log_title: "Onarım günlüğü",
        waiting_op: "İşlem bekleniyor...",

        term_title: "Root terminali",
        term_desc: "Bakım komutlarını doğrudan root yetkisiyle çalıştırın",
        term_placeholder: "Komut girin",
        btn_send: "Çalıştır",

        reboot_title: "Güç ve yeniden başlatma",
        reboot_desc: "Kurtarma ortamından güvenli çıkış yapın",
        btn_reboot_sys_title: "Sistemi başlat",
        btn_reboot_sys_desc: "BlazeOS masaüstüne dön",
        btn_reboot_rec_title: "Kurtarmayı yeniden başlat",
        btn_reboot_rec_desc: "BERE oturumunu yeniden aç",
        btn_reboot_uefi_title: "UEFI / BIOS ayarları",
        btn_reboot_uefi_desc: "Anakart donanım ekranına geç",
        btn_poweroff_title: "Bilgisayarı kapat",
        btn_poweroff_desc: "Sistemi güvenli şekilde kapat",

        confirm_reboot_sys: "BlazeOS normal masaüstüne yeniden başlatılsın mı?",
        confirm_reboot_rec: "Kurtarma ortamı yeniden başlatılsın mı?",
        confirm_reboot_uefi: "UEFI / BIOS ayarlarına yeniden başlatılsın mı?",
        confirm_poweroff: "Bilgisayar kapatılsın mı?"
    },
    en: {
        status_active: "Recovery mode active",
        nav_install: "Install ISO",
        nav_backup: "Backup",
        nav_restore: "Restore",
        nav_wipe: "Reset",
        nav_partitions: "Disks",
        nav_boot: "Boot repair",
        nav_terminal: "Terminal",
        nav_reboot: "Reboot",

        install_title: "ISO install and recovery",
        install_desc: "Boot ISO images from USB or internal storage, or copy them to the local vault",
        detected_isos: "Detected ISO images",
        btn_rescan: "Rescan",
        scanning_isos: "Scanning ISO images...",
        no_isos_found: "No ISO image found. Check the Downloads folder or an external drive",
        selected_iso_action: "Launch settings",
        slider_iso_desc: "Slide to boot the selected image",
        slider_iso_label: "Slide to boot ISO",
        iso_configured_msg: "ISO boot configured, rebooting...",

        backup_title: "Snapshot backup",
        backup_desc: "Create a Btrfs Copy-on-Write recovery point on disk",
        create_snapshot_title: "New snapshot",
        backup_name_label: "Snapshot name",
        backup_name_placeholder: "pre-update",
        backup_root_label: "System root (/)",
        backup_home_label: "User data (/home)",
        btn_create_snapshot: "Create snapshot",
        snapshot_creating: "Creating snapshot...",
        snapshot_success: "Recovery point saved",

        restore_title: "System rollback",
        restore_desc: "Revert to a previously saved recovery point",
        restore_points_title: "Restore points",
        btn_refresh_list: "Refresh",
        scanning_snapshots: "Scanning restore points...",
        no_snapshots_found: "No Btrfs snapshots found",
        slider_restore_desc: "Slide to restore the selected snapshot",
        slider_restore_label: "Slide to restore",
        restoring_msg: "Restoring...",
        restore_success_msg: "Restored, rebooting system...",

        wipe_title: "Wipe and reset",
        wipe_desc: "Clean caches or reset the system. Actions only start after sliding to confirm",
        wipe_select_title: "Areas to clean",
        wipe_cache_title: "System and package caches (/var/cache)",
        wipe_cache_desc: "Cleans DNF, Flatpak, and temporary files. Leaves user data intact.",
        wipe_user_cache_title: "User caches (~/.cache)",
        wipe_user_cache_desc: "Clears browser and desktop caches. Preserves personal documents and keys.",
        wipe_home_title: "User accounts and data (/home)",
        wipe_home_desc: "Deletes all user documents and accounts. System returns to first-boot setup (OOBE).",
        wipe_factory_title: "Factory reset",
        wipe_factory_desc: "Clears user accounts and caches, and returns the system root to initial state.",
        wipe_hard_reset_title: "Complete system wipe (erase rootfs)",
        wipe_hard_reset_desc: "Erases the root filesystem while preserving /boot/bere and Btrfs snapshots. Confirmation code and password required.",
        wipe_slider_warn: "Selected areas will be permanently erased",
        wipe_slider_label: "Slide to wipe",
        wipe_running: "Wiping...",
        wipe_running_hard: "Erasing rootfs (keeping /boot/bere)...",
        wipe_complete: "Wipe complete",
        wipe_success_msg: "Wipe completed",
        wipe_hard_reset_success: "System wiped, rebooting into recovery...",
        wipe_select_warn: "Select at least one area to clean",

        modal_hard_reset_title: "Confirm complete wipe",
        modal_hard_reset_msg: "This action cannot be undone",
        modal_hard_reset_submsg: "All operating system files in the root filesystem will be permanently erased. Type YES and enter your administrator password to continue.",
        modal_confirm_label: "Confirmation code (YES)",
        modal_pass_label: "Administrator (wheel) or root password",
        btn_cancel: "Cancel",
        btn_confirm_wipe: "Confirm and unlock",
        modal_err_yes: "Type YES to confirm",
        modal_err_pass: "Administrator (wheel) or root password is required",

        part_title: "Disk and partition management",
        part_desc: "Partition tables (GPT/MBR), formatting, mount controls, filesystem checks and SMART health",
        mounted_parts_title: "Detected partitions",
        btn_refresh_parts: "Refresh",
        th_device: "Device",
        th_fs: "Filesystem and label",
        th_size: "Size",
        th_mount: "Mount point",
        th_action: "Actions",
        fetching_parts: "Querying partition tables...",
        no_parts_found: "No partitions detected",
        btn_mount: "Mount",
        btn_unmount: "Unmount",
        not_mounted: "Not mounted",
        smart_title: "Drive health (SMART / NVMe)",
        analyzing_smart: "Reading drive health...",

        boot_title: "Boot repair",
        boot_desc: "Rebuild GRUB2 or Limine, restore EFI entries and regenerate initramfs",
        grub_card_title: "Repair GRUB2",
        grub_card_desc: "Verifies EFI/MBR binaries and regenerates /etc/grub.d entries and EFI stubs",
        btn_repair_grub: "Repair GRUB2",
        efi_card_title: "Repair EFI NVRAM",
        efi_card_desc: "Cleans duplicate UEFI NVRAM records and sets BlazeOS first in boot order",
        btn_repair_efi: "Repair EFI",
        dracut_card_title: "Rebuild initramfs",
        dracut_card_desc: "Regenerates initramfs images with hardware drivers and emergency recovery module",
        btn_repair_dracut: "Rebuild initramfs",
        repair_log_title: "Repair log",
        waiting_op: "Awaiting operation...",

        term_title: "Root terminal",
        term_desc: "Run maintenance commands directly with root privileges",
        term_placeholder: "Enter command",
        btn_send: "Run",

        reboot_title: "Power and reboot",
        reboot_desc: "Exit the recovery environment",
        btn_reboot_sys_title: "Boot system",
        btn_reboot_sys_desc: "Return to BlazeOS desktop",
        btn_reboot_rec_title: "Reboot recovery",
        btn_reboot_rec_desc: "Restart BERE recovery session",
        btn_reboot_uefi_title: "UEFI / BIOS setup",
        btn_reboot_uefi_desc: "Enter motherboard firmware settings",
        btn_poweroff_title: "Power off",
        btn_poweroff_desc: "Safely shut down the computer",

        confirm_reboot_sys: "Reboot into normal BlazeOS desktop?",
        confirm_reboot_rec: "Restart the recovery environment?",
        confirm_reboot_uefi: "Reboot into UEFI / BIOS setup?",
        confirm_poweroff: "Shut down the computer now?"
    }
};

let currentLang = "tr";

function t(key) {
    const dict = TRANSLATIONS[currentLang] || TRANSLATIONS.tr;
    return dict[key] || key;
}

function setLanguage(lang) {
    if (!TRANSLATIONS[lang]) lang = "tr";
    currentLang = lang;
    localStorage.setItem("berp_lang", lang);
    document.documentElement.lang = lang;

    const langBtnText = document.getElementById("lang-current");
    if (langBtnText) langBtnText.textContent = lang.toUpperCase();

    // Update data-i18n text
    document.querySelectorAll("[data-i18n]").forEach(el => {
        const key = el.getAttribute("data-i18n");
        if (TRANSLATIONS[lang][key]) {
            el.textContent = TRANSLATIONS[lang][key];
        }
    });

    // Update data-i18n-placeholder
    document.querySelectorAll("[data-i18n-placeholder]").forEach(el => {
        const key = el.getAttribute("data-i18n-placeholder");
        if (TRANSLATIONS[lang][key]) {
            el.placeholder = TRANSLATIONS[lang][key];
        }
    });
}

function initLanguage() {
    const saved = localStorage.getItem("berp_lang");
    if (saved && TRANSLATIONS[saved]) {
        currentLang = saved;
    }
    setLanguage(currentLang);

    const toggleBtn = document.getElementById("btn-lang-toggle");
    if (toggleBtn) {
        toggleBtn.addEventListener("click", () => {
            const nextLang = currentLang === "tr" ? "en" : "tr";
            setLanguage(nextLang);
        });
    }
}

// REST API Helper (Bootstraps token from localhost /api/bootstrap so token is never leaked in /proc/<pid>/cmdline)
let cachedApiToken = new URLSearchParams(window.location.search).get("token") || "";

async function ensureApiToken() {
    if (cachedApiToken) return cachedApiToken;
    try {
        const res = await fetch("/api/bootstrap", { method: "GET", cache: "no-store" });
        if (res.ok) {
            const payload = await res.json();
            if (payload && payload.token) {
                cachedApiToken = payload.token;
            }
        }
    } catch (e) {
        console.error("Bootstrap token fetch failed:", e);
    }
    return cachedApiToken;
}

let cachedAuthRequired = true;
let isSessionUnlocked = false;
let workspaceInitialized = false;

function showStartupLockOverlay(show, adminUsers = []) {
    const overlay = document.getElementById("bere-lock-overlay");
    const usersEl = document.getElementById("bere-lock-users");
    const errEl = document.getElementById("bere-lock-error");
    const pwInp = document.getElementById("bere-lock-password");
    if (!overlay) return;
    if (show) {
        if (usersEl) {
            usersEl.textContent = adminUsers.length
                ? `Yetkili hesaplar: ${adminUsers.join(", ")}`
                : "Yetkili yönetici hesabı doğrulanıyor...";
        }
        if (errEl) errEl.style.display = "none";
        overlay.style.display = "flex";
        setTimeout(() => pwInp && pwInp.focus(), 80);
    } else {
        overlay.style.display = "none";
    }
}

async function requestActionPassword(title, desc) {
    if (!cachedAuthRequired) {
        return "";
    }
    const modal = document.getElementById("modal-action-auth");
    const titleEl = document.getElementById("action-auth-title");
    const descEl = document.getElementById("action-auth-desc");
    const inp = document.getElementById("input-action-auth-pass");
    const errEl = document.getElementById("action-auth-error");
    const btnOk = document.getElementById("btn-action-auth-confirm");
    const btnCancel = document.getElementById("btn-action-auth-cancel");
    const btnClose = document.getElementById("btn-action-auth-close");

    if (!modal || !inp || !btnOk) return null;

    if (titleEl && title) titleEl.textContent = title;
    if (descEl && desc) descEl.textContent = desc;
    inp.value = "";
    if (errEl) {
        errEl.style.display = "none";
        errEl.textContent = "";
    }
    modal.style.display = "flex";
    setTimeout(() => inp.focus(), 60);

    return new Promise((resolve) => {
        const cleanup = () => {
            modal.style.display = "none";
            btnOk.removeEventListener("click", onConfirm);
            btnCancel && btnCancel.removeEventListener("click", onCancel);
            btnClose && btnClose.removeEventListener("click", onCancel);
            inp.removeEventListener("keydown", onKey);
        };
        const onCancel = () => {
            cleanup();
            resolve(null);
        };
        const onConfirm = async () => {
            const pw = inp.value;
            if (!pw) {
                if (errEl) {
                    errEl.textContent = "Yönetici veya root parolası gerekir";
                    errEl.style.display = "block";
                }
                inp.focus();
                return;
            }
            btnOk.disabled = true;
            const checkRes = await apiCall("auth_unlock", { password: pw });
            btnOk.disabled = false;
            if (checkRes.status !== "ok") {
                if (errEl) {
                    errEl.textContent = checkRes.message || "Hatalı parola";
                    errEl.style.display = "block";
                }
                inp.value = "";
                inp.focus();
                return;
            }
            cleanup();
            resolve(pw);
        };
        const onKey = (ev) => {
            if (ev.key === "Enter") {
                ev.preventDefault();
                void onConfirm();
            } else if (ev.key === "Escape") {
                ev.preventDefault();
                onCancel();
            }
        };
        btnOk.addEventListener("click", onConfirm);
        btnCancel && btnCancel.addEventListener("click", onCancel);
        btnClose && btnClose.addEventListener("click", onCancel);
        inp.addEventListener("keydown", onKey);
    });
}

async function apiCall(endpoint, data = null) {
    try {
        const token = await ensureApiToken();
        const options = {
            method: data ? "POST" : "GET",
            headers: { "Content-Type": "application/json", "X-BERP-Token": token }
        };
        if (data) options.body = JSON.stringify(data);
        const res = await fetch("/api/" + endpoint, options);
        const payload = await res.json().catch(() => ({}));
        if (!res.ok) {
            if (res.status === 401 || payload.status === "locked") {
                isSessionUnlocked = false;
                showStartupLockOverlay(true);
            }
            throw new Error(payload.message || `HTTP ${res.status}`);
        }
        return payload;
    } catch (e) {
        console.error("API call error:", e);
        return { status: "error", message: e.toString() };
    }
}

// 1. Tab Navigation
const TAB_ORDER = [
    "tab-install",
    "tab-backup",
    "tab-restore",
    "tab-wipe",
    "tab-partitions",
    "tab-boot",
    "tab-terminal",
    "tab-reboot"
];

function switchTab(target) {
    const navItems = document.querySelectorAll(".nav-item");
    const tabPanes = document.querySelectorAll(".tab-pane");

    navItems.forEach(n => {
        if (n.getAttribute("data-tab") === target) {
            n.classList.add("active");
        } else {
            n.classList.remove("active");
        }
    });

    tabPanes.forEach(p => {
        if (p.id === target) {
            p.classList.add("active");
        } else {
            p.classList.remove("active");
        }
    });

    if (target === "tab-install") {
        loadIsos();
        loadUsbDevices();
        loadIsoTargets();
    }
    if (target === "tab-restore") loadSnapshots();
    if (target === "tab-partitions") loadPartitions();
}

function initTabs() {
    const navItems = document.querySelectorAll(".nav-item");
    navItems.forEach(btn => {
        btn.addEventListener("click", () => {
            const target = btn.getAttribute("data-tab");
            switchTab(target);
        });
    });

    window.addEventListener("keydown", (e) => {
        const isInput = e.target && (e.target.tagName === "INPUT" || e.target.tagName === "TEXTAREA" || e.target.tagName === "SELECT");
        if (isInput) return;

        // Number keys 1-8
        const num = parseInt(e.key, 10);
        if (num >= 1 && num <= TAB_ORDER.length) {
            e.preventDefault();
            switchTab(TAB_ORDER[num - 1]);
            return;
        }

        // Arrow keys Up / Down
        const activeNav = document.querySelector(".nav-item.active");
        const currentTarget = activeNav ? activeNav.getAttribute("data-tab") : TAB_ORDER[0];
        const curIdx = TAB_ORDER.indexOf(currentTarget);

        if (e.key === "ArrowUp") {
            e.preventDefault();
            const prevIdx = (curIdx - 1 + TAB_ORDER.length) % TAB_ORDER.length;
            switchTab(TAB_ORDER[prevIdx]);
        } else if (e.key === "ArrowDown") {
            e.preventDefault();
            const nextIdx = (curIdx + 1) % TAB_ORDER.length;
            switchTab(TAB_ORDER[nextIdx]);
        }
    });
}

// 2. Hardware Telemetry & Clock
function initTelemetry() {
    const clockEl = document.getElementById("clock-val");
    const cpuEl = document.getElementById("cpu-val");
    const ramEl = document.getElementById("ram-val");

    function updateClock() {
        const d = new Date();
        const hh = String(d.getHours()).padStart(2, "0");
        const mm = String(d.getMinutes()).padStart(2, "0");
        if (clockEl) clockEl.textContent = `${hh}:${mm}`;
    }
    updateClock();
    setInterval(updateClock, 1000);

    async function pollTelemetry() {
        const res = await apiCall("telemetry");
        if (res.status === "ok") {
            if (cpuEl) cpuEl.textContent = res.cpu + "%";
            if (ramEl) ramEl.textContent = res.ram + "%";
        }
    }
    pollTelemetry();
    setInterval(pollTelemetry, 3000);
}

// 3. Touch/Mouse/Keyboard Confirmation Slider Engine
function initSliders() {
    setupSlider("slider-iso-boot", onConfirmIsoBoot);
    setupSlider("slider-restore", onConfirmRestore);
    setupSlider("slider-wipe", onConfirmWipe);
}

function setupSlider(sliderId, onComplete) {
    const slider = document.getElementById(sliderId);
    if (!slider) return;

    const thumb = slider.querySelector(".slider-thumb");
    const fill = slider.querySelector(".slider-fill");

    let isDragging = false;
    let startX = 0;
    let currentX = 0;
    let maxDist = 0;

    slider.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " " || e.key === "ArrowRight") {
            e.preventDefault();
            const max = slider.offsetWidth - thumb.offsetWidth - 8;
            thumb.style.transition = "transform 240ms cubic-bezier(0.22, 1, 0.36, 1)";
            fill.style.transition = "width 240ms cubic-bezier(0.22, 1, 0.36, 1)";
            thumb.style.transform = `translateX(${max}px)`;
            fill.style.width = "100%";
            setTimeout(() => {
                thumb.style.transition = "";
                fill.style.transition = "";
                onComplete(slider);
            }, 280);
        }
    });

    function onPointerDown(e) {
        isDragging = true;
        startX = e.clientX || (e.touches && e.touches[0].clientX);
        maxDist = slider.offsetWidth - thumb.offsetWidth - 8;
        if (slider.setPointerCapture && e.pointerId) {
            try { slider.setPointerCapture(e.pointerId); } catch(err) {}
        }
    }

    function onPointerMove(e) {
        if (!isDragging) return;
        const clientX = e.clientX || (e.touches && e.touches[0].clientX);
        let delta = clientX - startX;
        delta = Math.max(0, Math.min(delta, maxDist));
        currentX = delta;

        thumb.style.transform = `translateX(${delta}px)`;
        const pct = (delta / maxDist) * 100;
        fill.style.width = `${pct}%`;

        if (delta >= maxDist - 5) {
            isDragging = false;
            thumb.style.transform = `translateX(${maxDist}px)`;
            fill.style.width = "100%";
            onComplete(slider);
        }
    }

    function onPointerUp() {
        if (!isDragging) return;
        isDragging = false;
        thumb.style.transition = "transform 180ms cubic-bezier(0.22, 1, 0.36, 1)";
        fill.style.transition = "width 180ms cubic-bezier(0.22, 1, 0.36, 1)";
        thumb.style.transform = "translateX(0px)";
        fill.style.width = "0%";
        setTimeout(() => {
            thumb.style.transition = "";
            fill.style.transition = "";
        }, 200);
    }

    slider.addEventListener("mousedown", onPointerDown);
    window.addEventListener("mousemove", onPointerMove);
    window.addEventListener("mouseup", onPointerUp);

    slider.addEventListener("touchstart", onPointerDown, { passive: true });
    window.addEventListener("touchmove", onPointerMove, { passive: true });
    window.addEventListener("touchend", onPointerUp);
}

function resetSlider(slider) {
    if (!slider) return;
    const thumb = slider.querySelector(".slider-thumb");
    const fill = slider.querySelector(".slider-fill");
    thumb.style.transition = "transform 220ms cubic-bezier(0.22, 1, 0.36, 1)";
    fill.style.transition = "width 220ms cubic-bezier(0.22, 1, 0.36, 1)";
    thumb.style.transform = "translateX(0px)";
    fill.style.width = "0%";
    setTimeout(() => {
        thumb.style.transition = "";
        fill.style.transition = "";
    }, 240);
}

// 4. ISO Install, USB File Explorer & Pull-to-Vault
let selectedIsoPath = null;
let selectedIsoObj = null;
let selectedUsbDevice = null;
let currentUsbSubpath = "";
let parentUsbSubpath = "";
let selectedUsbIso = null;
let pullPollTimer = null;
let recommendRamDefaultApplied = false;

function initIsos() {
    const btn = document.getElementById("btn-refresh-isos");
    if (btn) btn.addEventListener("click", loadIsos);

    const btnUsb = document.getElementById("btn-refresh-usb");
    if (btnUsb) {
        btnUsb.addEventListener("click", () => {
            loadUsbDevices();
            loadIsoTargets();
        });
    }

    const btnUsbUp = document.getElementById("btn-usb-up");
    if (btnUsbUp) {
        btnUsbUp.addEventListener("click", () => {
            if (selectedUsbDevice) {
                browseUsbDevice(selectedUsbDevice, parentUsbSubpath || "");
            }
        });
    }

    const btnPull = document.getElementById("btn-pull-iso");
    if (btnPull) {
        btnPull.addEventListener("click", startIsoPull);
    }

    const btnCancelPull = document.getElementById("btn-cancel-pull");
    if (btnCancelPull) {
        btnCancelPull.addEventListener("click", async () => {
            await apiCall("iso_pull_cancel", { cancel: true });
        });
    }

    const btnBootUsbDirect = document.getElementById("btn-boot-usb-direct");
    if (btnBootUsbDirect) {
        btnBootUsbDirect.addEventListener("click", () => {
            if (selectedUsbIso && selectedUsbIso.full_path) {
                selectIsoForLaunch({
                    name: selectedUsbIso.name,
                    path: selectedUsbIso.full_path,
                    size_mb: selectedUsbIso.size_mb,
                    can_pull: true,
                    kind: selectedUsbIso.full_path.startsWith("media://") ? "media" : "iso",
                    source: selectedUsbIso.source || "usb",
                });
            }
        });
    }

    const btnCardPull = document.getElementById("btn-iso-card-pull");
    if (btnCardPull) {
        btnCardPull.addEventListener("click", () => {
            if (!selectedIsoObj) return;
            selectedUsbIso = {
                name: selectedIsoObj.name,
                full_path: selectedIsoObj.path,
                size_mb: selectedIsoObj.size_mb || 0,
                source: selectedIsoObj.source || "optical",
            };
            const lbl = document.getElementById("usb-selected-iso-label");
            const pullPanel = document.getElementById("usb-pull-panel");
            if (lbl) {
                lbl.innerHTML = `<strong>${escapeHtml(selectedUsbIso.name)}</strong> (${selectedUsbIso.size_mb} MB)<br><small>${escapeHtml(selectedUsbIso.full_path)}</small>`;
            }
            if (pullPanel) pullPanel.style.display = "block";
            void startIsoPull();
        });
    }

    loadIsos();
    loadUsbDevices();
    loadIsoTargets();

    const btnVaultReinstall = document.getElementById("btn-vault-reinstall");
    if (btnVaultReinstall) {
        btnVaultReinstall.addEventListener("click", () => void triggerVaultBoot("install"));
    }
    const btnVaultLive = document.getElementById("btn-vault-live");
    if (btnVaultLive) {
        btnVaultLive.addEventListener("click", () => void triggerVaultBoot("live"));
    }
}

async function triggerVaultBoot(profile) {
    const labelTxt = profile === "install"
        ? "BLAZE_REC kasasından USB olmadan Anaconda kurulumu (kexec) başlatılsın mı?"
        : "BLAZE_REC kasasından canlı Blaze SolarEvolution masaüstü (kexec) başlatılsın mı?";
    const pw = await requestActionPassword("Dahili kasa doğrulaması", labelTxt);
    if (pw === null) return;
    const ramChk = document.getElementById("iso-copy-to-ram");
    const copyToRam = ramChk ? ramChk.checked : false;
    const res = await apiCall("vault_verify_and_boot", {
        profile,
        copy_to_ram: copyToRam,
        password: pw,
    });
    if (res.status !== "ok") {
        alert("Hata: " + (res.message || "BLAZE_REC başlatılamadı"));
    }
}

async function loadIsoTargets() {
    const sel = document.getElementById("iso-pull-target");
    if (!sel) return;
    const prev = sel.value;
    const res = await apiCall("iso_targets");
    sel.innerHTML = "";
    if (!res.targets || res.targets.length === 0) {
        const opt = document.createElement("option");
        opt.value = "";
        opt.textContent = currentLang === "tr"
            ? "Yazılabilir dahili bölüm bulunamadı (önce Diskler sekmesinden bölüm oluşturun)"
            : "No writable internal partition found";
        sel.appendChild(opt);
        return;
    }
    res.targets.forEach(tg => {
        const opt = document.createElement("option");
        opt.value = tg.dest_dir;
        opt.textContent = tg.label;
        sel.appendChild(opt);
    });
    if (prev && Array.from(sel.options).some(o => o.value === prev)) {
        sel.value = prev;
    }
}

async function loadUsbDevices() {
    const container = document.getElementById("usb-device-list");
    if (!container) return;
    container.innerHTML = `<div class='empty-hint'>${currentLang === "tr" ? "USB ve harici diskler taranıyor..." : "Scanning USB and external drives..."}</div>`;
    const res = await apiCall("usb_devices");
    if (!res.devices || res.devices.length === 0) {
        container.innerHTML = `<div class='empty-hint'>${currentLang === "tr" ? "Takılı USB veya harici bölüm bulunamadı. Aygıtı bağlayıp 'USB tara' düğmesine basın" : "No USB flash drive or external partition detected"}</div>`;
        return;
    }

    container.innerHTML = "";
    res.devices.forEach(dev => {
        const row = document.createElement("div");
        row.className = "list-row-item" + (selectedUsbDevice === dev.device ? " selected" : "");
        row.innerHTML = `
            <div class="item-left">
                <span class="item-title">${escapeHtml(dev.label)} <span class="tag-badge">${escapeHtml(dev.badge)}</span></span>
                <span class="item-meta">${escapeHtml(dev.device)} | ${escapeHtml(dev.fstype.toUpperCase())} | ${dev.size_gb} GB | ${escapeHtml(dev.model)}</span>
            </div>
            <button class="fox-btn outline-btn fox-btn-sm" type="button">${currentLang === "tr" ? "Aç" : "Browse"}</button>
        `;
        row.addEventListener("click", () => {
            selectedUsbDevice = dev.device;
            document.querySelectorAll("#usb-device-list .list-row-item").forEach(r => r.classList.remove("selected"));
            row.classList.add("selected");
            if (dev.is_live_media && dev.media_ref) {
                selectedUsbIso = {
                    name: `${dev.label || "BlazeOS-Media"}.iso`,
                    full_path: dev.media_ref,
                    size_mb: Math.round((dev.size_gb || 0) * 1024),
                    source: dev.kind || "optical",
                };
                const pullPanel = document.getElementById("usb-pull-panel");
                const lbl = document.getElementById("usb-selected-iso-label");
                if (lbl) {
                    lbl.innerHTML = `<strong>${escapeHtml(selectedUsbIso.name)}</strong> (${escapeHtml(dev.badge)})<br><small>${escapeHtml(selectedUsbIso.full_path)}</small>`;
                }
                if (pullPanel) pullPanel.style.display = "block";
                void loadIsoTargets();
            }
            browseUsbDevice(dev.device, "");
        });
        container.appendChild(row);
    });
}

async function browseUsbDevice(device, subpath) {
    const box = document.getElementById("usb-browser-box");
    const listEl = document.getElementById("usb-entry-list");
    const pathEl = document.getElementById("usb-current-path");
    const upBtn = document.getElementById("btn-usb-up");
    if (!box || !listEl) return;

    box.style.display = "block";
    listEl.innerHTML = `<div class='empty-hint'>${currentLang === "tr" ? "Klasör okunuyor..." : "Reading directory..."}</div>`;
    const res = await apiCall("usb_browse", { device, subpath });
    if (res.status !== "ok") {
        listEl.innerHTML = `<div class='empty-hint'>Hata: ${escapeHtml(res.message || "Okunamadı")}</div>`;
        return;
    }

    currentUsbSubpath = res.current_subpath || "";
    parentUsbSubpath = res.parent_subpath || "";
    if (pathEl) pathEl.textContent = `${device}:/${currentUsbSubpath}`;
    if (upBtn) upBtn.disabled = !currentUsbSubpath;

    if (!res.entries || res.entries.length === 0) {
        listEl.innerHTML = `<div class='empty-hint'>${currentLang === "tr" ? "Bu klasörde alt dizin veya .iso dosyası yok" : "No folders or .iso files in this directory"}</div>`;
        return;
    }

    listEl.innerHTML = "";
    res.entries.forEach(entry => {
        const row = document.createElement("div");
        row.className = "list-row-item";
        if (entry.type === "dir") {
            row.innerHTML = `
                <div class="item-left">
                    <span class="item-title">${escapeHtml(entry.name)} <span class="tag-badge">${currentLang === "tr" ? "Klasör" : "Folder"}</span></span>
                    <span class="item-meta">/${escapeHtml(entry.subpath)}</span>
                </div>
                <button class="fox-btn outline-btn fox-btn-sm" type="button">${currentLang === "tr" ? "Aç" : "Open"}</button>
            `;
            row.addEventListener("click", () => browseUsbDevice(device, entry.subpath));
        } else {
            row.innerHTML = `
                <div class="item-left">
                    <span class="item-title">${escapeHtml(entry.name)} <span class="tag-badge">ISO</span></span>
                    <span class="item-meta">${entry.size_mb} MB | /${escapeHtml(entry.subpath)}</span>
                </div>
                <button class="fox-btn primary-btn fox-btn-sm" type="button">${currentLang === "tr" ? "Seç" : "Select"}</button>
            `;
            row.addEventListener("click", () => {
                document.querySelectorAll("#usb-entry-list .list-row-item").forEach(r => r.classList.remove("selected"));
                row.classList.add("selected");
                selectedUsbIso = entry;
                const pullPanel = document.getElementById("usb-pull-panel");
                const lbl = document.getElementById("usb-selected-iso-label");
                if (lbl) {
                    lbl.innerHTML = `<strong>${escapeHtml(entry.name)}</strong> (${entry.size_mb} MB)<br><small>${escapeHtml(entry.full_path)}</small>`;
                }
                if (pullPanel) pullPanel.style.display = "block";
                loadIsoTargets();
            });
        }
        listEl.appendChild(row);
    });
}

async function startIsoPull() {
    if (!selectedUsbIso || !selectedUsbIso.full_path) return;
    const targetSel = document.getElementById("iso-pull-target");
    const destDir = targetSel ? targetSel.value : "";
    if (!destDir) {
        alert(currentLang === "tr" ? "Hedef dahili bölümü seçin" : "Select a destination partition");
        return;
    }

    const btnPull = document.getElementById("btn-pull-iso");
    const btnCancel = document.getElementById("btn-cancel-pull");
    const progWrap = document.getElementById("iso-pull-progress-wrap");
    const progFill = document.getElementById("iso-pull-progress-fill");
    const progTxt = document.getElementById("iso-pull-progress-text");

    if (btnPull) btnPull.disabled = true;
    const res = await apiCall("iso_pull", { src_path: selectedUsbIso.full_path, dest_dir: destDir });
    if (res.status !== "ok") {
        if (btnPull) btnPull.disabled = false;
        alert("Hata: " + (res.message || "Aktarım başlatılamadı"));
        return;
    }

    if (progWrap) progWrap.style.display = "block";
    if (btnCancel) btnCancel.style.display = "inline-block";
    if (progFill) progFill.style.width = "1%";
    if (progTxt) progTxt.textContent = "Başlatılıyor...";

    if (pullPollTimer) clearInterval(pullPollTimer);
    pullPollTimer = setInterval(async () => {
        const st = await apiCall("iso_pull_status");
        if (st.status !== "ok") return;
        const doneMb = Math.round((st.bytes_done || 0) / (1024 * 1024));
        const totalMb = Math.round((st.bytes_total || 0) / (1024 * 1024));
        const pct = st.percent || 0;
        if (progFill) progFill.style.width = `${pct}%`;
        if (progTxt) {
            progTxt.textContent = st.active
                ? `%${pct} (${doneMb} / ${totalMb} MB, ${st.speed_mbps || 0} MB/sn) | ${st.message || ""}`
                : (st.message || "");
        }
        if (!st.active) {
            clearInterval(pullPollTimer);
            pullPollTimer = null;
            if (btnPull) btnPull.disabled = false;
            if (btnCancel) btnCancel.style.display = "none";
            await loadIsos();
            await loadIsoTargets();
            if (st.dest_path && pct === 100) {
                selectIsoForLaunch({
                    name: st.filename || st.dest_path.split("/").pop(),
                    path: st.dest_path,
                    size_mb: totalMb,
                    source: "vault",
                });
            }
        }
    }, 450);
}

async function selectIsoForLaunch(iso) {
    selectedIsoPath = iso.path;
    selectedIsoObj = iso;
    const metaEl = document.getElementById("selected-iso-meta");
    const actCard = document.getElementById("iso-action-card");
    const pullBar = document.getElementById("selected-iso-pull-bar");
    if (actCard) actCard.style.display = "block";
    if (pullBar) {
        pullBar.style.display = (iso.can_pull || iso.kind === "media") ? "flex" : "none";
    }
    if (metaEl) {
        metaEl.innerHTML = `<strong>${escapeHtml(iso.display_name || iso.name)}</strong> (${iso.size_mb} MB)<br><small>${escapeHtml(iso.path)}</small>`;
    }
    const probe = await apiCall("iso_inspect", { path: iso.path });
    if (probe.status === "ok" && probe.info && metaEl && selectedIsoPath === iso.path) {
        const inf = probe.info;
        const badgeTxt = inf.supported ? inf.distro_label : "Desteklenmeyen yapı";
        const lblExtra = inf.cd_label ? ` | CDLABEL: ${escapeHtml(inf.cd_label)}` : "";
        metaEl.innerHTML = `<strong>${escapeHtml(iso.display_name || iso.name)}</strong> <span class="tag-badge">${escapeHtml(badgeTxt)}</span><br><small>${inf.size_mb || iso.size_mb} MB${lblExtra} | ${escapeHtml(iso.path)}</small>`;
    }
}

async function loadIsos() {
    const container = document.getElementById("iso-list");
    if (!container) return;
    container.innerHTML = `<div class='empty-hint'>${escapeHtml(t("scanning_isos"))}</div>`;
    const res = await apiCall("isos");

    const ramChk = document.getElementById("iso-copy-to-ram");
    if (ramChk && !recommendRamDefaultApplied && res.recommend_ram_boot) {
        ramChk.checked = true;
        recommendRamDefaultApplied = true;
    }

    const vaultBanner = document.getElementById("bere-vault-banner");
    if (vaultBanner && res.vault && res.vault.present) {
        vaultBanner.style.display = "block";
        const desc = document.getElementById("bere-vault-desc");
        const actions = document.getElementById("bere-vault-actions");
        const badge = document.getElementById("bere-vault-badge");
        const szGb = ((res.vault.squashfs_mb || 0) / 1024).toFixed(2);
        if (res.vault.full_os_embedded) {
            if (badge) badge.textContent = "Tam imaj · SHA-256";
            if (desc) {
                desc.textContent = `Aygıt: ${res.vault.device} (${szGb} GiB). Blaze SolarEvolution kurulum imajı gömülü; USB olmadan kexec ile kurulum veya canlı masaüstü başlatılabilir`;
            }
            if (actions) actions.style.display = "flex";
        } else {
            if (badge) badge.textContent = "Minimal BERE";
            if (desc) {
                desc.textContent = `Aygıt: ${res.vault.device} (${res.vault.squashfs_mb || 0} MB). Minimal BERE kurtarma ortamı etkin; tam kurulum imajı gömülü değil`;
            }
            if (actions) actions.style.display = "none";
        }
    } else if (vaultBanner) {
        vaultBanner.style.display = "none";
    }

    if (!res.isos || res.isos.length === 0) {
        container.innerHTML = `<div class='empty-hint'>${escapeHtml(t("no_isos_found"))}</div>`;
        return;
    }

    container.innerHTML = "";
    res.isos.forEach(iso => {
        const item = document.createElement("div");
        item.className = "list-row-item" + (selectedIsoPath === iso.path ? " selected" : "");
        let srcBadge = "";
        if (iso.source === "armored_vault") {
            srcBadge = `<span class="tag-badge">Dahili kasa</span>`;
        } else if (iso.source === "vault") {
            srcBadge = `<span class="tag-badge">Kasa</span>`;
        } else if (iso.source === "optical") {
            srcBadge = `<span class="tag-badge">DVD/ISO</span>`;
        } else if (iso.source === "usb") {
            srcBadge = `<span class="tag-badge">USB</span>`;
        } else {
            srcBadge = `<span class="tag-badge">Disk</span>`;
        }
        const delBtnHtml = iso.managed
            ? `<button class="fox-btn danger-btn fox-btn-sm btn-iso-del" type="button">${currentLang === "tr" ? "Sil" : "Delete"}</button>`
            : "";
        item.innerHTML = `
            <div class="item-left">
                <span class="item-title">${escapeHtml(iso.display_name || iso.name)} ${srcBadge}</span>
                <span class="item-meta">${iso.size_mb} MB | ${escapeHtml(iso.path)}</span>
            </div>
            <div class="inline-row">
                <button class="fox-btn outline-btn fox-btn-sm" type="button">${currentLang === "tr" ? "Seç" : "Select"}</button>
                ${delBtnHtml}
            </div>
        `;
        item.addEventListener("click", () => {
            document.querySelectorAll("#iso-list .list-row-item").forEach(r => r.classList.remove("selected"));
            item.classList.add("selected");
            selectIsoForLaunch(iso);
        });
        const delBtn = item.querySelector(".btn-iso-del");
        if (delBtn) {
            delBtn.addEventListener("click", async (ev) => {
                ev.stopPropagation();
                const pw = await requestActionPassword(
                    "ISO kalıbını sil",
                    `${iso.name} kalıbını .blaze-isos kasasından kalıcı olarak silmek için parolanızı girin`
                );
                if (pw === null) return;
                delBtn.disabled = true;
                const dr = await apiCall("iso_delete", { path: iso.path, password: pw });
                if (dr.status !== "ok") {
                    alert("Hata: " + (dr.message || "Silinemedi"));
                    delBtn.disabled = false;
                    return;
                }
                if (selectedIsoPath === iso.path) {
                    selectedIsoPath = null;
                    selectedIsoObj = null;
                    const actCard = document.getElementById("iso-action-card");
                    if (actCard) actCard.style.display = "none";
                }
                loadIsos();
                loadIsoTargets();
            });
        }
        container.appendChild(item);
    });
}

async function onConfirmIsoBoot(slider) {
    if (!selectedIsoPath) {
        alert(currentLang === "tr" ? "Bir ISO dosyası seçin" : "Select an ISO file");
        resetSlider(slider);
        return;
    }

    const profSel = document.getElementById("iso-boot-profile");
    const engSel = document.getElementById("iso-boot-engine");
    const ramChk = document.getElementById("iso-copy-to-ram");
    const profile = profSel ? profSel.value : "live";
    const engine = engSel ? engSel.value : "auto";
    const copyToRam = ramChk ? ramChk.checked : false;

    const label = slider.querySelector(".slider-label");
    label.textContent = t("iso_configured_msg");

    const res = await apiCall("iso_boot", {
        path: selectedIsoPath,
        profile,
        engine,
        copy_to_ram: copyToRam,
    });
    if (res.status === "ok") {
        if (res.message) label.textContent = res.message;
        if (!res.auto_transition) {
            setTimeout(() => {
                apiCall("reboot", { target: "system" });
            }, 1500);
        }
    } else {
        alert("Hata: " + res.message);
        label.textContent = t("slider_iso_label");
        resetSlider(slider);
    }
}

// 5. Backup / Snapshot Management
let selectedSnapshot = null;

function initSnapshots() {
    const btnCreate = document.getElementById("btn-create-snapshot");
    const statusEl = document.getElementById("backup-status-text");

    if (btnCreate) {
        btnCreate.addEventListener("click", async () => {
            const name = document.getElementById("backup-name").value.trim();
            const root = document.getElementById("chk-backup-root").checked;
            const home = document.getElementById("chk-backup-home").checked;

            btnCreate.disabled = true;
            statusEl.textContent = t("snapshot_creating");

            const res = await apiCall("create_snapshot", { name, root, home });
            btnCreate.disabled = false;

            if (res.status === "ok") {
                statusEl.textContent = t("snapshot_success");
                document.getElementById("backup-name").value = "";
                loadSnapshots();
            } else {
                statusEl.textContent = "Hata: " + res.message;
            }
        });
    }

    const btnRefresh = document.getElementById("btn-refresh-snapshots");
    if (btnRefresh) btnRefresh.addEventListener("click", loadSnapshots);
    loadSnapshots();
}

async function loadSnapshots() {
    const container = document.getElementById("snapshot-list");
    if (!container) return;
    container.innerHTML = `<div class='empty-hint'>${escapeHtml(t("scanning_snapshots"))}</div>`;
    const res = await apiCall("snapshots");

    if (!res.snapshots || res.snapshots.length === 0) {
        container.innerHTML = `<div class='empty-hint'>${escapeHtml(t("no_snapshots_found"))}</div>`;
        const sliderBox = document.getElementById("restore-slider-box");
        if (sliderBox) sliderBox.style.display = "none";
        return;
    }

    container.innerHTML = "";
    res.snapshots.forEach(s => {
        const row = document.createElement("div");
        row.className = "list-row-item";
        row.innerHTML = `
            <div class="item-left">
                <span class="item-title">${escapeHtml(s.name)}</span>
                <span class="item-meta">${currentLang === "tr" ? "Tarih" : "Date"}: ${escapeHtml(s.date)} | ${escapeHtml(s.path)}</span>
            </div>
            <div class="inline-row">
                <button class="fox-btn outline-btn btn-snap-select" type="button">${currentLang === "tr" ? "Seç" : "Select"}</button>
                <button class="fox-btn outline-btn fox-btn-sm btn-snap-delete" type="button">${currentLang === "tr" ? "Sil" : "Delete"}</button>
            </div>
        `;
        const selectSnap = () => {
            document.querySelectorAll("#snapshot-list .list-row-item").forEach(r => r.classList.remove("selected"));
            row.classList.add("selected");
            selectedSnapshot = s;
            const targetLabel = document.getElementById("restore-target-label");
            if (targetLabel) {
                targetLabel.textContent = `"${s.name}" ${currentLang === "tr" ? "yedeğine geri dönmek için kaydırın" : "snapshot restore"}`;
            }
            const sliderBox = document.getElementById("restore-slider-box");
            if (sliderBox) sliderBox.style.display = "block";
        };
        row.addEventListener("click", selectSnap);
        const delBtn = row.querySelector(".btn-snap-delete");
        if (delBtn) {
            delBtn.addEventListener("click", async (ev) => {
                ev.stopPropagation();
                const pw = await requestActionPassword(
                    "Anlık görüntü silme onayı",
                    `${s.name} yedeğini kalıcı olarak silmek için parolanızı girin`
                );
                if (pw === null) return;
                delBtn.disabled = true;
                const dres = await apiCall("delete_snapshot", { snapshot: s.path, password: pw });
                if (dres.status !== "ok") {
                    alert("Hata: " + (dres.message || "Silinemedi"));
                    delBtn.disabled = false;
                    return;
                }
                if (selectedSnapshot && selectedSnapshot.path === s.path) {
                    selectedSnapshot = null;
                    const sliderBox = document.getElementById("restore-slider-box");
                    if (sliderBox) sliderBox.style.display = "none";
                }
                loadSnapshots();
            });
        }
        container.appendChild(row);
    });
}

async function onConfirmRestore(slider) {
    if (!selectedSnapshot) {
        alert(currentLang === "tr" ? "Geri yüklenecek bir yedek seçin" : "Select a backup to restore");
        resetSlider(slider);
        return;
    }

    const pw = await requestActionPassword(
        "Sistem geri yükleme onayı",
        `${selectedSnapshot.name} yedeğine geri dönmek için yönetici (wheel) veya root parolanızı girin`
    );
    if (pw === null) {
        resetSlider(slider);
        return;
    }

    const label = slider.querySelector(".slider-label");
    label.textContent = t("restoring_msg");

    const res = await apiCall("restore_snapshot", { snapshot: selectedSnapshot.path, password: pw });
    if (res.status === "ok") {
        label.textContent = t("restore_success_msg");
        setTimeout(() => {
            apiCall("reboot", { target: "system" });
        }, 1500);
    } else {
        alert("Geri yükleme hatası: " + res.message);
        label.textContent = t("slider_restore_label");
        resetSlider(slider);
    }
}

// 6. Wipe & Hard Reset Logic
let hardResetVerified = false;
let hardResetConfirmCode = "SIFIRLA";
let hardResetPassword = "";

function initHardResetModal() {
    const chkHardReset = document.getElementById("wipe-hard-reset");
    const modal = document.getElementById("modal-hard-reset");
    const btnClose = document.getElementById("btn-modal-close");
    const btnCancel = document.getElementById("btn-cancel-hard-reset");
    const btnConfirm = document.getElementById("btn-confirm-hard-reset");
    const inputConfirm = document.getElementById("input-hard-reset-confirm");
    const inputPass = document.getElementById("input-hard-reset-pass");
    const errorEl = document.getElementById("modal-error-msg");

    function openModal() {
        if (!modal) return;
        inputConfirm.value = "";
        inputPass.value = "";
        errorEl.style.display = "none";
        errorEl.textContent = "";
        const passField = inputPass ? inputPass.closest(".modal-field") : null;
        if (passField) passField.style.display = cachedAuthRequired ? "flex" : "none";
        modal.style.display = "flex";
        setTimeout(() => inputConfirm.focus(), 100);
    }

    function closeModal() {
        if (!modal) return;
        modal.style.display = "none";
        if (!hardResetVerified && chkHardReset) {
            chkHardReset.checked = false;
        }
    }

    if (chkHardReset) {
        chkHardReset.addEventListener("click", (e) => {
            if (chkHardReset.checked && !hardResetVerified) {
                e.preventDefault();
                chkHardReset.checked = false;
                openModal();
            } else if (!chkHardReset.checked) {
                hardResetVerified = false;
                hardResetPassword = "";
            }
        });
    }

    if (btnClose) btnClose.addEventListener("click", closeModal);
    if (btnCancel) btnCancel.addEventListener("click", closeModal);

    if (btnConfirm) {
        btnConfirm.addEventListener("click", async () => {
            const confirmVal = inputConfirm.value.trim().toUpperCase();
            const pw = inputPass ? inputPass.value : "";

            if (confirmVal !== "YES" && confirmVal !== "SIFIRLA" && confirmVal !== "RESET") {
                errorEl.textContent = t("modal_err_yes");
                errorEl.style.display = "block";
                inputConfirm.focus();
                return;
            }

            if (cachedAuthRequired) {
                if (!pw) {
                    errorEl.textContent = t("modal_err_pass");
                    errorEl.style.display = "block";
                    inputPass.focus();
                    return;
                }
                btnConfirm.disabled = true;
                const authRes = await apiCall("auth_unlock", { password: pw });
                btnConfirm.disabled = false;
                if (authRes.status !== "ok") {
                    errorEl.textContent = authRes.message || "Yönetici veya root parolası hatalı";
                    errorEl.style.display = "block";
                    inputPass.value = "";
                    inputPass.focus();
                    return;
                }
            }

            // Verification successful
            hardResetVerified = true;
            hardResetConfirmCode = confirmVal;
            hardResetPassword = pw;
            if (chkHardReset) chkHardReset.checked = true;
            closeModal();
        });
    }
}

async function onConfirmWipe(slider) {
    const wipeCache = document.getElementById("wipe-cache").checked;
    const wipeUserCache = document.getElementById("wipe-user-cache").checked;
    const wipeHome = document.getElementById("wipe-home").checked;
    const wipeFactory = document.getElementById("wipe-factory").checked;
    const wipeHardReset = document.getElementById("wipe-hard-reset") ? document.getElementById("wipe-hard-reset").checked : false;
    const statusEl = document.getElementById("wipe-status-text");

    if (!wipeCache && !wipeUserCache && !wipeHome && !wipeFactory && !wipeHardReset) {
        alert(t("wipe_select_warn"));
        resetSlider(slider);
        return;
    }

    let actionPassword = wipeHardReset ? hardResetPassword : "";
    if (cachedAuthRequired && !actionPassword) {
        actionPassword = await requestActionPassword(
            "Temizleme ve sıfırlama onayı",
            "Seçili sistem alanlarını silmek veya sıfırlamak için yönetici (wheel) veya root parolanızı girin"
        );
        if (actionPassword === null) {
            resetSlider(slider);
            return;
        }
    }

    const label = slider.querySelector(".slider-label");
    if (wipeHardReset) {
        label.textContent = t("wipe_running_hard");
        statusEl.textContent = t("wipe_running_hard");
    } else {
        label.textContent = t("wipe_running");
        statusEl.textContent = t("wipe_running");
    }

    const res = await apiCall("wipe", {
        system_cache: wipeCache,
        user_cache: wipeUserCache,
        home: wipeHome,
        factory: wipeFactory,
        hard_reset: wipeHardReset,
        confirm: hardResetConfirmCode,
        password: actionPassword,
        lang: currentLang
    });

    if (res.status === "ok") {
        if (wipeHardReset) {
            label.textContent = t("wipe_hard_reset_success");
            statusEl.textContent = t("wipe_hard_reset_success");
        } else {
            label.textContent = t("wipe_complete");
            statusEl.textContent = t("wipe_success_msg");
            setTimeout(() => {
                resetSlider(slider);
                label.textContent = t("wipe_slider_label");
            }, 2500);
        }
    } else {
        alert("Hata: " + res.message);
        label.textContent = t("wipe_slider_label");
        statusEl.textContent = "Hata: " + res.message;
        resetSlider(slider);
    }
}

// 7. Partitions, Disk Partitioning & SMART
function initPartitions() {
    const btn = document.getElementById("btn-refresh-parts");
    if (btn) btn.addEventListener("click", loadPartitions);

    const logEl = document.getElementById("part-op-console");
    function setPartLog(msg) {
        if (logEl) logEl.textContent = msg;
    }

    const btnMklabel = document.getElementById("btn-disk-mklabel");
    if (btnMklabel) {
        btnMklabel.addEventListener("click", async () => {
            const diskSel = document.getElementById("part-target-disk");
            const tblSel = document.getElementById("part-table-type");
            const disk = diskSel ? diskSel.value : "";
            const labelType = tblSel ? tblSel.value : "gpt";
            if (!disk) return;
            const pw = await requestActionPassword(
                "Bölüm tablosu sıfırlama",
                `${disk} üzerinde ${labelType.toUpperCase()} bölüm tablosu oluşturmak için yönetici veya root parolanızı girin`
            );
            if (pw === null) return;
            btnMklabel.disabled = true;
            setPartLog(`${disk} üzerinde ${labelType.toUpperCase()} bölüm tablosu oluşturuluyor...`);
            const res = await apiCall("disk_mklabel", { disk, label_type: labelType, password: pw });
            btnMklabel.disabled = false;
            setPartLog(res.status === "ok" ? res.output : `Hata: ${res.message || res.output || ""}`);
            loadPartitions();
        });
    }

    const btnCreate = document.getElementById("btn-part-create");
    if (btnCreate) {
        btnCreate.addEventListener("click", async () => {
            const diskSel = document.getElementById("part-target-disk");
            const sizeInp = document.getElementById("part-new-size");
            const fsSel = document.getElementById("part-new-fs");
            const lblInp = document.getElementById("part-new-label");
            const disk = diskSel ? diskSel.value : "";
            const sizeMb = sizeInp ? sizeInp.value.trim() : "";
            const fstype = fsSel ? fsSel.value : "ext4";
            const label = lblInp ? lblInp.value.trim() : "";
            if (!disk) return;
            const pw = await requestActionPassword(
                "Yeni disk bölümü oluşturma",
                `${disk} diskinde yeni ${fstype.toUpperCase()} bölümü oluşturmak için yönetici veya root parolanızı girin`
            );
            if (pw === null) return;
            btnCreate.disabled = true;
            setPartLog(`${disk} üzerinde yeni bölüm (${fstype}, boyut=${sizeMb || "100%"}) oluşturuluyor...`);
            const res = await apiCall("part_create", {
                disk,
                size_mb: sizeMb,
                fstype,
                label,
                password: pw,
            });
            btnCreate.disabled = false;
            if (res.status === "ok") {
                if (sizeInp) sizeInp.value = "";
                if (lblInp) lblInp.value = "";
                setPartLog(res.output || "Bölüm oluşturuldu");
            } else {
                setPartLog(`Hata: ${res.message || res.output || ""}`);
            }
            loadPartitions();
        });
    }

    loadPartitions();
}

async function loadPartitions() {
    const tbody = document.querySelector("#parts-table tbody");
    const diskSel = document.getElementById("part-target-disk");
    const logEl = document.getElementById("part-op-console");
    if (!tbody) return;
    tbody.innerHTML = `<tr><td colspan='5' class='text-center'>${escapeHtml(t("fetching_parts"))}</td></tr>`;

    const res = await apiCall("partitions");

    if (diskSel && Array.isArray(res.disks)) {
        const prev = diskSel.value;
        diskSel.innerHTML = "";
        res.disks.forEach(dk => {
            const opt = document.createElement("option");
            opt.value = dk.name;
            opt.textContent = `${dk.name} (${dk.size}, ${dk.model || "Disk"}, ${String(dk.pttype || "none").toUpperCase()}${dk.busy ? ", bağlı" : ""})`;
            diskSel.appendChild(opt);
        });
        if (prev && Array.from(diskSel.options).some(o => o.value === prev)) {
            diskSel.value = prev;
        }
    }

    if (!res.partitions || res.partitions.length === 0) {
        tbody.innerHTML = `<tr><td colspan='5' class='text-center'>${escapeHtml(t("no_parts_found"))}</td></tr>`;
        return;
    }

    tbody.innerHTML = "";
    res.partitions.forEach(p => {
        const tr = document.createElement("tr");
        const isMounted = Boolean(p.mountpoint && p.mountpoint.length > 0);
        const isPart = p.type === "part";
        const isProtected = Boolean(p.protected);
        const fsLabel = [p.fstype || "-", p.label ? `[${p.label}]` : ""].filter(Boolean).join(" ");

        let actionsHtml = "";
        if (!isProtected && (isPart || p.fstype)) {
            actionsHtml += `
                <button class="fox-btn outline-btn fox-btn-sm part-act-mount" type="button">
                    ${isMounted ? t("btn_unmount") : t("btn_mount")}
                </button>
            `;
            if (p.fstype && p.fstype !== "swap") {
                actionsHtml += `
                    <button class="fox-btn outline-btn fox-btn-sm part-act-fsck" type="button">
                        ${currentLang === "tr" ? "Onar" : "Check"}
                    </button>
                `;
            }
            if (isPart && !isMounted) {
                actionsHtml += `
                    <button class="fox-btn outline-btn fox-btn-sm part-act-fmt" type="button">
                        ${currentLang === "tr" ? "Biçimlendir" : "Format"}
                    </button>
                    <button class="fox-btn danger-btn fox-btn-sm part-act-del" type="button">
                        ${currentLang === "tr" ? "Sil" : "Delete"}
                    </button>
                `;
            }
        } else if (p.vault_protected) {
            actionsHtml = `<span class="vault-lock-badge">Korumalı kasa</span>`;
        } else {
            actionsHtml = `<span class="item-meta">${p.type === "disk" ? "Disk" : "Sistem"}</span>`;
        }

        tr.innerHTML = `
            <td><strong>${escapeHtml(p.name)}</strong></td>
            <td>${escapeHtml(fsLabel)}</td>
            <td>${escapeHtml(p.size || "-")}</td>
            <td>${escapeHtml(p.mountpoint || t("not_mounted"))}</td>
            <td><div class="part-actions-cell">${actionsHtml}</div></td>
        `;

        const mountBtn = tr.querySelector(".part-act-mount");
        if (mountBtn) {
            mountBtn.addEventListener("click", async () => {
                const pw = await requestActionPassword(
                    isMounted ? "Bölüm ayırma onayı" : "Bölüm bağlama onayı",
                    `${p.name} bölümünü ${isMounted ? "ayırmak" : "bağlamak"} için yönetici veya root parolanızı girin`
                );
                if (pw === null) return;
                mountBtn.disabled = true;
                const r = await apiCall("mount_toggle", { device: p.name, action: isMounted ? "unmount" : "mount", password: pw });
                if (logEl) logEl.textContent = r.status === "ok" ? (r.output || "Tamam") : `Hata: ${r.message}`;
                loadPartitions();
            });
        }

        const fsckBtn = tr.querySelector(".part-act-fsck");
        if (fsckBtn) {
            fsckBtn.addEventListener("click", async () => {
                const pw = await requestActionPassword(
                    "Dosya sistemi denetimi",
                    `${p.name} bölümünde dosya sistemi denetimi için yönetici veya root parolanızı girin`
                );
                if (pw === null) return;
                fsckBtn.disabled = true;
                if (logEl) logEl.textContent = `${p.name} dosya sistemi denetleniyor...`;
                const r = await apiCall("part_fsck", { device: p.name, password: pw });
                fsckBtn.disabled = false;
                if (logEl) logEl.textContent = r.output || r.message || "";
            });
        }

        const fmtBtn = tr.querySelector(".part-act-fmt");
        if (fmtBtn) {
            fmtBtn.addEventListener("click", async () => {
                const fsSel = document.getElementById("part-new-fs");
                const lblInp = document.getElementById("part-new-label");
                const chosenFs = fsSel ? fsSel.value : "ext4";
                const chosenLbl = lblInp ? lblInp.value.trim() : "";
                const pw = await requestActionPassword(
                    "Bölüm biçimlendirme onayı",
                    `${p.name} bölümünü ${chosenFs.toUpperCase()} olarak biçimlendirmek için yönetici veya root parolanızı girin`
                );
                if (pw === null) return;
                fmtBtn.disabled = true;
                if (logEl) logEl.textContent = `${p.name} -> ${chosenFs} olarak biçimlendiriliyor...`;
                const r = await apiCall("part_format", { device: p.name, fstype: chosenFs, label: chosenLbl, password: pw });
                fmtBtn.disabled = false;
                if (logEl) logEl.textContent = r.status === "ok" ? r.output : `Hata: ${r.message}`;
                loadPartitions();
            });
        }

        const delBtn = tr.querySelector(".part-act-del");
        if (delBtn) {
            delBtn.addEventListener("click", async () => {
                const pw = await requestActionPassword(
                    "Bölüm silme onayı",
                    `${p.name} bölümünü kalıcı olarak silmek için yönetici veya root parolanızı girin`
                );
                if (pw === null) return;
                delBtn.disabled = true;
                if (logEl) logEl.textContent = `${p.name} bölümü siliniyor...`;
                const r = await apiCall("part_delete", { device: p.name, password: pw });
                if (logEl) logEl.textContent = r.status === "ok" ? r.output : `Hata: ${r.message}`;
                loadPartitions();
            });
        }

        tbody.appendChild(tr);
    });

    const smartBox = document.getElementById("smart-summary");
    if (smartBox) {
        smartBox.textContent = res.smart_info || (currentLang === "tr" ? "SMART sağlık verisi alınamadı" : "No SMART data available");
    }
}

// 8. Boot Repair
function initBootRepair() {
    const consoleEl = document.getElementById("boot-repair-console");

    async function runBootOp(target, bannerTr, bannerEn) {
        let pw = "";
        if (target !== "status") {
            pw = await requestActionPassword(
                "Önyükleme onarım onayı",
                "Önyükleyici veya initramfs yapılandırmasını değiştirmek için yönetici veya root parolanızı girin"
            );
            if (pw === null) return;
        }
        if (consoleEl) consoleEl.textContent = (currentLang === "tr" ? bannerTr : bannerEn) + "\n";
        const res = await apiCall("boot_repair", { target, password: pw });
        if (consoleEl) {
            const prefix = res.status === "ok" ? "" : "Hata: ";
            consoleEl.textContent += prefix + (res.output || res.message || "") + "\n" + (currentLang === "tr" ? "İşlem tamamlandı" : "Operation complete");
        }
    }

    const btnStatus = document.getElementById("btn-boot-status");
    if (btnStatus) {
        btnStatus.addEventListener("click", () => runBootOp("status", "Önyükleyici durumu sorgulanıyor...", "Querying bootloader status..."));
    }

    const btnGrub = document.getElementById("btn-repair-grub");
    if (btnGrub) {
        btnGrub.addEventListener("click", () => runBootOp("grub", "GRUB2 yapılandırması ve ikilileri onarılıyor...", "Rebuilding GRUB2 binaries and configuration..."));
    }

    const btnLimine = document.getElementById("btn-repair-limine");
    if (btnLimine) {
        btnLimine.addEventListener("click", () => runBootOp("limine", "Limine önyükleyicisi kuruluyor ve yapılandırılıyor...", "Installing and configuring Limine bootloader..."));
    }

    const btnEfi = document.getElementById("btn-repair-efi");
    if (btnEfi) {
        btnEfi.addEventListener("click", () => runBootOp("efi", "EFI NVRAM girişleri kontrol ediliyor ve onarılıyor (efibootmgr)...", "Repairing EFI NVRAM boot entries (efibootmgr)..."));
    }

    const btnDracut = document.getElementById("btn-repair-dracut");
    if (btnDracut) {
        btnDracut.addEventListener("click", () => runBootOp("dracut", "Dracut ile tüm initramfs kalıpları yeniden üretiliyor...", "Regenerating all initramfs images via dracut..."));
    }
}

// 9. Root Terminal
function initTerminal() {
    const screen = document.getElementById("term-output");
    const form = document.getElementById("term-form");
    const input = document.getElementById("term-input");

    if (form) {
        form.addEventListener("submit", async (e) => {
            e.preventDefault();
            const cmd = input.value.trim();
            if (!cmd) return;

            screen.textContent += cmd + "\n";
            input.value = "";
            screen.scrollTop = screen.scrollHeight;

            const res = await apiCall("exec", { command: cmd });
            if (res.output) {
                screen.textContent += res.output + "\n# ";
            } else if (res.error) {
                screen.textContent += "Hata: " + res.error + "\n# ";
            } else {
                screen.textContent += "# ";
            }
            screen.scrollTop = screen.scrollHeight;
        });
    }
}

// 10. Reboot Handlers
function initReboot() {
    const btnSys = document.getElementById("btn-reboot-system");
    if (btnSys) {
        btnSys.addEventListener("click", () => {
            if (confirm(t("confirm_reboot_sys"))) {
                apiCall("reboot", { target: "system" });
            }
        });
    }

    const btnRec = document.getElementById("btn-reboot-recovery");
    if (btnRec) {
        btnRec.addEventListener("click", () => {
            if (confirm(t("confirm_reboot_rec"))) {
                apiCall("reboot", { target: "recovery" });
            }
        });
    }

    const btnUefi = document.getElementById("btn-reboot-uefi");
    if (btnUefi) {
        btnUefi.addEventListener("click", () => {
            if (confirm(t("confirm_reboot_uefi"))) {
                apiCall("reboot", { target: "uefi" });
            }
        });
    }

    const btnOff = document.getElementById("btn-poweroff");
    if (btnOff) {
        btnOff.addEventListener("click", () => {
            if (confirm(t("confirm_poweroff"))) {
                apiCall("reboot", { target: "poweroff" });
            }
        });
    }
}

function escapeHtml(text) {
    if (!text) return "";
    return String(text).replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function bootstrapRecoveryWorkspace() {
    if (workspaceInitialized) return;
    workspaceInitialized = true;
    initTabs();
    initTelemetry();
    initSliders();
    initIsos();
    initSnapshots();
    initPartitions();
    initBootRepair();
    initTerminal();
    initReboot();
    initHardResetModal();
}

async function initStartupSecurityLock() {
    const form = document.getElementById("bere-lock-form");
    const pwInp = document.getElementById("bere-lock-password");
    const errEl = document.getElementById("bere-lock-error");
    const unlockBtn = document.getElementById("btn-bere-unlock");

    if (form) {
        form.addEventListener("submit", async (e) => {
            e.preventDefault();
            const pw = pwInp ? pwInp.value : "";
            if (!pw) {
                if (errEl) {
                    errEl.textContent = "Yönetici veya root parolası boş bırakılamaz";
                    errEl.style.display = "block";
                }
                pwInp && pwInp.focus();
                return;
            }
            if (unlockBtn) unlockBtn.disabled = true;
            const res = await apiCall("auth_unlock", { password: pw });
            if (unlockBtn) unlockBtn.disabled = false;
            if (res.status === "ok" && res.unlocked) {
                cachedAuthRequired = Boolean(res.auth_required);
                isSessionUnlocked = true;
                showStartupLockOverlay(false);
                bootstrapRecoveryWorkspace();
            } else {
                if (errEl) {
                    errEl.textContent = res.message || "Parola doğrulanamadı";
                    errEl.style.display = "block";
                }
                if (pwInp) {
                    pwInp.value = "";
                    pwInp.focus();
                }
            }
        });
    }

    const st = await apiCall("auth_status");
    if (st && st.status === "ok") {
        cachedAuthRequired = Boolean(st.auth_required);
        if (!st.auth_required || st.unlocked) {
            isSessionUnlocked = true;
            showStartupLockOverlay(false);
            bootstrapRecoveryWorkspace();
            return;
        }
        isSessionUnlocked = false;
        showStartupLockOverlay(true, st.admin_users || []);
        return;
    }

    // Fallback: keep lock screen visible on transient fetch error
    showStartupLockOverlay(true);
}

document.addEventListener("DOMContentLoaded", () => {
    initLanguage();
    void initStartupSecurityLock();
});
