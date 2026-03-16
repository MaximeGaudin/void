use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::completion::CompletionModel;
use std::io::Write;

#[derive(Clone)]
pub struct AgentHook {
    pub verbose: bool,
}

impl<M: CompletionModel> PromptHook<M> for AgentHook {
    async fn on_tool_call(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
    ) -> ToolCallHookAction {
        let cmd_preview = extract_command_preview(tool_name, args);
        eprint!("\x1b[2m⚙ {}\x1b[0m", cmd_preview);
        let _ = std::io::stderr().flush();
        ToolCallHookAction::Continue
    }

    async fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        _args: &str,
        result: &str,
    ) -> HookAction {
        let lines: Vec<&str> = result.lines().collect();
        let preview = if lines.len() <= 3 {
            result.trim().to_string()
        } else {
            format!("{} … ({} lines)", lines[0].trim(), lines.len())
        };

        if self.verbose {
            eprintln!(" → {}", preview);
        } else {
            eprintln!();
        }

        if self.verbose && tool_name == "void_cli" && lines.len() > 3 {
            for line in lines.iter().take(10) {
                eprintln!("\x1b[2m  │ {}\x1b[0m", line);
            }
            if lines.len() > 10 {
                eprintln!("\x1b[2m  │ ... ({} more lines)\x1b[0m", lines.len() - 10);
            }
        }

        HookAction::Continue
    }
}

fn extract_command_preview(tool_name: &str, args_json: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args_json) {
        if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
            return match tool_name {
                "void_cli" => format!("void {}", truncate(cmd, 120)),
                "shell" => format!("$ {}", truncate(cmd, 120)),
                _ => format!("{}: {}", tool_name, truncate(cmd, 100)),
            };
        }
    }
    format!("{}(…)", tool_name)
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_command_preview_void_cli() {
        let args = r#"{"command": "inbox --pretty"}"#;
        let result = extract_command_preview("void_cli", args);
        assert_eq!(result, "void inbox --pretty");
    }

    #[test]
    fn extract_command_preview_shell() {
        let args = r#"{"command": "date +%Y-%m-%d"}"#;
        let result = extract_command_preview("shell", args);
        assert_eq!(result, "$ date +%Y-%m-%d");
    }

    #[test]
    fn extract_command_preview_unknown_tool() {
        let args = r#"{"command": "test"}"#;
        let result = extract_command_preview("custom_tool", args);
        assert_eq!(result, "custom_tool: test");
    }

    #[test]
    fn extract_command_preview_invalid_json() {
        let result = extract_command_preview("void_cli", "not json");
        assert_eq!(result, "void_cli(…)");
    }

    #[test]
    fn extract_command_preview_missing_command_field() {
        let args = r#"{"other": "value"}"#;
        let result = extract_command_preview("void_cli", args);
        assert_eq!(result, "void_cli(…)");
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn truncate_exact_length() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("hello world", 5), "hello");
    }
}
