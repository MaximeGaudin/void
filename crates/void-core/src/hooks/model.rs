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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,
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
