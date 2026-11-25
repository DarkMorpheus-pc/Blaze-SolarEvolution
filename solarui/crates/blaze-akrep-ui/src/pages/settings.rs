use crate::components::build_kv_row;
use crate::dbus_client::{UiCommand, AKREP_BUS_NAME, AKREP_INTERFACE_NAME, AKREP_OBJECT_PATH};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{
    AKREP_PRIV_BUS_NAME, AKREP_PRIV_INTERFACE_NAME, AKREP_PRIV_OBJECT_PATH, ALL_POLKIT_ACTIONS,
};
use blaze_sentinel_core::{AKREP_SCHEMA_VERSION, AKREP_VERSION, SENTINEL_VERSION};
use gtk4::prelude::*;
use gtk4::{Box as GtkBox, Button, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

pub fn build_settings_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    on_rerender: Rc<dyn Fn()>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let priv_ver = st.privd_version.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 16);

    let title = Label::new(Some(lang.pick(
        "Ayarlar, Dil Tercihi ve Akrep v1.0 Çift Veriyolu Mimarisi",
        "Settings, Language Preference & Akrep v1.0 Dual-Bus Architecture",
    )));
    title.add_css_class("akrep-title");
    title.set_xalign(0.0);
    root.append(&title);

    // 1. Language Switcher Card
    let lang_card = GtkBox::new(Orientation::Vertical, 10);
    lang_card.add_css_class("akrep-detail-box");
    let l_title = Label::new(Some(lang.pick("Arayüz Dili", "Interface Language")));
    l_title.add_css_class("akrep-section-hdr");
    l_title.set_xalign(0.0);
    lang_card.append(&l_title);

    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    for target_lang in [Lang::Tr, Lang::En] {
        let b = Button::with_label(target_lang.display_name());
        b.add_css_class(if lang == target_lang {
            "akrep-primary-btn"
        } else {
            "akrep-ghost-btn"
        });
        let st_l = Rc::clone(state);
        let rr_l = Rc::clone(&on_rerender);
        b.connect_clicked(move |_| {
            st_l.borrow_mut().lang = target_lang;
            rr_l();
        });
        btn_row.append(&b);
    }

    let ver_btn =
        Button::with_label(lang.pick("akrep-privd Sürümünü Sorgula", "Ping akrep-privd Version"));
    ver_btn.add_css_class("akrep-ghost-btn");
    let cmd_v = cmd_tx.clone();
    ver_btn.connect_clicked(move |_| {
        let _ = cmd_v.send_blocking(UiCommand::QueryPrivVersion);
    });
    btn_row.append(&ver_btn);
    lang_card.append(&btn_row);
    root.append(&lang_card);

    // 2. Service Versions & Privilege-Separation Contract Card
    let info_box = GtkBox::new(Orientation::Vertical, 8);
    info_box.add_css_class("akrep-detail-box");
    let i_title = Label::new(Some(lang.pick(
        "Bileşen Sürümleri ve Yetki Ayrımı Sözleşmesi",
        "Component Versions & Privilege-Separation Contract",
    )));
    i_title.add_css_class("akrep-section-hdr");
    i_title.set_xalign(0.0);
    info_box.append(&i_title);

    let rows: Vec<(&str, &str, String)> = vec![
        (
            "Akrep Sürümü",
            "Akrep Suite Version",
            format!("v{} (Schema v{})", AKREP_VERSION, AKREP_SCHEMA_VERSION),
        ),
        (
            "Paylaşılan Çekirdek",
            "Shared Posture Core",
            format!("blaze-sentinel-core / Sentinel v{}", SENTINEL_VERSION),
        ),
        (
            "Salt Okunur Telemetri (Kanal 1)",
            "Read-Only Telemetry (Channel 1)",
            format!(
                "{} ({} / {}) — User=akrep, read_only=true, 0 GTK4 bağımlılığı",
                AKREP_BUS_NAME, AKREP_OBJECT_PATH, AKREP_INTERFACE_NAME
            ),
        ),
        (
            "Ayrıcalıklı İşlem Servisi (Kanal 2)",
            "Privileged Transaction Daemon (Channel 2)",
            format!(
                "{} ({} / {}) — D-Bus aktivasyonlu, 60sn boşta kapanma ({})",
                AKREP_PRIV_BUS_NAME,
                AKREP_PRIV_OBJECT_PATH,
                AKREP_PRIV_INTERFACE_NAME,
                priv_ver
                    .as_deref()
                    .map(|v| format!("v{}", v))
                    .unwrap_or_else(|| lang.pick("talep üzerine hazır", "on-demand ready").to_string())
            ),
        ),
        (
            "Kullanıcı Oturumu İşlemleri (Kanal 3)",
            "User-Session Channel (Channel 3)",
            lang.pick(
                "Kullanıcı Flatpak INI override + XDG Portal & PipeWire doğrudan akrep-ui içinde çalışır",
                "User Flatpak INI overrides + XDG Portal & PipeWire run directly in-process as desktop user",
            )
            .to_string(),
        ),
        (
            "Tanımlı Polkit Eylemleri",
            "Granular Polkit Actions",
            ALL_POLKIT_ACTIONS.join(", "),
        ),
        (
            "akrepd Bağlantı Durumu",
            "akrepd Telemetry Connection",
            match snap {
                Some(s) => format!(
                    "Online (v{}, read_only={}, mutators_enabled={})",
                    s.version, s.read_only, s.mutators_enabled
                ),
                None => lang
                    .pick("Çevrimdışı / Kısıtlı görünüm", "Offline / Degraded view")
                    .to_string(),
            },
        ),
    ];

    for (l_tr, l_en, val) in rows {
        info_box.append(&build_kv_row(lang.pick(l_tr, l_en), &val));
    }
    root.append(&info_box);

    // 3. Performance & Resource Footprint Card
    let perf_box = GtkBox::new(Orientation::Vertical, 8);
    perf_box.add_css_class("akrep-detail-box");
    let p_title = Label::new(Some(lang.pick(
        "Performans ve Bellek Ayak İzi Garantileri",
        "Performance & Resource Footprint Guarantees",
    )));
    p_title.add_css_class("akrep-section-hdr");
    p_title.set_xalign(0.0);
    perf_box.append(&p_title);

    perf_box.append(&build_kv_row(
        "akrepd",
        lang.pick(
            "Olay güdümlü D-Bus sinyalleri (sıfır sıcak yoklama döngüsü), MemoryMax=96M, CPUQuota=15%, GetSnapshot() < 100ms",
            "Event-driven D-Bus signals (zero hot polling loops), MemoryMax=96M, CPUQuota=15%, GetSnapshot() < 100ms",
        ),
    ));
    perf_box.append(&build_kv_row(
        "akrep-privd",
        lang.pick(
            "Yalnızca işlem sırasında çalışır; 60 saniye hareketsizlikten sonra kendiliğinden kapanır (boşta 0 MB RAM)",
            "Runs strictly on-demand; exits cleanly after 60s of inactivity (0 MB idle RSS)",
        ),
    ));
    perf_box.append(&build_kv_row(
        "ClamAV",
        lang.pick(
            "İsteğe bağlı ve yalnızca talep üzerine tarama; sürekli arka plan fanotify taraması çalıştırılmaz",
            "Optional & strictly on-demand scanning; zero continuous background fanotify CPU tax",
        ),
    ));
    root.append(&perf_box);

    root
}
