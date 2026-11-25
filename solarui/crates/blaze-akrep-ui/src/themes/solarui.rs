use gtk4::CssProvider;

pub const SOLARUI_CSS: &str = r#"
window.akrep-window {
    background-color: #0b0f16;
    color: #e6edf3;
}
window.akrep-modal {
    background-color: #0f1520;
    color: #e6edf3;
    border: 1px solid #2b384e;
    border-radius: 14px;
}
headerbar.akrep-headerbar {
    background-color: #101622;
    border-bottom: 1px solid rgba(148, 163, 184, 0.14);
    color: #e6edf3;
}
.akrep-rail {
    background-color: #0e141f;
    border-right: 1px solid rgba(148, 163, 184, 0.14);
    padding: 14px 10px;
}
.akrep-nav-btn {
    background: transparent;
    color: #9da9bb;
    border-radius: 10px;
    padding: 8px 12px;
    border: 1px solid transparent;
    font-weight: 500;
}
.akrep-nav-btn:hover {
    background-color: rgba(59, 130, 246, 0.10);
    color: #f0f6fc;
}
.akrep-nav-btn-active {
    background-color: rgba(59, 130, 246, 0.18);
    color: #93c5fd;
    border: 1px solid rgba(59, 130, 246, 0.42);
    font-weight: 600;
}
.akrep-banner {
    background-color: #131a26;
    border: 1px solid rgba(148, 163, 184, 0.18);
    border-radius: 14px;
    padding: 16px 20px;
}
.akrep-offline-banner {
    background-color: rgba(217, 119, 6, 0.14);
    border: 1px solid rgba(245, 158, 11, 0.45);
    border-radius: 12px;
    padding: 10px 14px;
}
.akrep-toast-ok {
    background-color: rgba(16, 185, 129, 0.14);
    border: 1px solid rgba(52, 211, 153, 0.42);
    border-radius: 10px;
    padding: 10px 14px;
    color: #a7f3d0;
}
.akrep-toast-err {
    background-color: rgba(220, 38, 38, 0.16);
    border: 1px solid rgba(248, 113, 113, 0.45);
    border-radius: 10px;
    padding: 10px 14px;
    color: #fecaca;
}
.akrep-toast-info {
    background-color: rgba(59, 130, 246, 0.14);
    border: 1px solid rgba(96, 165, 250, 0.40);
    border-radius: 10px;
    padding: 10px 14px;
    color: #bfdbfe;
}
.akrep-ro-pill {
    background-color: rgba(99, 102, 241, 0.16);
    color: #c7d2fe;
    border: 1px solid rgba(129, 140, 248, 0.35);
    border-radius: 999px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 700;
}
.akrep-profile-pill {
    background-color: rgba(14, 165, 233, 0.16);
    color: #7dd3fc;
    border: 1px solid rgba(56, 189, 248, 0.38);
    border-radius: 999px;
    padding: 4px 12px;
    font-size: 11px;
    font-weight: 700;
}
button.akrep-card {
    background-color: #131a26;
    border: 1px solid rgba(148, 163, 184, 0.16);
    border-radius: 14px;
    padding: 15px;
    color: #e6edf3;
    transition: all 140ms ease;
}
button.akrep-card:hover {
    background-color: #192232;
    border-color: rgba(96, 165, 250, 0.55);
}
button.akrep-primary-btn {
    background-color: #1d4ed8;
    color: #f8fafc;
    border: 1px solid rgba(147, 197, 253, 0.40);
    border-radius: 8px;
    padding: 6px 14px;
    font-weight: 600;
}
button.akrep-primary-btn:hover {
    background-color: #2563eb;
}
button.akrep-danger-btn {
    background-color: rgba(185, 28, 28, 0.30);
    color: #fca5a5;
    border: 1px solid rgba(248, 113, 113, 0.45);
    border-radius: 8px;
    padding: 6px 12px;
    font-weight: 600;
}
button.akrep-danger-btn:hover {
    background-color: rgba(220, 38, 38, 0.45);
}
button.akrep-ghost-btn {
    background-color: rgba(30, 41, 59, 0.65);
    color: #cbd5e1;
    border: 1px solid rgba(148, 163, 184, 0.24);
    border-radius: 8px;
    padding: 6px 12px;
    font-weight: 500;
}
button.akrep-ghost-btn:hover {
    background-color: rgba(51, 65, 85, 0.80);
    color: #f8fafc;
}
.akrep-title {
    font-size: 20px;
    font-weight: 700;
    color: #f8fafc;
}
.akrep-section-hdr {
    font-size: 16px;
    font-weight: 700;
    color: #f1f5f9;
}
.akrep-subtitle {
    font-size: 13px;
    color: #94a3b8;
}
.akrep-mono {
    font-family: "JetBrains Mono", monospace;
    font-size: 12px;
    color: #cbd5e1;
}
.akrep-card-title {
    font-size: 14px;
    font-weight: 700;
    color: #f1f5f9;
}
.akrep-badge {
    border-radius: 999px;
    padding: 3px 10px;
    font-size: 11px;
    font-weight: 700;
}
/* 5 Distinct Severity States: Protected, Attention, Critical, Unavailable, Unknown */
.akrep-sev-protected,
.akrep-sev-normal {
    background-color: rgba(16, 185, 129, 0.18);
    color: #34d399;
    border: 1px solid rgba(52, 211, 153, 0.38);
}
.akrep-sev-attention,
.akrep-sev-warning {
    background-color: rgba(217, 119, 6, 0.20);
    color: #fbbf24;
    border: 1px solid rgba(245, 158, 11, 0.42);
}
.akrep-sev-critical {
    background-color: rgba(220, 38, 38, 0.22);
    color: #f87171;
    border: 1px solid rgba(239, 68, 68, 0.45);
}
.akrep-sev-unavailable {
    background-color: rgba(71, 85, 105, 0.28);
    color: #94a3b8;
    border: 1px dashed rgba(148, 163, 184, 0.45);
}
.akrep-sev-unknown {
    background-color: rgba(147, 51, 234, 0.18);
    color: #d8b4fe;
    border: 1px solid rgba(192, 132, 252, 0.40);
}
/* Risk Level Badges */
.akrep-risk-low {
    background-color: rgba(14, 165, 233, 0.16);
    color: #7dd3fc;
    border: 1px solid rgba(56, 189, 248, 0.35);
}
.akrep-risk-medium {
    background-color: rgba(234, 179, 8, 0.18);
    color: #fde047;
    border: 1px solid rgba(250, 204, 21, 0.38);
}
.akrep-risk-high {
    background-color: rgba(249, 115, 22, 0.20);
    color: #fdba74;
    border: 1px solid rgba(251, 146, 60, 0.42);
}
.akrep-risk-critical {
    background-color: rgba(220, 38, 38, 0.24);
    color: #fca5a5;
    border: 1px solid rgba(248, 113, 113, 0.48);
}
/* Transaction State Badges */
.akrep-tx-committed {
    background-color: rgba(16, 185, 129, 0.18);
    color: #6ee7b7;
    border: 1px solid rgba(52, 211, 153, 0.35);
}
.akrep-tx-rolledback {
    background-color: rgba(59, 130, 246, 0.18);
    color: #93c5fd;
    border: 1px solid rgba(96, 165, 250, 0.38);
}
.akrep-tx-failed {
    background-color: rgba(220, 38, 38, 0.22);
    color: #fca5a5;
    border: 1px solid rgba(248, 113, 113, 0.45);
}
.akrep-tx-interrupted {
    background-color: rgba(217, 119, 6, 0.20);
    color: #fcd34d;
    border: 1px solid rgba(245, 158, 11, 0.42);
}
.akrep-detail-box {
    background-color: #131a26;
    border: 1px solid rgba(148, 163, 184, 0.16);
    border-radius: 12px;
    padding: 14px 16px;
}
.akrep-diff-row {
    background-color: #0e1420;
    border: 1px solid rgba(148, 163, 184, 0.15);
    border-radius: 8px;
    padding: 10px 12px;
}
.akrep-metric-key {
    color: #94a3b8;
    font-size: 13px;
    font-weight: 600;
}
.akrep-metric-val {
    color: #f1f5f9;
    font-size: 13px;
}
entry.akrep-entry {
    background-color: #0d131d;
    color: #f1f5f9;
    border: 1px solid rgba(148, 163, 184, 0.28);
    border-radius: 8px;
    padding: 4px 10px;
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
