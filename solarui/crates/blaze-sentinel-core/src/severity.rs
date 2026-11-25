use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AkrepSeverity {
    Normal,
    Warning,
    Critical,
    Unknown,
    Unavailable,
}

impl AkrepSeverity {
    pub fn rank(self) -> u8 {
        match self {
            Self::Critical => 4,
            Self::Warning => 3,
            Self::Unknown => 2,
            Self::Unavailable => 1,
            Self::Normal => 0,
        }
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
            Self::Normal => "normal",
            Self::Warning => "warning",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
            Self::Unavailable => "unavailable",
        }
    }

    pub fn label_tr(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Warning => "Uyarı",
            Self::Critical => "Kritik",
            Self::Unknown => "Bilinmiyor",
            Self::Unavailable => "Erişilemiyor",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Warning => "Warning",
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
            Self::Unavailable => "Erişilemiyor (Salt Okunur)",
            Self::Unknown => "Bilinmiyor",
        }
    }

    pub fn label_en(self) -> &'static str {
        match self {
            Self::Available => "Available",
            Self::Unavailable => "Unavailable (Read-Only)",
            Self::Unknown => "Unknown",
        }
    }
}
