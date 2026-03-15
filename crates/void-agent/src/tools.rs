use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum ToolExecError {
    #[error("Failed to execute command: {0}")]
    Execution(String),
}

// ---------------------------------------------------------------------------
// VoidCommand — execute `void <subcommand>` as a subprocess
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct VoidCommandArgs {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoidCommandTool;

impl Tool for VoidCommandTool {
    const NAME: &'static str = "void_cli";
    type Error = ToolExecError;
    type Args = VoidCommandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "void_cli".to_string(),
            description: "Execute a void CLI command. Void is a unified communication CLI that \
                manages Gmail, Slack, WhatsApp, and Google Calendar. Pass the command WITHOUT \
                the 'void' prefix.\n\n\
                AVAILABLE COMMANDS:\n\n\
                INBOX & MESSAGES:\n\
                - inbox [--connector gmail|slack|whatsapp] [--account <id>] [--pretty] [-n <max>] [--all] [--include-muted]\n\
                - messages <conversation-id> [-n <count>] [--pretty]\n\
                - search \"<query>\" [--connector <c>] [-n <max>] [--pretty]\n\
                - archive <id1> [<id2> ...]\n\
                - mute <name> [--unmute] [--list]\n\n\
                SENDING:\n\
                - send --via <slack|gmail> --to \"<recipient>\" --message \"<text>\" [--file <path>] [--at \"<time>\"]\n\
                - reply <message-id> --message \"<text>\" [--in-thread] [--file <path>] [--at \"<time>\"]\n\n\
                GMAIL:\n\
                - gmail search '<query>' [--max <n>] [--account <email>] [--pretty]\n\
                - gmail thread <threadId> [--account <email>]\n\
                - gmail url <threadId>\n\
                - gmail labels [--account <email>]\n\
                - gmail label <threadId> --add <label> [--remove <label>] [--account <email>]\n\
                - gmail drafts [--account <email>]\n\
                - gmail draft create --to \"<email>\" --subject \"<s>\" --body \"<b>\" [--reply-to <msgId>] [--thread-id <tId>] [--account <email>]\n\
                - gmail draft update <draftId> --to \"<email>\" --subject \"<s>\" --body \"<b>\" [--account <email>]\n\
                - gmail draft delete <draftId> [--account <email>]\n\
                - gmail attachment <messageId> <attachmentId> --out <path> [--account <email>]\n\
                - gmail batch-modify <id1> [<id2>...] --add <label> [--remove <label>] [--account <email>]\n\n\
                SLACK:\n\
                - slack react <message-id> --emoji <name>\n\
                - slack edit <message-id> --message \"<text>\"\n\
                - slack schedule --channel \"<ch>\" --message \"<text>\" --at \"<time>\" [--thread <ts>]\n\
                - slack open --users <uid1>,<uid2>\n\n\
                CALENDAR:\n\
                - calendar [--day today|tomorrow|<date>] [--from <date> --to <date>] [--account <id>] [--pretty]\n\
                - calendar week [--account <id>] [--pretty]\n\
                - calendar create --title \"<t>\" --start \"<iso>\" [--end \"<iso>\"] [--attendees \"<emails>\"] [--meet] [--description \"<d>\"] [--account <id>]\n\
                - calendar search \"<query>\" [--from <date> --to <date>] [--account <id>]\n\
                - calendar update <event-id> [--title \"<t>\"] [--start \"<iso>\"] [--end \"<iso>\"] [--account <id>]\n\
                - calendar respond <event-id> --status accepted|declined|tentative [--comment \"<c>\"] [--email <e>] [--account <id>]\n\
                - calendar delete <event-id> [--account <id>]\n\
                - calendar availability --attendees \"<emails>\" --from <date> --to <date> [--account <id>]\n\
                - calendar calendars\n\n\
                OTHER:\n\
                - contacts [--connector <c>] [--pretty]\n\
                - channels [--connector <c>] [--pretty]\n\
                - conversations [--connector <c>] [--pretty]\n\n\
                NOTES:\n\
                - Output is JSON by default; use --pretty for human-readable tables.\n\
                - For multi-line bodies use heredoc: --body \"$(cat <<'EOF'\\n...\\nEOF\\n)\"\n\
                - Gmail accounts: mgaudin@gladia.io (professional), me@maxime.ly (personal)\n\
                - Calendar accounts: mgaudin@gladia.io-calendar, me@maxime.ly-calendar"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The void CLI command to execute, without the 'void' prefix. Example: 'inbox --connector gmail --account mgaudin@gladia.io --pretty'"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let full_command = format!("void {}", args.command);
        tracing::debug!(command = %full_command, "executing void command");

        let output = Command::new("sh")
            .arg("-c")
            .arg(&full_command)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| ToolExecError::Execution(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            if stdout.trim().is_empty() {
                Ok("(no output)".to_string())
            } else {
                Ok(stdout)
            }
        } else {
            Ok(format!(
                "EXIT {}:\n{}\n{}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// ShellCommand — execute arbitrary shell commands
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ShellCommandArgs {
    pub command: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommandTool;

impl Tool for ShellCommandTool {
    const NAME: &'static str = "shell";
    type Error = ToolExecError;
    type Args = ShellCommandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "shell".to_string(),
            description: "Execute an arbitrary shell command (bash). Use for file operations \
                (reading files, listing directories), date/time queries, opening URLs, or any \
                operation not covered by void_cli. The working directory is the user's home."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        tracing::debug!(command = %args.command, "executing shell command");

        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());

        let output = Command::new("sh")
            .arg("-c")
            .arg(&args.command)
            .current_dir(&home)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .await
            .map_err(|e| ToolExecError::Execution(e.to_string()))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        const MAX_OUTPUT: usize = 50_000;
        let combined = if output.status.success() {
            if stdout.trim().is_empty() {
                "(no output)".to_string()
            } else {
                stdout
            }
        } else {
            format!(
                "EXIT {}:\n{}\n{}",
                output.status.code().unwrap_or(-1),
                stdout,
                stderr
            )
        };

        if combined.len() > MAX_OUTPUT {
            Ok(format!(
                "{}...\n\n[truncated — {} bytes total]",
                &combined[..MAX_OUTPUT],
                combined.len()
            ))
        } else {
            Ok(combined)
        }
    }
}
