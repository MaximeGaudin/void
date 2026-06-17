use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConnectorType {
    WhatsApp,
    Slack,
    Gmail,
    Calendar,
    Telegram,
    HackerNews,
    GoogleNews,
    LinkedIn,
}

impl std::fmt::Display for ConnectorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WhatsApp => write!(f, "whatsapp"),
            Self::Slack => write!(f, "slack"),
            Self::Gmail => write!(f, "gmail"),
            Self::Calendar => write!(f, "calendar"),
            Self::Telegram => write!(f, "telegram"),
            Self::HackerNews => write!(f, "hackernews"),
            Self::GoogleNews => write!(f, "googlenews"),
            Self::LinkedIn => write!(f, "linkedin"),
        }
    }
}

impl ConnectorType {
    /// Short badge for display in unified views (e.g. "[WA]", "[SL]").
    pub fn badge(&self) -> &'static str {
        match self {
            Self::WhatsApp => "WA",
            Self::Slack => "SL",
            Self::Gmail => "GM",
            Self::Calendar => "CA",
            Self::Telegram => "TG",
            Self::HackerNews => "HN",
            Self::GoogleNews => "GN",
            Self::LinkedIn => "LI",
        }
    }
}
