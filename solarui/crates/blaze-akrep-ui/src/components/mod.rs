pub mod finding_row;
pub mod header_banner;
pub mod nav_rail;
pub mod status_card;

use blaze_sentinel_core::{AkrepSeverity, Availability};

pub fn severity_css_class(sev: AkrepSeverity) -> &'static str {
    match sev {
        AkrepSeverity::Normal => "akrep-sev-normal",
        AkrepSeverity::Warning => "akrep-sev-warning",
        AkrepSeverity::Critical => "akrep-sev-critical",
        AkrepSeverity::Unknown | AkrepSeverity::Unavailable => "akrep-sev-unknown",
    }
}

pub fn severity_icon_name(sev: AkrepSeverity, avail: Availability) -> &'static str {
    if avail != Availability::Available {
        return "dialog-question-symbolic";
    }
    match sev {
        AkrepSeverity::Normal => "security-high-symbolic",
        AkrepSeverity::Warning => "dialog-warning-symbolic",
        AkrepSeverity::Critical => "dialog-error-symbolic",
        AkrepSeverity::Unknown | AkrepSeverity::Unavailable => "dialog-question-symbolic",
    }
}
