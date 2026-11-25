use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use std::collections::HashMap;
use std::path::Path;
use zbus::Connection;

pub struct FirewallSection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

pub fn decode_zone_ports_message(msg: &zbus::message::Message) -> Option<Vec<(String, String)>> {
    if let Ok(nested) = msg.body().deserialize::<Vec<Vec<String>>>() {
        let mut out = Vec::new();
        for item in nested {
            if item.len() >= 2 {
                out.push((item[0].clone(), item[1].to_ascii_lowercase()));
            }
        }
        return Some(out);
    }
    if let Ok(pairs) = msg.body().deserialize::<Vec<(String, String)>>() {
        return Some(
            pairs
                .into_iter()
                .map(|(p, pr)| (p, pr.to_ascii_lowercase()))
                .collect(),
        );
    }
    None
}

pub async fn collect_firewalld(conn: Option<&Connection>) -> FirewallSection {
    let mut findings = Vec::new();
    let mut metrics = Vec::new();
    let mut status = AkrepSeverity::Warning;
    let mut avail = Availability::Available;
    let mut default_zone = String::new();
    let mut active_zones_summary = String::new();
    let mut services_summary = String::new();
    let mut ports_summary = String::new();
    let mut rich_rules_summary = String::new();
    let mut active = false;

    if let Some(c) = conn {
        if let Ok(proxy) = zbus::Proxy::new(
            c,
            "org.fedoraproject.FirewallD1",
            "/org/fedoraproject/FirewallD1",
            "org.fedoraproject.FirewallD1",
        )
        .await
        {
            if let Ok(dz) = proxy.call::<_, _, String>("getDefaultZone", &()).await {
                active = true;
                status = AkrepSeverity::Normal;
                default_zone = dz.clone();

                if let Ok(zone_proxy) = zbus::Proxy::new(
                    c,
                    "org.fedoraproject.FirewallD1",
                    "/org/fedoraproject/FirewallD1",
                    "org.fedoraproject.FirewallD1.zone",
                )
                .await
                {
                    if let Ok(az) = zone_proxy
                        .call::<_, _, HashMap<String, HashMap<String, Vec<String>>>>(
                            "getActiveZones",
                            &(),
                        )
                        .await
                    {
                        let mut parts = Vec::new();
                        for (z, spec) in &az {
                            let ifaces = spec
                                .get("interfaces")
                                .map(|v| v.join(", "))
                                .unwrap_or_default();
                            if ifaces.is_empty() {
                                parts.push(z.clone());
                            } else {
                                parts.push(format!("{} ({})", z, ifaces));
                            }
                        }
                        active_zones_summary = if parts.is_empty() {
                            default_zone.clone()
                        } else {
                            parts.join("; ")
                        };
                    }

                    if let Ok(svcs) = zone_proxy
                        .call::<_, _, Vec<String>>("getServices", &(&dz,))
                        .await
                    {
                        services_summary = if svcs.is_empty() {
                            "Yok".to_string()
                        } else {
                            svcs.join(", ")
                        };
                    }

                    if let Ok(ports_msg) = c
                        .call_method(
                            Some("org.fedoraproject.FirewallD1"),
                            "/org/fedoraproject/FirewallD1",
                            Some("org.fedoraproject.FirewallD1.zone"),
                            "getPorts",
                            &(&dz,),
                        )
                        .await
                    {
                        if let Some(ports) = decode_zone_ports_message(&ports_msg) {
                            ports_summary = if ports.is_empty() {
                                "Açık özel port yok".to_string()
                            } else {
                                ports
                                    .iter()
                                    .map(|(p, proto)| format!("{}/{}", p, proto))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            };
                        }
                    }

                    if let Ok(rich_rules) = zone_proxy
                        .call::<_, _, Vec<String>>("getRichRules", &(&dz,))
                        .await
                    {
                        rich_rules_summary = if rich_rules.is_empty() {
                            "0 kural".to_string()
                        } else {
                            format!("{} aktif kural", rich_rules.len())
                        };
                    }
                }
            }
        } else {
            avail = Availability::Unavailable;
            status = AkrepSeverity::Unknown;
        }
    } else {
        avail = Availability::Unavailable;
        status = AkrepSeverity::Unknown;
    }

    if !active && Path::new("/run/firewalld").exists() {
        active = true;
        status = AkrepSeverity::Normal;
        avail = Availability::Available;
        default_zone = "firewalld çalışıyor (/run/firewalld)".to_string();
    }

    if !active {
        let mut warn_msg_tr =
            "firewalld servisi aktif değil veya D-Bus üzerinden yanıt vermiyor.".to_string();
        let mut warn_msg_en =
            "firewalld service is inactive or unreachable over system D-Bus.".to_string();
        if avail == Availability::Unavailable {
            warn_msg_tr =
                "Sistem D-Bus bağlantısı kurulamadığı için firewalld durumu doğrulanamadı."
                    .to_string();
            warn_msg_en =
                "Could not query firewalld state because system D-Bus was unavailable.".to_string();
        }
        findings.push(AkrepFinding {
            id: "firewall.inactive".to_string(),
            section: "firewall".to_string(),
            severity: status,
            availability: avail,
            title_tr: "Güvenlik Duvarı Etkin Değil".to_string(),
            title_en: "Firewall Not Active".to_string(),
            description_tr: warn_msg_tr,
            description_en: warn_msg_en,
            remediation_hint_tr: "Durumu doğrulamak için `systemctl status firewalld` komutunu kullanın (akrepd salt okunur telemetri sunar).".to_string(),
            remediation_hint_en: "Verify with `systemctl status firewalld` (akrepd provides read-only telemetry).".to_string(),
            source: "collectors/firewalld (D-Bus)".to_string(),
            read_only_note: "akrepd salt okunurdur — kurallar Güvenlik Duvarı sayfasından Polkit onayıyla yönetilir.".to_string(),
        });
    }

    metrics.push(AkrepKeyValue {
        label_tr: "Firewalld D-Bus Durumu".to_string(),
        label_en: "Firewalld D-Bus State".to_string(),
        value: if active {
            "Aktif (org.fedoraproject.FirewallD1)".to_string()
        } else {
            "Pasif / Erişilemiyor".to_string()
        },
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Varsayılan Bölge (Zone)".to_string(),
        label_en: "Default Zone".to_string(),
        value: if default_zone.is_empty() {
            "Bilinmiyor".to_string()
        } else {
            default_zone.clone()
        },
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Etkin Bölgeler".to_string(),
        label_en: "Active Zones".to_string(),
        value: if active_zones_summary.is_empty() {
            "—".to_string()
        } else {
            active_zones_summary.clone()
        },
    });
    metrics.push(AkrepKeyValue {
        label_tr: "İzin Verilen Servisler".to_string(),
        label_en: "Allowed Services".to_string(),
        value: if services_summary.is_empty() {
            "—".to_string()
        } else {
            services_summary
        },
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Açık Özel Portlar".to_string(),
        label_en: "Open Custom Ports".to_string(),
        value: if ports_summary.is_empty() {
            "—".to_string()
        } else {
            ports_summary
        },
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Zengin Kurallar (Rich Rules)".to_string(),
        label_en: "Rich Rules".to_string(),
        value: if rich_rules_summary.is_empty() {
            "—".to_string()
        } else {
            rich_rules_summary
        },
    });

    let summary_tr = if active {
        format!("Aktif • Varsayılan bölge: {}", default_zone)
    } else {
        "Pasif veya erişilemiyor".to_string()
    };
    let summary_en = if active {
        format!("Active • Default zone: {}", default_zone)
    } else {
        "Inactive or unreachable".to_string()
    };

    let card = AkrepCardSummary {
        id: "firewall".to_string(),
        title_tr: "Güvenlik Duvarı (Firewalld)".to_string(),
        title_en: "Firewall & Network (Firewalld)".to_string(),
        status,
        availability: avail,
        summary_tr: summary_tr.clone(),
        summary_en: summary_en.clone(),
        metric: if active {
            default_zone
        } else {
            "OFF".to_string()
        },
    };

    let detail = AkrepSectionDetail {
        section: "firewall".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status,
        availability: avail,
        summary_tr,
        summary_en,
        metrics,
        findings: findings.clone(),
    };

    FirewallSection {
        card,
        detail,
        findings,
    }
}
