use crate::components::{build_kv_row, build_status_indicator};
use crate::dbus_client::{
    dispatch_cmd, UiCommand, AKREP_BUS_NAME, AKREP_INTERFACE_NAME, AKREP_OBJECT_PATH,
};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{
    AKREP_PRIV_BUS_NAME, AKREP_PRIV_INTERFACE_NAME, AKREP_PRIV_OBJECT_PATH, ALL_POLKIT_ACTIONS,
};
use blaze_sentinel_core::{AKREP_SCHEMA_VERSION, AKREP_VERSION, SENTINEL_VERSION};
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, Expander, Label, Orientation};
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

    let root = GtkBox::new(Orientation::Vertical, 20);

    let page_desc = Label::new(Some(lang.pick(
        "Arayüz dili ve Akrep servis bilgileri",
        "Interface language and Akrep service details",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    root.append(&page_desc);

    // 1. Language card
    let lang_card = GtkBox::new(Orientation::Vertical, 12);
    lang_card.add_css_class("akrep-detail-box");
    let l_title = Label::new(Some(lang.pick("Arayüz dili", "Interface language")));
    l_title.add_css_class("akrep-section-hdr");
    l_title.set_xalign(0.0);
    lang_card.append(&l_title);

    let btn_row = GtkBox::new(Orientation::Horizontal, 8);
    for target_lang in [Lang::Tr, Lang::En] {
        let b = Button::with_label(target_lang.display_name());
        b.add_css_class(if lang == target_lang {
            "akrep-seg-active"
        } else {
            "akrep-seg-btn"
        });
        let st_l = Rc::clone(state);
        let rr_l = Rc::clone(&on_rerender);
        b.connect_clicked(move |_| {
            st_l.borrow_mut().lang = target_lang;
            rr_l();
        });
        btn_row.append(&b);
    }
    lang_card.append(&btn_row);
    root.append(&lang_card);

    // 2. Service status & versions
    let info_box = GtkBox::new(Orientation::Vertical, 12);
    info_box.add_css_class("akrep-detail-box");

    let i_hdr = GtkBox::new(Orientation::Horizontal, 12);
    i_hdr.set_valign(Align::Center);
    let i_title = Label::new(Some(lang.pick(
        "Bileşenler ve servisler",
        "Components and services",
    )));
    i_title.add_css_class("akrep-section-hdr");
    i_title.set_xalign(0.0);
    i_title.set_hexpand(true);
    i_hdr.append(&i_title);

    i_hdr.append(&build_status_indicator(
        if snap.is_some() {
            "akrep-sev-protected"
        } else {
            "akrep-sev-unavailable"
        },
        if snap.is_some() {
            lang.pick("Çevrimiçi", "Online")
        } else {
            lang.pick("Çevrimdışı", "Offline")
        },
        snap.is_none(),
    ));

    let ver_btn = Button::with_label(lang.pick("Servisi denetle", "Check service"));
    ver_btn.add_css_class("akrep-ghost-btn");
    let cmd_v = cmd_tx.clone();
    ver_btn.connect_clicked(move |_| {
        dispatch_cmd(&cmd_v, UiCommand::QueryPrivVersion);
    });
    i_hdr.append(&ver_btn);
    info_box.append(&i_hdr);

    info_box.append(&build_kv_row(
        lang.pick("Akrep sürümü", "Akrep version"),
        &format!("v{} (schema v{})", AKREP_VERSION, AKREP_SCHEMA_VERSION),
    ));
    info_box.append(&build_kv_row(
        lang.pick("Sentinel çekirdeği", "Sentinel core"),
        &format!("v{}", SENTINEL_VERSION),
    ));
    info_box.append(&build_kv_row(
        lang.pick("İzleme servisi", "Monitoring service"),
        &match &snap {
            Some(s) => format!("{} (v{})", lang.pick("Çevrimiçi", "Online"), s.version),
            None => lang.pick("Çevrimdışı", "Offline").to_string(),
        },
    ));
    info_box.append(&build_kv_row(
        lang.pick("Yönetici yardımcı servisi", "Admin helper service"),
        &priv_ver
            .as_deref()
            .map(|v| format!("v{}", v))
            .unwrap_or_else(|| lang.pick("Talep üzerine hazır", "Ready on demand").to_string()),
    ));

    let tech_exp = Expander::new(Some(lang.pick(
        "Teknik servis ayrıntıları",
        "Technical service details",
    )));
    let tech_col = GtkBox::new(Orientation::Vertical, 8);
    tech_col.set_margin_top(8);
    tech_col.append(&build_kv_row(
        "akrepd",
        &format!(
            "{} ({} / {})",
            AKREP_BUS_NAME, AKREP_OBJECT_PATH, AKREP_INTERFACE_NAME
        ),
    ));
    tech_col.append(&build_kv_row(
        "akrep-privd",
        &format!(
            "{} ({} / {})",
            AKREP_PRIV_BUS_NAME, AKREP_PRIV_OBJECT_PATH, AKREP_PRIV_INTERFACE_NAME
        ),
    ));
    tech_col.append(&build_kv_row(
        lang.pick("Yetki eylemleri", "Auth actions"),
        &ALL_POLKIT_ACTIONS.join(", "),
    ));
    tech_exp.set_child(Some(&tech_col));
    info_box.append(&tech_exp);
    root.append(&info_box);

    // 3. Resource usage card
    let perf_box = GtkBox::new(Orientation::Vertical, 12);
    perf_box.add_css_class("akrep-detail-box");
    let p_title = Label::new(Some(lang.pick("Kaynak kullanımı", "Resource usage")));
    p_title.add_css_class("akrep-section-hdr");
    p_title.set_xalign(0.0);
    perf_box.append(&p_title);

    perf_box.append(&build_kv_row(
        "akrepd",
        lang.pick(
            "Yalnızca sistem durumu değiştiğinde güncellenir; düşük bellek ve işlemci sınırıyla çalışır",
            "Updates only when system state changes, with strict memory and CPU limits",
        ),
    ));
    perf_box.append(&build_kv_row(
        "akrep-privd",
        lang.pick(
            "Yalnızca onaylı değişiklik sırasında açılır ve kısa süre sonra kendiliğinden kapanır",
            "Starts only for approved changes and exits shortly after completion",
        ),
    ));
    perf_box.append(&build_kv_row(
        "ClamAV",
        lang.pick(
            "İsteğe bağlıdır; yalnızca siz başlattığınızda dosya ve klasör taraması yapar",
            "Optional; scans files and folders only when you start a scan",
        ),
    ));
    root.append(&perf_box);

    root
}
