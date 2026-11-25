use gtk4::CssProvider;

pub fn apply_solarui_theme() {
    let Some(display) = gdk4::Display::default() else {
        return;
    };
    let provider = CssProvider::new();
    provider.load_from_string(
        r#"
        window.akrep-window {
            background-color: #0d1117;
            color: #e6edf3;
        }
        headerbar.akrep-headerbar {
            background-color: #121721;
            border-bottom: 1px solid #222936;
            color: #e6edf3;
        }
        .akrep-rail {
            background-color: #111620;
            border-right: 1px solid #222936;
            padding: 12px 10px;
        }
        .akrep-nav-btn {
            background: transparent;
            color: #9da9bb;
            border-radius: 10px;
            padding: 9px 12px;
            border: 1px solid transparent;
            font-weight: 500;
        }
        .akrep-nav-btn:hover {
            background-color: #1a2130;
            color: #f0f6fc;
        }
        .akrep-nav-btn-active {
            background-color: rgba(59, 130, 246, 0.16);
            color: #93c5fd;
            border: 1px solid rgba(59, 130, 246, 0.35);
            font-weight: 600;
        }
        .akrep-banner {
            background-color: #161b22;
            border: 1px solid #263041;
            border-radius: 14px;
            padding: 16px 20px;
        }
        .akrep-offline-banner {
            background-color: rgba(217, 119, 6, 0.14);
            border: 1px solid rgba(245, 158, 11, 0.45);
            border-radius: 12px;
            padding: 12px 16px;
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
        button.akrep-card {
            background-color: #151b24;
            border: 1px solid #252e3d;
            border-radius: 14px;
            padding: 16px;
            color: #e6edf3;
            transition: all 150ms ease;
        }
        button.akrep-card:hover {
            background-color: #1b2330;
            border-color: #3b82f6;
        }
        .akrep-title {
            font-size: 20px;
            font-weight: 700;
            color: #f8fafc;
        }
        .akrep-subtitle {
            font-size: 13px;
            color: #94a3b8;
        }
        .akrep-card-title {
            font-size: 15px;
            font-weight: 700;
            color: #f1f5f9;
        }
        .akrep-badge {
            border-radius: 999px;
            padding: 3px 9px;
            font-size: 11px;
            font-weight: 700;
        }
        .akrep-sev-normal {
            background-color: rgba(22, 163, 74, 0.18);
            color: #4ade80;
            border: 1px solid rgba(34, 197, 94, 0.35);
        }
        .akrep-sev-warning {
            background-color: rgba(217, 119, 6, 0.18);
            color: #fbbf24;
            border: 1px solid rgba(245, 158, 11, 0.35);
        }
        .akrep-sev-critical {
            background-color: rgba(220, 38, 38, 0.22);
            color: #f87171;
            border: 1px solid rgba(239, 68, 68, 0.4);
        }
        .akrep-sev-unknown {
            background-color: rgba(100, 116, 139, 0.22);
            color: #cbd5e1;
            border: 1px solid rgba(148, 163, 184, 0.35);
        }
        .akrep-detail-box {
            background-color: #151b24;
            border: 1px solid #252e3d;
            border-radius: 12px;
            padding: 14px 16px;
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
        "#,
    );
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
