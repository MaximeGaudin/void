use chrono::{Datelike, Local, NaiveTime, Timelike, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookLog {
    pub id: i64,
    pub hook_name: String,
    pub trigger_type: String,
    pub started_at: i64,
    pub duration_ms: i64,
    pub success: bool,
    pub result: Option<String>,
    pub error: Option<String>,
    pub message_id: Option<String>,
    pub input_prompt: Option<String>,
    pub raw_output: Option<String>,
}

/// Parameters for inserting a hook log entry. Used to avoid too many function arguments.
#[derive(Debug)]
pub struct HookLogInsert<'a> {
    pub hook_name: &'a str,
    pub trigger_type: &'a str,
    pub started_at: i64,
    pub duration_ms: i64,
    pub success: bool,
    pub result: Option<&'a str>,
    pub error: Option<&'a str>,
    pub message_id: Option<&'a str>,
    pub input_prompt: Option<&'a str>,
    pub raw_output: Option<&'a str>,
}

/// Time window during which a hook is allowed to execute.
///
/// If set on a hook, the hook will only fire when the current local time
/// falls within the specified days and time range. Outside this window,
/// triggers are silently skipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveWindow {
    /// Days of the week when the hook is active.
    /// Accepted values: "mon", "tue", "wed", "thu", "fri", "sat", "sun".
    pub days: Vec<Weekday>,
    /// Start time (inclusive), format "HH:MM" in 24h.
    pub start: String,
    /// End time (exclusive), format "HH:MM" in 24h.
    pub end: String,
    /// UTC offset in hours (e.g. 2 for UTC+2, -5 for UTC-5).
    /// If omitted, system local time is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub utc_offset_hours: Option<i32>,
}

/// Days of the week for the active window, serialized as lowercase short names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mon" | "monday" => Some(Self::Mon),
            "tue" | "tuesday" => Some(Self::Tue),
            "wed" | "wednesday" => Some(Self::Wed),
            "thu" | "thursday" => Some(Self::Thu),
            "fri" | "friday" => Some(Self::Fri),
            "sat" | "saturday" => Some(Self::Sat),
            "sun" | "sunday" => Some(Self::Sun),
            _ => None,
        }
    }

    fn matches_chrono(&self, weekday: chrono::Weekday) -> bool {
        matches!(
            (self, weekday),
            (Self::Mon, chrono::Weekday::Mon)
                | (Self::Tue, chrono::Weekday::Tue)
                | (Self::Wed, chrono::Weekday::Wed)
                | (Self::Thu, chrono::Weekday::Thu)
                | (Self::Fri, chrono::Weekday::Fri)
                | (Self::Sat, chrono::Weekday::Sat)
                | (Self::Sun, chrono::Weekday::Sun)
        )
    }
}

fn current_minute_time(hour: u32, minute: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(hour, minute, 0)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(0, 0, 0).expect("midnight is a valid NaiveTime"))
}

impl ActiveWindow {
    /// Returns true if the current time falls within this active window.
    pub fn is_active_now(&self) -> bool {
        let (current_weekday, current_time) = match self.utc_offset_hours {
            Some(offset_hours) => {
                let offset_secs = offset_hours * 3600;
                let offset = chrono::FixedOffset::east_opt(offset_secs).unwrap_or_else(|| {
                    chrono::FixedOffset::east_opt(0).expect("UTC offset is valid")
                });
                let now = Utc::now().with_timezone(&offset);
                (now.weekday(), current_minute_time(now.hour(), now.minute()))
            }
            None => {
                let now = Local::now();
                (now.weekday(), current_minute_time(now.hour(), now.minute()))
            }
        };

        let day_match = self.days.iter().any(|d| d.matches_chrono(current_weekday));
        if !day_match {
            return false;
        }

        let Some(start) = parse_hhmm(&self.start) else {
            return true;
        };
        let Some(end) = parse_hhmm(&self.end) else {
            return true;
        };

        if start <= end {
            current_time >= start && current_time < end
        } else {
            // Wraps midnight (e.g. 22:00 -> 06:00)
            current_time >= start || current_time < end
        }
    }
}

fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let h: u32 = parts[0].parse().ok()?;
    let m: u32 = parts[1].parse().ok()?;
    NaiveTime::from_hms_opt(h, m, 0)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
    #[serde(default = "default_agent")]
    pub agent: String,
    /// Extra CLI arguments forwarded verbatim to the agent process. Each
    /// entry is appended as a single argv slot (no shell splitting), so a
    /// flag with a value becomes two entries.
    ///
    /// `void` does not interpret the contents — the hook author is expected
    /// to know the target agent's CLI. Common examples for Claude:
    ///
    /// - `["--model", "sonnet"]` — pin a cheaper, less rate-limited model.
    /// - `["--allowedTools", "Bash(void *),Bash(curl *)"]` — custom tool whitelist.
    /// - `["--dangerously-skip-permissions"]` — skip all permission checks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    /// Optional time window restricting when this hook can fire.
    /// If absent, the hook can fire at any time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_window: Option<ActiveWindow>,
    pub trigger: Trigger,
    pub prompt: PromptConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    NewMessage {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connector: Option<String>,
    },
    Schedule {
        cron: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    pub text: String,
}

pub(crate) fn default_true() -> bool {
    true
}

pub(crate) fn default_max_turns() -> usize {
    3
}

pub(crate) fn default_agent() -> String {
    "claude".to_string()
}
