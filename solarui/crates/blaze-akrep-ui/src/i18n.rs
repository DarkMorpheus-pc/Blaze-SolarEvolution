#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Tr,
    En,
}

impl Lang {
    pub fn detect() -> Self {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var) {
                let lower = v.to_lowercase();
                if lower.starts_with("tr") {
                    return Self::Tr;
                }
                if lower.starts_with("en") {
                    return Self::En;
                }
            }
        }
        Self::Tr
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Tr => "tr",
            Self::En => "en",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Tr => "Türkçe (TR)",
            Self::En => "English (EN)",
        }
    }

    pub fn pick<'a>(self, tr: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Tr => tr,
            Self::En => en,
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Tr => Self::En,
            Self::En => Self::Tr,
        }
    }
}
