use anyhow::{bail, Context, Result};
use rig::agent::Agent;
use rig::client::CompletionClient;
use rig::completion::message::Message;
use rig::completion::{CompletionModel, Prompt};
use rig::providers::{anthropic, openai, openrouter};

use crate::hook::AgentHook;
use crate::prompt::build_system_prompt;
use crate::tools::{ShellCommandTool, VoidCommandTool};

const DEFAULT_MAX_TOKENS: u64 = 16384;
const DEFAULT_MAX_TURNS: usize = 30;

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderKind {
    ClaudeCode,
    Anthropic,
    OpenAI,
    OpenRouter,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClaudeCode => write!(f, "claude-code"),
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
            Self::OpenRouter => write!(f, "openrouter"),
        }
    }
}

impl std::str::FromStr for ProviderKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "claude-code" | "claudecode" | "cc" => Ok(Self::ClaudeCode),
            "anthropic" | "claude" => Ok(Self::Anthropic),
            "openai" | "gpt" => Ok(Self::OpenAI),
            "openrouter" | "or" => Ok(Self::OpenRouter),
            _ => bail!(
                "Unknown provider '{}'. Supported: claude-code, anthropic, openai, openrouter",
                s
            ),
        }
    }
}

struct ResolvedCredentials {
    provider: ProviderKind,
    api_key: String,
    source: &'static str,
}

pub struct AgentConfig {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub instructions_file: Option<String>,
    pub inline_instructions: Option<String>,
    pub verbose: bool,
    pub max_turns: Option<usize>,
    pub initial_prompt: Option<String>,
}

pub async fn run(config: AgentConfig) -> Result<()> {
    let creds = resolve_credentials(&config)?;

    let custom_instructions = load_instructions(&config)?;
    let system_prompt = build_system_prompt(custom_instructions.as_deref());

    let model_name = config
        .model
        .clone()
        .unwrap_or_else(|| default_model(&creds.provider));
    let max_turns = config.max_turns.unwrap_or(DEFAULT_MAX_TURNS);

    eprintln!(
        "\x1b[1mVoid Agent\x1b[0m ({} via {}, max turns: {})",
        model_name, creds.source, max_turns
    );
    eprintln!("Type your request, or 'quit' to exit.\n");

    match creds.provider {
        ProviderKind::ClaudeCode => {
            run_claude_code_loop(
                &model_name,
                &system_prompt,
                max_turns,
                config.initial_prompt,
            )
            .await
        }
        ProviderKind::Anthropic => {
            let hook = AgentHook {
                verbose: config.verbose,
            };
            let client = anthropic::Client::new(&creds.api_key)?;
            let agent = client
                .agent(&model_name)
                .preamble(&system_prompt)
                .max_tokens(DEFAULT_MAX_TOKENS)
                .tool(VoidCommandTool)
                .tool(ShellCommandTool)
                .build();
            run_rig_loop(agent, max_turns, hook, config.initial_prompt).await
        }
        ProviderKind::OpenAI => {
            let hook = AgentHook {
                verbose: config.verbose,
            };
            let client = openai::Client::new(&creds.api_key)?;
            let agent = client
                .agent(&model_name)
                .preamble(&system_prompt)
                .max_tokens(DEFAULT_MAX_TOKENS)
                .tool(VoidCommandTool)
                .tool(ShellCommandTool)
                .build();
            run_rig_loop(agent, max_turns, hook, config.initial_prompt).await
        }
        ProviderKind::OpenRouter => {
            let hook = AgentHook {
                verbose: config.verbose,
            };
            let client = openrouter::Client::new(&creds.api_key)?;
            let agent = client
                .agent(&model_name)
                .preamble(&system_prompt)
                .max_tokens(DEFAULT_MAX_TOKENS)
                .tool(VoidCommandTool)
                .tool(ShellCommandTool)
                .build();
            run_rig_loop(agent, max_turns, hook, config.initial_prompt).await
        }
    }
}

// ---------------------------------------------------------------------------
// Claude Code backend — uses `claude -p` with session management
// ---------------------------------------------------------------------------

async fn run_claude_code_loop(
    model: &str,
    system_prompt: &str,
    max_turns: usize,
    initial_prompt: Option<String>,
) -> Result<()> {
    let mut session_id: Option<String> = None;

    let first_input = match initial_prompt {
        Some(prompt) => prompt,
        None => read_user_input()?,
    };

    let mut current_input = first_input;

    loop {
        if current_input.trim().eq_ignore_ascii_case("quit")
            || current_input.trim().eq_ignore_ascii_case("exit")
        {
            eprintln!("\nBye!");
            break;
        }

        if current_input.trim().is_empty() {
            current_input = read_user_input()?;
            continue;
        }

        match invoke_claude_code(&current_input, model, system_prompt, max_turns, &session_id) {
            Ok((text, sid)) => {
                session_id = Some(sid);
                println!("\n{}\n", text);
            }
            Err(e) => {
                eprintln!("\n\x1b[31mError: {}\x1b[0m\n", e);
            }
        }

        current_input = read_user_input()?;
    }

    Ok(())
}

fn invoke_claude_code(
    prompt: &str,
    model: &str,
    system_prompt: &str,
    max_turns: usize,
    session_id: &Option<String>,
) -> Result<(String, String)> {
    let mut cmd = std::process::Command::new("claude");
    cmd.args(["-p", prompt]);
    cmd.args(["--output-format", "json"]);
    cmd.args(["--model", model]);
    cmd.args(["--max-turns", &max_turns.to_string()]);
    cmd.args([
        "--allowedTools",
        "Bash(void *),Bash(date *),Bash(cat *),Bash(ls *),Bash(echo *)",
    ]);
    cmd.args(["--append-system-prompt", system_prompt]);

    if let Some(sid) = session_id {
        cmd.args(["--resume", sid]);
    }

    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::inherit());

    let output = cmd
        .output()
        .context("Failed to run `claude` CLI. Is Claude Code installed?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("claude exited with {}: {}", output.status, stderr.trim());
    }

    let raw = String::from_utf8(output.stdout).context("Invalid UTF-8 in claude output")?;

    let json: serde_json::Value =
        serde_json::from_str(raw.trim()).context("Failed to parse claude JSON output")?;

    let result = json
        .get("result")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let sid = json
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if json
        .get("is_error")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        bail!("Claude returned an error: {}", result);
    }

    Ok((result, sid))
}

// ---------------------------------------------------------------------------
// rig-core loop — used for Anthropic API, OpenAI, OpenRouter
// ---------------------------------------------------------------------------

async fn run_rig_loop<M: CompletionModel + 'static>(
    agent: Agent<M>,
    max_turns: usize,
    hook: AgentHook,
    initial_prompt: Option<String>,
) -> Result<()> {
    let mut history: Vec<Message> = Vec::new();

    let first_input = match initial_prompt {
        Some(prompt) => prompt,
        None => read_user_input()?,
    };

    let mut current_input = first_input;

    loop {
        if current_input.trim().eq_ignore_ascii_case("quit")
            || current_input.trim().eq_ignore_ascii_case("exit")
        {
            eprintln!("\nBye!");
            break;
        }

        if current_input.trim().is_empty() {
            current_input = read_user_input()?;
            continue;
        }

        let response: Result<String, _> = agent
            .prompt(&current_input)
            .with_history(&mut history)
            .max_turns(max_turns)
            .with_hook(hook.clone())
            .await;

        match response {
            Ok(text) => {
                println!("\n{}\n", text);
            }
            Err(e) => {
                eprintln!("\n\x1b[31mError: {}\x1b[0m\n", e);
            }
        }

        current_input = read_user_input()?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Credential resolution
// ---------------------------------------------------------------------------

fn resolve_credentials(config: &AgentConfig) -> Result<ResolvedCredentials> {
    if let Some(ref provider_str) = config.provider {
        let provider: ProviderKind = provider_str.parse()?;
        return find_key_for_provider(&provider);
    }

    // 1. Explicit Anthropic API key always wins
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Ok(ResolvedCredentials {
                provider: ProviderKind::Anthropic,
                api_key: key,
                source: "ANTHROPIC_API_KEY",
            });
        }
    }

    // 2. Claude Code CLI (uses Max/Pro subscription natively)
    if is_claude_code_available() {
        return Ok(ResolvedCredentials {
            provider: ProviderKind::ClaudeCode,
            api_key: String::new(),
            source: "Claude Code CLI",
        });
    }

    // 3. OpenRouter
    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        if !key.is_empty() {
            return Ok(ResolvedCredentials {
                provider: ProviderKind::OpenRouter,
                api_key: key,
                source: "OPENROUTER_API_KEY",
            });
        }
    }

    // 4. OpenAI
    if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        if !key.is_empty() {
            return Ok(ResolvedCredentials {
                provider: ProviderKind::OpenAI,
                api_key: key,
                source: "OPENAI_API_KEY",
            });
        }
    }

    bail!(
        "No API credentials found. Set one of:\n\
         \n\
         Claude Max/Pro (recommended — uses your subscription):\n\
         1. Install Claude Code: npm install -g @anthropic-ai/claude-code && claude auth login\n\
         \n\
         Anthropic API:\n\
         2. export ANTHROPIC_API_KEY=\"sk-ant-api03-...\"\n\
         \n\
         OpenRouter:\n\
         3. export OPENROUTER_API_KEY=\"sk-or-...\"\n\
         \n\
         OpenAI:\n\
         4. export OPENAI_API_KEY=\"sk-...\""
    )
}

fn find_key_for_provider(provider: &ProviderKind) -> Result<ResolvedCredentials> {
    match provider {
        ProviderKind::ClaudeCode => {
            if is_claude_code_available() {
                Ok(ResolvedCredentials {
                    provider: ProviderKind::ClaudeCode,
                    api_key: String::new(),
                    source: "Claude Code CLI",
                })
            } else {
                bail!(
                    "Claude Code CLI not found. Install it:\n  \
                     npm install -g @anthropic-ai/claude-code && claude auth login"
                )
            }
        }
        ProviderKind::Anthropic => {
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                if !key.is_empty() {
                    return Ok(ResolvedCredentials {
                        provider: ProviderKind::Anthropic,
                        api_key: key,
                        source: "ANTHROPIC_API_KEY",
                    });
                }
            }
            bail!("ANTHROPIC_API_KEY not set")
        }
        ProviderKind::OpenAI => {
            let key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY not set")?;
            Ok(ResolvedCredentials {
                provider: ProviderKind::OpenAI,
                api_key: key,
                source: "OPENAI_API_KEY",
            })
        }
        ProviderKind::OpenRouter => {
            let key = std::env::var("OPENROUTER_API_KEY").context("OPENROUTER_API_KEY not set")?;
            Ok(ResolvedCredentials {
                provider: ProviderKind::OpenRouter,
                api_key: key,
                source: "OPENROUTER_API_KEY",
            })
        }
    }
}

fn is_claude_code_available() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn default_model(provider: &ProviderKind) -> String {
    match provider {
        ProviderKind::ClaudeCode => "claude-sonnet-4-20250514".to_string(),
        ProviderKind::Anthropic => "claude-sonnet-4-20250514".to_string(),
        ProviderKind::OpenAI => "gpt-4o".to_string(),
        ProviderKind::OpenRouter => "anthropic/claude-sonnet-4".to_string(),
    }
}

fn load_instructions(config: &AgentConfig) -> Result<Option<String>> {
    if let Some(ref path) = config.instructions_file {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read instructions file: {}", path))?;
        Ok(Some(content))
    } else if let Some(ref inline) = config.inline_instructions {
        Ok(Some(inline.clone()))
    } else {
        Ok(None)
    }
}

fn read_user_input() -> Result<String> {
    eprint!("\x1b[1;34m> \x1b[0m");
    std::io::Write::flush(&mut std::io::stderr())?;

    let mut input = String::new();
    let n = std::io::stdin().read_line(&mut input)?;
    if n == 0 {
        return Ok("quit".to_string());
    }
    Ok(input.trim().to_string())
}
