use crate::i18n::Lang;
use blaze_akrep_common::{
    ActionPlan, DeviceSecurityStatus, FirewallProto, FirewallStateSnapshot, FlatpakScope,
    PrivacySessionSnapshot, ProfileCatalogResponse, QuarantineInventory, RulePersistence,
    ScanProgress, ScannerStatusReport, SelinuxDiagnoseResponse, TransactionRecord,
    UnifiedAppInventory, UserOverrideTransaction,
};
use blaze_sentinel_core::AkrepSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Success,
    Error,
    Info,
}

#[derive(Debug, Clone)]
pub struct NoticeBanner {
    pub kind: NoticeKind,
    pub message_tr: String,
    pub message_en: String,
}

#[derive(Debug, Clone)]
pub struct ThreatFormDraft {
    pub scan_path: String,
    pub recursive: bool,
    pub confirm_permanent_delete: bool,
}

impl Default for ThreatFormDraft {
    fn default() -> Self {
        let default_path = std::env::var("HOME")
            .ok()
            .filter(|h| !h.trim().is_empty() && h != "/")
            .map(|h| format!("{}/Downloads", h.trim_end_matches('/')))
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            scan_path: default_path,
            recursive: true,
            confirm_permanent_delete: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SelinuxFormDraft {
    pub boolean_search: String,
    pub persistent_toggle: bool,
    pub restorecon_path: String,
    pub restorecon_recursive: bool,
}

impl Default for SelinuxFormDraft {
    fn default() -> Self {
        Self {
            boolean_search: String::new(),
            persistent_toggle: false,
            restorecon_path: "/var/www/html".to_string(),
            restorecon_recursive: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirewallRuleInputKind {
    Port,
    Service,
}

#[derive(Debug, Clone)]
pub struct FirewallFormDraft {
    pub zone: String,
    pub input_kind: FirewallRuleInputKind,
    pub port_or_service: String,
    pub proto: FirewallProto,
    pub persistence: RulePersistence,
    pub source_cidr: String,
    pub bind_interface: String,
    pub bind_zone: String,
    pub bind_permanent: bool,
}

impl Default for FirewallFormDraft {
    fn default() -> Self {
        Self {
            zone: "public".to_string(),
            input_kind: FirewallRuleInputKind::Port,
            port_or_service: "25565".to_string(),
            proto: FirewallProto::Tcp,
            persistence: RulePersistence::TimedRuntime { seconds: 3600 },
            source_cidr: "192.168.1.0/24".to_string(),
            bind_interface: String::new(),
            bind_zone: "public".to_string(),
            bind_permanent: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppFilterTab {
    All,
    UserFlatpak,
    SystemFlatpak,
    Native,
}

#[derive(Debug, Clone)]
pub struct ApplicationsFormDraft {
    pub filter: AppFilterTab,
    pub search_query: String,
    pub selected_app_key: Option<(String, Option<FlatpakScope>)>,
    pub narrow_fs_input: String,
    pub restart_needed_app: Option<String>,
}

impl Default for ApplicationsFormDraft {
    fn default() -> Self {
        Self {
            filter: AppFilterTab::All,
            search_query: String::new(),
            selected_app_key: None,
            narrow_fs_input: "~/Projects:ro".to_string(),
            restart_needed_app: None,
        }
    }
}

pub struct AppUiState {
    pub snapshot: Option<AkrepSnapshot>,
    pub offline_reason: Option<String>,
    pub active_tab: &'static str,
    pub lang: Lang,
    pub notice: Option<NoticeBanner>,

    pub scanner_report: Option<ScannerStatusReport>,
    pub active_scan: Option<ScanProgress>,
    pub quarantine: Option<QuarantineInventory>,

    pub selinux_diag: Option<SelinuxDiagnoseResponse>,
    pub firewall_state: Option<FirewallStateSnapshot>,

    pub app_inventory: Option<UnifiedAppInventory>,
    pub user_flatpak_txs: Vec<UserOverrideTransaction>,

    pub device_security: Option<DeviceSecurityStatus>,
    pub privacy_snapshot: Option<PrivacySessionSnapshot>,

    pub profile_catalog: Option<ProfileCatalogResponse>,
    pub last_profile_preview: Option<ActionPlan>,

    pub history_records: Vec<TransactionRecord>,
    pub privd_version: Option<String>,

    pub threat_draft: ThreatFormDraft,
    pub selinux_draft: SelinuxFormDraft,
    pub firewall_draft: FirewallFormDraft,
    pub apps_draft: ApplicationsFormDraft,
}

impl AppUiState {
    pub fn new(lang: Lang) -> Self {
        Self {
            snapshot: None,
            offline_reason: None,
            active_tab: "overview",
            lang,
            notice: None,
            scanner_report: None,
            active_scan: None,
            quarantine: None,
            selinux_diag: None,
            firewall_state: None,
            app_inventory: None,
            user_flatpak_txs: Vec::new(),
            device_security: None,
            privacy_snapshot: None,
            profile_catalog: None,
            last_profile_preview: None,
            history_records: Vec::new(),
            privd_version: None,
            threat_draft: ThreatFormDraft::default(),
            selinux_draft: SelinuxFormDraft::default(),
            firewall_draft: FirewallFormDraft::default(),
            apps_draft: ApplicationsFormDraft::default(),
        }
    }
}
