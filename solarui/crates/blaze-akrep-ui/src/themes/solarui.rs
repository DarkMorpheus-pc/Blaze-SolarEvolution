use gtk4::CssProvider;

pub const SOLARUI_CSS: &str = r#"
/* Blaze SolarEvolution — Graphite Dark Ramp + Blaze Ember Accent */
window.akrep-window {
    background-color: #141415;
    color: #ededed;
    font-feature-settings: "tnum";
}
window.akrep-modal {
    background-color: #141415;
    color: #ededed;
    border: 1px solid rgba(237, 237, 237, 0.12);
    border-radius: 16px;
    font-feature-settings: "tnum";
}
headerbar.akrep-headerbar {
    background-color: #141415;
    background-image: none;
    border-bottom: 1px solid rgba(237, 237, 237, 0.08);
    color: #ededed;
    min-height: 40px;
    box-shadow: none;
}

/* Visible keyboard focus ring everywhere */
window.akrep-window button:focus-visible,
window.akrep-window entry:focus-visible,
window.akrep-window checkbutton:focus-visible,
window.akrep-window switch:focus-visible,
window.akrep-window expander title:focus-visible,
window.akrep-modal button:focus-visible,
window.akrep-modal entry:focus-visible,
window.akrep-modal checkbutton:focus-visible,
window.akrep-modal switch:focus-visible,
window.akrep-modal expander title:focus-visible {
    outline: 2px solid rgba(224, 87, 60, 0.65);
    outline-offset: 2px;
}

/* Sidebar navigation rail */
.akrep-rail {
    background-color: #0f0f10;
    border-right: 1px solid rgba(237, 237, 237, 0.08);
    padding: 16px 12px;
}
.akrep-brand-title {
    font-size: 16px;
    font-weight: 500;
    color: #ededed;
}
.akrep-brand-sub {
    font-size: 12px;
    font-weight: 400;
    color: #85858b;
}
button.akrep-nav-btn {
    background-color: transparent;
    background-image: none;
    color: #a1a1a6;
    border-radius: 8px;
    padding: 0 12px;
    min-height: 36px;
    border: 1px solid transparent;
    font-size: 14px;
    font-weight: 400;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-nav-btn image {
    color: #85858b;
}
button.akrep-nav-btn:hover {
    background-color: #19191b;
    color: #ededed;
}
button.akrep-nav-btn:hover image {
    color: #a1a1a6;
}
button.akrep-nav-btn-active {
    background-color: #232325;
    color: #ededed;
    border-color: rgba(237, 237, 237, 0.08);
    font-weight: 500;
}
button.akrep-nav-btn-active:hover {
    background-color: #232325;
    color: #ededed;
}
button.akrep-nav-btn-active image {
    color: #ededed;
}
.akrep-nav-indicator {
    min-width: 3px;
    min-height: 16px;
    border-radius: 2px;
    background-color: transparent;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-nav-btn-active .akrep-nav-indicator {
    background-color: #e0573c;
}

/* Compact single-line top bar */
.akrep-topbar {
    background-color: #141415;
    border-bottom: 1px solid rgba(237, 237, 237, 0.08);
    padding: 12px 24px;
    min-height: 52px;
}
.akrep-topbar-notices {
    padding: 12px 24px 0 24px;
}

/* Overview overall status block (single visual moment) */
.akrep-hero-card {
    background-color: #19191b;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 12px;
    padding: 20px 24px;
}
.akrep-hero-title {
    font-size: 24px;
    font-weight: 500;
    color: #ededed;
}

/* Cards & nested rows */
.akrep-detail-box {
    background-color: #19191b;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 12px;
    padding: 16px 20px;
}
.akrep-override-card {
    background-color: #19191b;
    border: 1px solid rgba(217, 160, 63, 0.35);
    border-radius: 12px;
    padding: 16px 20px;
}
.akrep-diff-row {
    background-color: #141415;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 8px;
    padding: 12px;
}
button.akrep-card {
    background-color: #19191b;
    background-image: none;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 12px;
    padding: 16px 20px;
    color: #ededed;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-card:hover {
    background-color: #232325;
    border-color: rgba(237, 237, 237, 0.14);
}
button.akrep-card:active {
    transform: scale(0.98);
}

/* Quiet inline notices */
.akrep-offline-banner {
    background-color: #19191b;
    border: 1px solid rgba(217, 160, 63, 0.35);
    border-radius: 8px;
    padding: 12px 16px;
    color: #ededed;
}
.akrep-high-risk-banner {
    background-color: #19191b;
    border: 1px solid rgba(229, 72, 77, 0.45);
    border-radius: 8px;
    padding: 12px 16px;
    color: #ededed;
}
.akrep-toast-ok {
    background-color: #19191b;
    border: 1px solid rgba(76, 195, 138, 0.35);
    border-radius: 8px;
    padding: 12px 16px;
    color: #ededed;
}
.akrep-toast-err {
    background-color: #19191b;
    border: 1px solid rgba(229, 72, 77, 0.40);
    border-radius: 8px;
    padding: 12px 16px;
    color: #ededed;
}
.akrep-toast-info {
    background-color: #19191b;
    border: 1px solid rgba(237, 237, 237, 0.12);
    border-radius: 8px;
    padding: 12px 16px;
    color: #ededed;
}

/* Compact header tags & icon buttons */
button.akrep-tag-btn {
    background-color: #19191b;
    background-image: none;
    color: #a1a1a6;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 6px;
    padding: 4px 12px;
    min-height: 28px;
    font-size: 12px;
    font-weight: 400;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-tag-btn:hover {
    background-color: #232325;
    border-color: rgba(237, 237, 237, 0.14);
    color: #ededed;
}
button.akrep-tag-btn:active {
    transform: scale(0.98);
}
button.akrep-icon-btn {
    background-color: #19191b;
    background-image: none;
    color: #a1a1a6;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 6px;
    padding: 4px 8px;
    min-width: 28px;
    min-height: 28px;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-icon-btn:hover {
    background-color: #232325;
    border-color: rgba(237, 237, 237, 0.14);
    color: #ededed;
}
button.akrep-icon-btn:active {
    transform: scale(0.98);
}

/* Primary, secondary, segmented and destructive buttons */
button.akrep-primary-btn {
    background-color: #ededed;
    background-image: none;
    color: #141415;
    border: 1px solid #ededed;
    border-radius: 8px;
    padding: 8px 16px;
    min-height: 36px;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-primary-btn:hover {
    background-color: #dcdce0;
    border-color: #dcdce0;
    color: #141415;
}
button.akrep-primary-btn:active {
    transform: scale(0.98);
}
button.akrep-ghost-btn {
    background-color: #232325;
    background-image: none;
    color: #ededed;
    border: 1px solid rgba(237, 237, 237, 0.10);
    border-radius: 8px;
    padding: 8px 12px;
    min-height: 36px;
    font-size: 13px;
    font-weight: 400;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-ghost-btn:hover {
    background-color: #2c2c2f;
    border-color: rgba(237, 237, 237, 0.16);
    color: #ededed;
}
button.akrep-ghost-btn:active {
    transform: scale(0.98);
}
button.akrep-seg-btn {
    background-color: #141415;
    background-image: none;
    color: #a1a1a6;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 8px;
    padding: 8px 12px;
    min-height: 36px;
    font-size: 13px;
    font-weight: 400;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-seg-btn:hover {
    background-color: #232325;
    border-color: rgba(237, 237, 237, 0.12);
    color: #ededed;
}
button.akrep-seg-btn:active {
    transform: scale(0.98);
}
button.akrep-seg-active {
    background-color: #232325;
    background-image: none;
    color: #ededed;
    border: 1px solid rgba(224, 87, 60, 0.65);
    border-radius: 8px;
    padding: 8px 12px;
    min-height: 36px;
    font-size: 13px;
    font-weight: 500;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-seg-active:hover {
    background-color: #2c2c2f;
}
button.akrep-seg-active:active {
    transform: scale(0.98);
}
button.akrep-danger-btn {
    background-color: #232325;
    background-image: none;
    color: #e5484d;
    border: 1px solid rgba(229, 72, 77, 0.35);
    border-radius: 8px;
    padding: 8px 12px;
    min-height: 36px;
    font-size: 13px;
    font-weight: 500;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-danger-btn:hover {
    background-color: #2a1d1e;
    border-color: rgba(229, 72, 77, 0.55);
    color: #e5484d;
}
button.akrep-danger-btn:active {
    transform: scale(0.98);
}
button.akrep-danger-solid-btn {
    background-color: #c62f34;
    background-image: none;
    color: #ededed;
    border: 1px solid #c62f34;
    border-radius: 8px;
    padding: 8px 16px;
    min-height: 36px;
    font-size: 14px;
    font-weight: 500;
    box-shadow: none;
    transition: background-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                transform 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
button.akrep-danger-solid-btn:hover {
    background-color: #d63a3f;
    border-color: #d63a3f;
}
button.akrep-danger-solid-btn:active {
    transform: scale(0.98);
}

/* Type scale (400/500; 600 only on page title) */
.akrep-title {
    font-size: 20px;
    font-weight: 600;
    color: #ededed;
}
.akrep-section-hdr {
    font-size: 16px;
    font-weight: 500;
    color: #ededed;
}
.akrep-card-title {
    font-size: 14px;
    font-weight: 500;
    color: #ededed;
}
.akrep-subtitle {
    font-size: 13px;
    font-weight: 400;
    color: #a1a1a6;
}
.akrep-meta {
    font-size: 12px;
    font-weight: 400;
    color: #85858b;
    font-feature-settings: "tnum";
}
.akrep-mono {
    font-family: monospace;
    font-size: 12px;
    font-weight: 400;
    color: #a1a1a6;
    font-feature-settings: "tnum";
}
.akrep-metric-key {
    color: #85858b;
    font-size: 13px;
    font-weight: 400;
}
.akrep-metric-val {
    color: #ededed;
    font-size: 13px;
    font-weight: 400;
    font-feature-settings: "tnum";
}
.akrep-tag {
    background-color: #232325;
    color: #a1a1a6;
    border: 1px solid rgba(237, 237, 237, 0.08);
    border-radius: 6px;
    padding: 2px 8px;
    font-size: 12px;
    font-weight: 400;
}

/* Quiet status dots (8px) + plain text labels */
.akrep-dot {
    min-width: 8px;
    min-height: 8px;
    border-radius: 999px;
}
.akrep-dot.akrep-sev-protected,
.akrep-dot.akrep-sev-normal {
    background-color: #4cc38a;
}
.akrep-dot.akrep-sev-attention,
.akrep-dot.akrep-sev-warning {
    background-color: #d9a03f;
}
.akrep-dot.akrep-sev-critical {
    background-color: #e5484d;
}
.akrep-dot.akrep-sev-unavailable {
    min-width: 5px;
    min-height: 5px;
    background-color: transparent;
    border: 1.5px solid #85858b;
}
.akrep-dot.akrep-sev-unknown {
    min-width: 5px;
    min-height: 5px;
    background-color: transparent;
    border: 1.5px dashed #85858b;
}

/* Risk dot states */
.akrep-dot.akrep-risk-low {
    background-color: #4cc38a;
}
.akrep-dot.akrep-risk-medium {
    background-color: #d9a03f;
}
.akrep-dot.akrep-risk-high {
    background-color: #e0573c;
}
.akrep-dot.akrep-risk-critical {
    background-color: #e5484d;
}

/* Transaction dot states */
.akrep-dot.akrep-tx-committed {
    background-color: #4cc38a;
}
.akrep-dot.akrep-tx-rolledback {
    min-width: 5px;
    min-height: 5px;
    background-color: transparent;
    border: 1.5px solid #a1a1a6;
}
.akrep-dot.akrep-tx-failed {
    background-color: #e5484d;
}
.akrep-dot.akrep-tx-interrupted {
    background-color: #d9a03f;
}

.akrep-status-text {
    font-size: 12px;
    font-weight: 400;
    color: #ededed;
}
.akrep-status-text-muted {
    font-size: 12px;
    font-weight: 400;
    color: #85858b;
}
.akrep-icon-muted {
    color: #85858b;
}
.akrep-icon-secondary {
    color: #a1a1a6;
}

/* Form controls */
entry.akrep-entry {
    background-color: #232325;
    background-image: none;
    color: #ededed;
    border: 1px solid rgba(237, 237, 237, 0.10);
    border-radius: 8px;
    padding: 4px 12px;
    min-height: 36px;
    font-size: 13px;
    box-shadow: none;
    transition: border-color 150ms cubic-bezier(0.22, 1, 0.36, 1),
                background-color 150ms cubic-bezier(0.22, 1, 0.36, 1);
}
entry.akrep-entry:focus-within {
    border-color: rgba(224, 87, 60, 0.65);
    outline: 2px solid rgba(224, 87, 60, 0.65);
    outline-offset: 1px;
}
checkbutton {
    color: #a1a1a6;
    font-size: 13px;
}
checkbutton check {
    background-color: #232325;
    background-image: none;
    border: 1px solid rgba(237, 237, 237, 0.16);
    border-radius: 4px;
    box-shadow: none;
}
checkbutton check:checked {
    background-color: #c83b22;
    border-color: #c83b22;
    color: #ededed;
}
switch {
    background-color: #232325;
    border: 1px solid rgba(237, 237, 237, 0.12);
    border-radius: 999px;
}
switch:checked {
    background-color: #c83b22;
    border-color: #c83b22;
}
progressbar trough {
    background-color: #232325;
    border-radius: 6px;
    min-height: 8px;
}
progressbar progress {
    background-color: #c83b22;
    background-image: none;
    border-radius: 6px;
}
progressbar text {
    color: #a1a1a6;
    font-size: 12px;
    font-feature-settings: "tnum";
}
expander title {
    color: #a1a1a6;
    font-size: 13px;
    padding: 4px 0;
}
expander title:hover {
    color: #ededed;
}
"#;

pub fn apply_solarui_theme() {
    let Some(display) = gdk4::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_string(SOLARUI_CSS);
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
