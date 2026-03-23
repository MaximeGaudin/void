use rig::completion::ToolDefinition;
use rig::tool::Tool;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
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

fn build_shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

impl Tool for VoidCommandTool {
    const NAME: &'static str = "void_cli";
    type Error = ToolExecError;
    type Args = VoidCommandArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "void_cli".to_string(),
            description: "Execute a void CLI command. Void is a unified communication CLI that \
                manages Gmail, Slack, WhatsApp, Telegram, Google Calendar, and Hacker News. Pass the command WITHOUT \
                the 'void' prefix.\n\n\
                AVAILABLE COMMANDS:\n\n\
                INBOX & MESSAGES:\n\
                - inbox [--connector gmail|slack|whatsapp|telegram|hackernews] [--connection <id>] [-n <max>] [--all] [--include-muted]\n\
                - messages <conversation-id> [-n <count>]\n\
                - search \"<query>\" [--connector <c>] [-n <max>]\n\
                - archive <id1> [<id2> ...]\n\
                - mute <name> [--unmute] [--list]\n\n\
                SENDING:\n\
                - send --via <slack|gmail|telegram> --to \"<recipient>\" --message \"<text>\" [--file <path>] [--at \"<time>\"]\n\
                  (Note: hackernews is read-only — no send/reply)\n\
                - reply <message-id> --message \"<text>\" [--in-thread] [--file <path>] [--at \"<time>\"]\n\n\
                GMAIL:\n\
                - gmail search '<query>' [--max <n>] [--connection <email>]\n\
                - gmail thread <threadId> [--connection <email>]\n\
                - gmail url <threadId>\n\
                - gmail labels [--connection <email>]\n\
                - gmail label <threadId> --add <label> [--remove <label>] [--connection <email>]\n\
                - gmail drafts [--connection <email>]\n\
                - gmail draft create --to \"<email>\" --subject \"<s>\" --body \"<b>\" [--reply-to <msgId>] [--thread-id <tId>] [--connection <email>]\n\
                - gmail draft update <draftId> --to \"<email>\" --subject \"<s>\" --body \"<b>\" [--connection <email>]\n\
                - gmail draft delete <draftId> [--connection <email>]\n\
                - gmail attachment <messageId> <attachmentId> --out <path> [--connection <email>]\n\
                - gmail batch-modify <id1> [<id2>...] --add <label> [--remove <label>] [--connection <email>]\n\n\
                SLACK:\n\
                - slack react <message-id> --emoji <name>\n\
                - slack edit <message-id> --message \"<text>\"\n\
                - slack schedule --channel \"<ch>\" --message \"<text>\" --at \"<time>\" [--thread <ts>]\n\
                - slack open --users <uid1>,<uid2>\n\n\
                CALENDAR:\n\
                - calendar [--day today|tomorrow|<date>] [--from <date> --to <date>] [--connection <id>]\n\
                - calendar week [--connection <id>]\n\
                - calendar create --title \"<t>\" --start \"<iso>\" [--end \"<iso>\"] [--attendees \"<emails>\"] [--meet] [--description \"<d>\"] [--connection <id>]\n\
                - calendar search \"<query>\" [--from <date> --to <date>] [--connection <id>]\n\
                - calendar update <event-id> [--title \"<t>\"] [--start \"<iso>\"] [--end \"<iso>\"] [--connection <id>]\n\
                - calendar respond <event-id> --status accepted|declined|tentative [--comment \"<c>\"] [--email <e>] [--connection <id>]\n\
                - calendar delete <event-id> [--connection <id>]\n\
                - calendar availability --attendees \"<emails>\" --from <date> --to <date> [--connection <id>]\n\
                - calendar calendars\n\n\
                OTHER:\n\
                - contacts [--connector <c>]\n\
                - channels [--connector <c>]\n\
                - conversations [--connector <c>]\n\n\
                NOTES:\n\
                - Output is always JSON.\n\
                - For multi-line bodies use heredoc: --body \"$(cat <<'EOF'\\n...\\nEOF\\n)\"\n\
                - Gmail connections: mgaudin@gladia.io (professional), me@maxime.ly (personal)\n\
                - Calendar connections: mgaudin@gladia.io-calendar, me@maxime.ly-calendar"
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The void CLI command to execute, without the 'void' prefix. Example: 'inbox --connector gmail --connection mgaudin@gladia.io'"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let full_command = format!("void {}", args.command);
        tracing::debug!(command = %full_command, "executing void command");

        let output = build_shell_command(&full_command)
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

        let home: PathBuf = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

        let output = build_shell_command(&args.command)
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
