use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AkrepSeverity {
    #[serde(alias = "normal")]
    Protected,
    #[serde(alias = "warning")]
    Attention,
    Critical,
    Unknown,
    Unavailable,
}

impl AkrepSeverity {
    #[allow(non_upper_case_globals)]
    pub const Normal: Self = Self::Protected;
    #[allow(non_upper_case_globals)]
    pub const Warning: Self = Self::Attention;
    pub const NORMAL: Self = Self::Protected;
    pub const WARNING: Self = Self::Attention;

    pub fn rank(self) -> u8 {
        match self {
            Self::Critical => 4,
            Self::Attention => 3,
            Self::Unknown => 2,
            Self::Unavailable => 1,
            Self::Protected => 0,
        }
    }

    pub fn is_protected(self) -> bool {
        matches!(self, Self::Protected)
    }

    pub fn combine(self, other: Self) -> Self {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Protected => "protected",
            Self::Attention => "attention",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn label_tr(self) -> &'static str {
        match self {
            Self::Protected => "Korunuyor",
            Self::Attention => "Dikkat",
            Self::Critical => "Kritik",
            Self::Unknown => "Bilinmiyor",
            Self::Unavailable => "Kullanılamıyor",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Protected => "Protected",
            Self::Attention => "Attention",
            Self::Critical => "Critical",
            Self::Unknown => "Unknown",
            Self::Unavailable => "Unavailable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
    Unknown,
}

impl Availability {
    pub fn label_tr(self) -> &'static str {
        match self {
            Self::Available => "Kullanılabilir",
            Self::Unavailable => "Kullanılamıyor",
            Self::Unknown => "Bilinmiyor",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::Unavailable => "Unavailable",
            Self::Unknown => "Unknown",
        }
    }
}
