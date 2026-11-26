use crate::components::{
    build_admin_note, build_kv_row, build_risk_indicator, build_severity_indicator,
    build_status_indicator, risk_css_class,
};
use crate::dbus_client::{dispatch_cmd, UiCommand};
use crate::i18n::Lang;
use crate::state::AppUiState;
use blaze_akrep_common::{
    ActionRequest, SelinuxBooleanEntry, SelinuxDiagnoseQuery, SelinuxRuntimeMode,
    SuggestedSelinuxFix,
};
use blaze_sentinel_core::AkrepSeverity;
use gtk4::pango::WrapMode;
use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, Button, CheckButton, Entry, Label, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

fn populate_boolean_rows(
    list_box: &GtkBox,
    booleans: &[SelinuxBooleanEntry],
    filter: &str,
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
    lang: Lang,
) {
    while let Some(child) = list_box.first_child() {
        list_box.remove(&child);
    }

    let q = filter.trim().to_ascii_lowercase();
    let filtered: Vec<&SelinuxBooleanEntry> = booleans
        .iter()
        .filter(|b| {
            q.is_empty()
                || b.name.to_ascii_lowercase().contains(&q)
                || b.category.to_ascii_lowercase().contains(&q)
                || b.summary_tr.to_lowercase().contains(&q)
                || b.summary_en.to_ascii_lowercase().contains(&q)
        })
        .take(60)
        .collect();

    if filtered.is_empty() {
        let empty = Label::new(Some(lang.pick(
            "Aramayla eşleşen SELinux anahtarı yok",
            "No matching SELinux booleans found",
        )));
        empty.add_css_class("akrep-subtitle");
        empty.set_xalign(0.0);
        list_box.append(&empty);
        return;
    }

    for b in filtered {
        let row = GtkBox::new(Orientation::Horizontal, 12);
        row.add_css_class("akrep-diff-row");
        row.set_valign(Align::Center);

        let info_col = GtkBox::new(Orientation::Vertical, 4);
        info_col.set_hexpand(true);

        let top = GtkBox::new(Orientation::Horizontal, 8);
        top.set_valign(Align::Center);
        let name_lbl = Label::new(Some(&b.name));
        name_lbl.add_css_class("akrep-mono");
        name_lbl.set_xalign(0.0);
        name_lbl.set_wrap(true);
        name_lbl.set_wrap_mode(WrapMode::WordChar);
        top.append(&name_lbl);

        top.append(&build_risk_indicator(b.risk, lang));

        let cat = Label::new(Some(&b.category));
        cat.add_css_class("akrep-tag");
        top.append(&cat);
        info_col.append(&top);

        let desc = Label::new(Some(lang.pick(&b.summary_tr, &b.summary_en)));
        desc.add_css_class("akrep-subtitle");
        desc.set_xalign(0.0);
        desc.set_wrap(true);
        desc.set_wrap_mode(WrapMode::WordChar);
        info_col.append(&desc);
        row.append(&info_col);

        let state_col = GtkBox::new(Orientation::Vertical, 4);
        state_col.set_valign(Align::Center);
        state_col.set_halign(Align::End);
        state_col.append(&build_status_indicator(
            if b.current {
                "akrep-sev-attention"
            } else {
                "akrep-sev-protected"
            },
            if b.current {
                lang.pick("Açık", "On")
            } else {
                lang.pick("Kapalı", "Off")
            },
            false,
        ));

        let pers_str = if !b.persistent_known {
            lang.pick("Varsayılan", "Default")
        } else if b.persistent {
            lang.pick("Açık", "On")
        } else {
            lang.pick("Kapalı", "Off")
        };
        let p_lbl = Label::new(Some(&format!(
            "{}: {}",
            lang.pick("Kalıcı", "Persistent"),
            pers_str
        )));
        p_lbl.add_css_class("akrep-meta");
        p_lbl.set_xalign(1.0);
        state_col.append(&p_lbl);
        row.append(&state_col);

        let target_val = !b.current;
        let btn_txt = if target_val {
            lang.pick("Aç", "Enable")
        } else {
            lang.pick("Kapat", "Disable")
        };
        let toggle_btn = Button::with_label(btn_txt);
        toggle_btn.add_css_class("akrep-ghost-btn");
        toggle_btn.set_valign(Align::Center);
        let cmd_b = cmd_tx.clone();
        let st_b = Rc::clone(state);
        let bname = b.name.clone();
        toggle_btn.connect_clicked(move |_| {
            let persistent = st_b.borrow().selinux_draft.persistent_toggle;
            dispatch_cmd(
                &cmd_b,
                UiCommand::CreatePlan {
                    request: ActionRequest::SelinuxSetBoolean {
                        name: bname.clone(),
                        value: target_val,
                        persistent,
                    },
                },
            );
        });
        row.append(&toggle_btn);

        list_box.append(&row);
    }
}

pub fn build_system_protection_page(
    state: &Rc<RefCell<AppUiState>>,
    cmd_tx: &async_channel::Sender<UiCommand>,
) -> GtkBox {
    let st = state.borrow();
    let lang = st.lang;
    let snap = st.snapshot.clone();
    let diag = st.selinux_diag.clone();
    let draft = st.selinux_draft.clone();
    drop(st);

    let root = GtkBox::new(Orientation::Vertical, 20);

    let page_desc = Label::new(Some(lang.pick(
        "SELinux erişim kontrolü, politika anahtarları ve dosya bağlamı onarımı",
        "SELinux access control, policy booleans and file context repair",
    )));
    page_desc.add_css_class("akrep-subtitle");
    page_desc.set_xalign(0.0);
    page_desc.set_wrap(true);
    root.append(&page_desc);

    // 1. SELinux Mode & Guardrail Status Card (NO disable SELinux button)
    let status_box = GtkBox::new(Orientation::Vertical, 12);
    status_box.add_css_class("akrep-detail-box");

    let top_row = GtkBox::new(Orientation::Horizontal, 12);
    top_row.set_valign(Align::Center);
    let st_hdr = Label::new(Some(lang.pick("SELinux durumu", "SELinux status")));
    st_hdr.add_css_class("akrep-section-hdr");
    st_hdr.set_xalign(0.0);
    st_hdr.set_hexpand(true);
    top_row.append(&st_hdr);

    if let Some(d) = &diag {
        let m_sev = match d.runtime_mode {
            SelinuxRuntimeMode::Enforcing => AkrepSeverity::Protected,
            SelinuxRuntimeMode::Permissive => AkrepSeverity::Attention,
            SelinuxRuntimeMode::Disabled => AkrepSeverity::Critical,
        };
        top_row.append(&build_severity_indicator(m_sev, lang));
    }
    status_box.append(&top_row);

    if let Some(sn) = &snap {
        if let Some(sec) = sn
            .sections
            .iter()
            .find(|x| x.section == "system_protection" || x.section == "selinux")
        {
            for m in &sec.metrics {
                status_box.append(&build_kv_row(lang.pick(&m.label_tr, &m.label_en), &m.value));
            }
        }
    }

    let guardrail_note = Label::new(Some(lang.pick(
        "SELinux tamamen kapatılamaz; politika anahtarları ve dosya bağlamları buradan yönetilebilir",
        "SELinux cannot be disabled; policy booleans and file contexts can be managed here",
    )));
    guardrail_note.add_css_class("akrep-meta");
    guardrail_note.set_xalign(0.0);
    guardrail_note.set_wrap(true);
    status_box.append(&guardrail_note);
    root.append(&status_box);

    // 2. Boolean Table & AVC Diagnosis Trigger
    let bool_card = GtkBox::new(Orientation::Vertical, 12);
    bool_card.add_css_class("akrep-detail-box");

    let b_hdr = GtkBox::new(Orientation::Horizontal, 12);
    b_hdr.set_valign(Align::Center);
    let b_title = Label::new(Some(lang.pick("Politika anahtarları", "Policy booleans")));
    b_title.add_css_class("akrep-section-hdr");
    b_title.set_xalign(0.0);
    b_title.set_hexpand(true);
    b_hdr.append(&b_title);

    let diag_btn = Button::with_label(lang.pick("Tanıla", "Diagnose"));
    diag_btn.add_css_class("akrep-primary-btn");
    diag_btn.set_valign(Align::Center);
    let cmd_diag = cmd_tx.clone();
    diag_btn.connect_clicked(move |_| {
        dispatch_cmd(
            &cmd_diag,
            UiCommand::DiagnoseSelinux {
                query: SelinuxDiagnoseQuery::default(),
            },
        );
    });
    b_hdr.append(&diag_btn);
    bool_card.append(&b_hdr);

    let filter_row = GtkBox::new(Orientation::Horizontal, 12);
    filter_row.set_valign(Align::Center);

    let search_entry = Entry::new();
    search_entry.add_css_class("akrep-entry");
    search_entry.set_width_chars(12);
    search_entry.set_hexpand(true);
    search_entry.set_placeholder_text(Some(lang.pick(
        "Anahtar ara (virt_use_kvm, httpd, samba)",
        "Search booleans (virt_use_kvm, httpd, samba)",
    )));
    search_entry.set_text(&draft.boolean_search);
    filter_row.append(&search_entry);

    let pers_chk = CheckButton::with_label(lang.pick("Kalıcı uygula", "Apply permanently"));
    pers_chk.set_active(draft.persistent_toggle);
    let st_p = Rc::clone(state);
    pers_chk.connect_toggled(move |chk| {
        st_p.borrow_mut().selinux_draft.persistent_toggle = chk.is_active();
    });
    filter_row.append(&pers_chk);
    bool_card.append(&filter_row);

    bool_card.append(&build_admin_note(lang));

    let bool_list_box = GtkBox::new(Orientation::Vertical, 8);
    if let Some(d) = &diag {
        populate_boolean_rows(
            &bool_list_box,
            &d.booleans,
            &draft.boolean_search,
            state,
            cmd_tx,
            lang,
        );
        let booleans_clone = d.booleans.clone();
        let list_for_search = bool_list_box.clone();
        let st_search = Rc::clone(state);
        let cmd_search = cmd_tx.clone();
        search_entry.connect_changed(move |ent| {
            let txt = ent.text().to_string();
            st_search.borrow_mut().selinux_draft.boolean_search = txt.clone();
            populate_boolean_rows(
                &list_for_search,
                &booleans_clone,
                &txt,
                &st_search,
                &cmd_search,
                lang,
            );
        });
    } else {
        let st_search = Rc::clone(state);
        search_entry.connect_changed(move |ent| {
            st_search.borrow_mut().selinux_draft.boolean_search = ent.text().to_string();
        });
        let prompt = Label::new(Some(lang.pick(
            "SELinux anahtarlarını ve engelleme kayıtlarını yüklemek için Tanıla düğmesini kullanın",
            "Select Diagnose above to load SELinux booleans and grouped access denials",
        )));
        prompt.add_css_class("akrep-subtitle");
        prompt.set_xalign(0.0);
        prompt.set_wrap(true);
        bool_list_box.append(&prompt);
    }
    bool_card.append(&bool_list_box);
    root.append(&bool_card);

    // 3. Grouped AVC Denials Section
    let avc_card = GtkBox::new(Orientation::Vertical, 12);
    avc_card.add_css_class("akrep-detail-box");
    let avc_title = Label::new(Some(lang.pick("Engelleme kayıtları", "Access denials")));
    avc_title.add_css_class("akrep-section-hdr");
    avc_title.set_xalign(0.0);
    avc_card.append(&avc_title);

    match &diag {
        Some(d) if !d.avc_groups.is_empty() => {
            for grp in &d.avc_groups {
                let grow = GtkBox::new(Orientation::Vertical, 8);
                grow.add_css_class("akrep-diff-row");

                let gtop = GtkBox::new(Orientation::Horizontal, 12);
                gtop.set_valign(Align::Center);
                let app_lbl = Label::new(Some(&format!(
                    "{} ({} → {}:{})",
                    grp.app_comm, grp.source_domain, grp.target_type, grp.target_class
                )));
                app_lbl.add_css_class("akrep-card-title");
                app_lbl.set_xalign(0.0);
                app_lbl.set_wrap(true);
                app_lbl.set_wrap_mode(WrapMode::WordChar);
                app_lbl.set_hexpand(true);
                gtop.append(&app_lbl);

                gtop.append(&build_status_indicator(
                    risk_css_class(grp.severity),
                    &format!("{}×", grp.count),
                    false,
                ));
                grow.append(&gtop);

                let exp = Label::new(Some(lang.pick(&grp.explanation_tr, &grp.explanation_en)));
                exp.add_css_class("akrep-subtitle");
                exp.set_xalign(0.0);
                exp.set_wrap(true);
                exp.set_wrap_mode(WrapMode::WordChar);
                grow.append(&exp);

                match &grp.suggested_remediation_kind {
                    SuggestedSelinuxFix::Restorecon {
                        target_path,
                        recursive,
                        ..
                    } => {
                        let act_row = GtkBox::new(Orientation::Horizontal, 12);
                        act_row.set_valign(Align::Center);
                        let p_lbl = Label::new(Some(&format!("restorecon {}", target_path)));
                        p_lbl.add_css_class("akrep-mono");
                        p_lbl.set_xalign(0.0);
                        p_lbl.set_wrap(true);
                        p_lbl.set_wrap_mode(WrapMode::WordChar);
                        p_lbl.set_hexpand(true);
                        act_row.append(&p_lbl);

                        let fix_btn = Button::with_label(lang.pick("Bağlamı onar", "Repair context"));
                        fix_btn.add_css_class("akrep-ghost-btn");
                        let cmd_f = cmd_tx.clone();
                        let tp = target_path.clone();
                        let rec = *recursive;
                        fix_btn.connect_clicked(move |_| {
                            dispatch_cmd(
                                &cmd_f,
                                UiCommand::CreatePlan {
                                    request: ActionRequest::SelinuxRestorecon {
                                        target_path: tp.clone(),
                                        recursive: rec,
                                    },
                                },
                            );
                        });
                        act_row.append(&fix_btn);
                        grow.append(&act_row);
                    }
                    SuggestedSelinuxFix::SetBoolean {
                        boolean_name,
                        recommended_value,
                        ..
                    } => {
                        let act_row = GtkBox::new(Orientation::Horizontal, 12);
                        act_row.set_valign(Align::Center);
                        let b_lbl =
                            Label::new(Some(&format!("{} = {}", boolean_name, recommended_value)));
                        b_lbl.add_css_class("akrep-mono");
                        b_lbl.set_xalign(0.0);
                        b_lbl.set_wrap(true);
                        b_lbl.set_hexpand(true);
                        act_row.append(&b_lbl);

                        let fix_btn =
                            Button::with_label(lang.pick("Anahtarı uygula", "Apply boolean"));
                        fix_btn.add_css_class("akrep-ghost-btn");
                        let cmd_f = cmd_tx.clone();
                        let st_f = Rc::clone(state);
                        let bn = boolean_name.clone();
                        let rv = *recommended_value;
                        fix_btn.connect_clicked(move |_| {
                            let persistent = st_f.borrow().selinux_draft.persistent_toggle;
                            dispatch_cmd(
                                &cmd_f,
                                UiCommand::CreatePlan {
                                    request: ActionRequest::SelinuxSetBoolean {
                                        name: bn.clone(),
                                        value: rv,
                                        persistent,
                                    },
                                },
                            );
                        });
                        act_row.append(&fix_btn);
                        grow.append(&act_row);
                    }
                    SuggestedSelinuxFix::ManualPolicyReview {
                        guidance_tr,
                        guidance_en,
                    } => {
                        let gl = Label::new(Some(lang.pick(guidance_tr, guidance_en)));
                        gl.add_css_class("akrep-meta");
                        gl.set_xalign(0.0);
                        gl.set_wrap(true);
                        gl.set_wrap_mode(WrapMode::WordChar);
                        grow.append(&gl);
                    }
                }

                avc_card.append(&grow);
            }
        }
        Some(_) => {
            let clean = Label::new(Some(lang.pick(
                "Son denetim kayıtlarında engelleme bulunmadı",
                "No grouped access denials found in recent audit logs",
            )));
            clean.add_css_class("akrep-subtitle");
            clean.set_xalign(0.0);
            avc_card.append(&clean);
        }
        None => {
            let unl = Label::new(Some(lang.pick(
                "Engelleme kayıtları tanılama sonrasında burada listelenir",
                "Grouped access denials appear here after diagnosis",
            )));
            unl.add_css_class("akrep-subtitle");
            unl.set_xalign(0.0);
            unl.set_wrap(true);
            avc_card.append(&unl);
        }
    }
    root.append(&avc_card);

    // 4. File Context (restorecon) Inspector & Fixer
    let ctx_card = GtkBox::new(Orientation::Vertical, 12);
    ctx_card.add_css_class("akrep-detail-box");

    let ctx_title = Label::new(Some(lang.pick(
        "Dosya bağlamı onarımı",
        "File context repair",
    )));
    ctx_title.add_css_class("akrep-section-hdr");
    ctx_title.set_xalign(0.0);
    ctx_card.append(&ctx_title);

    let ctx_input_row = GtkBox::new(Orientation::Horizontal, 12);
    ctx_input_row.set_valign(Align::Center);
    let ctx_entry = Entry::new();
    ctx_entry.add_css_class("akrep-entry");
    ctx_entry.set_width_chars(12);
    ctx_entry.set_hexpand(true);
    ctx_entry.set_text(&draft.restorecon_path);
    let st_cp = Rc::clone(state);
    ctx_entry.connect_changed(move |ent| {
        st_cp.borrow_mut().selinux_draft.restorecon_path = ent.text().to_string();
    });
    ctx_input_row.append(&ctx_entry);

    let rec_ctx = CheckButton::with_label(lang.pick("Alt klasörler dahil", "Include subfolders"));
    rec_ctx.set_active(draft.restorecon_recursive);
    let st_cr = Rc::clone(state);
    rec_ctx.connect_toggled(move |chk| {
        st_cr.borrow_mut().selinux_draft.restorecon_recursive = chk.is_active();
    });
    ctx_input_row.append(&rec_ctx);
    ctx_card.append(&ctx_input_row);

    let ctx_btn_row = GtkBox::new(Orientation::Horizontal, 8);
    let inspect_ctx_btn = Button::with_label(lang.pick("Bağlamı incele", "Inspect context"));
    inspect_ctx_btn.add_css_class("akrep-ghost-btn");
    let cmd_ic = cmd_tx.clone();
    let st_ic = Rc::clone(state);
    inspect_ctx_btn.connect_clicked(move |_| {
        let d = st_ic.borrow().selinux_draft.clone();
        dispatch_cmd(
            &cmd_ic,
            UiCommand::DiagnoseSelinux {
                query: SelinuxDiagnoseQuery {
                    include_booleans: true,
                    include_avc: false,
                    boolean_filter: None,
                    inspect_path: Some(d.restorecon_path.trim().to_string()),
                    recursive: d.restorecon_recursive,
                    action: Some("inspect_path".to_string()),
                },
            },
        );
    });
    ctx_btn_row.append(&inspect_ctx_btn);

    let fix_ctx_btn = Button::with_label(lang.pick("Onarımı önizle", "Preview repair"));
    fix_ctx_btn.add_css_class("akrep-ghost-btn");
    let cmd_fc = cmd_tx.clone();
    let st_fc = Rc::clone(state);
    fix_ctx_btn.connect_clicked(move |_| {
        let d = st_fc.borrow().selinux_draft.clone();
        dispatch_cmd(
            &cmd_fc,
            UiCommand::CreatePlan {
                request: ActionRequest::SelinuxRestorecon {
                    target_path: d.restorecon_path.trim().to_string(),
                    recursive: d.restorecon_recursive,
                },
            },
        );
    });
    ctx_btn_row.append(&fix_ctx_btn);
    ctx_card.append(&ctx_btn_row);

    if let Some(d) = &diag {
        for c in d.current_contexts.iter().take(15) {
            ctx_card.append(&build_kv_row(&c.path, &c.context));
        }
        for p in d.restorecon_preview.iter().take(15) {
            ctx_card.append(&build_kv_row(
                &p.path,
                &format!("{} → {}", p.from_context, p.to_context),
            ));
        }
    }

    root.append(&ctx_card);
    root
}
