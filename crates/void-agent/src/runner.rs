use anyhow::{Context, Result, bail};
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
    Anthropic,
    OpenAI,
    OpenRouter,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
            "anthropic" | "claude" => Ok(Self::Anthropic),
            "openai" | "gpt" => Ok(Self::OpenAI),
            "openrouter" | "or" => Ok(Self::OpenRouter),
            _ => bail!("Unknown provider '{}'. Supported: anthropic, openai, openrouter", s),
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
    let hook = AgentHook { verbose: config.verbose };

    eprintln!(
        "\x1b[1mVoid Agent\x1b[0m ({} via {}, max turns: {})",
        model_name, creds.source, max_turns
    );
    eprintln!("Type your request, or 'quit' to exit.\n");

    match creds.provider {
        ProviderKind::Anthropic => {
            let client = anthropic::Client::new(&creds.api_key)?;
            let agent = client
                .agent(&model_name)
                .preamble(&system_prompt)
                .max_tokens(DEFAULT_MAX_TOKENS)
                .tool(VoidCommandTool)
                .tool(ShellCommandTool)
                .build();
            run_loop(agent, max_turns, hook, config.initial_prompt).await
        }
        ProviderKind::OpenAI => {
            let client = openai::Client::new(&creds.api_key)?;
            let agent = client
                .agent(&model_name)
                .preamble(&system_prompt)
                .max_tokens(DEFAULT_MAX_TOKENS)
                .tool(VoidCommandTool)
                .tool(ShellCommandTool)
                .build();
            run_loop(agent, max_turns, hook, config.initial_prompt).await
        }
        ProviderKind::OpenRouter => {
            let client = openrouter::Client::new(&creds.api_key)?;
            let agent = client
                .agent(&model_name)
                .preamble(&system_prompt)
                .max_tokens(DEFAULT_MAX_TOKENS)
                .tool(VoidCommandTool)
                .tool(ShellCommandTool)
                .build();
            run_loop(agent, max_turns, hook, config.initial_prompt).await
        }
    }
}

async fn run_loop<M: CompletionModel + 'static>(
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
// Credential resolution — auto-detect the best available credentials
// ---------------------------------------------------------------------------

fn resolve_credentials(config: &AgentConfig) -> Result<ResolvedCredentials> {
    if let Some(ref provider_str) = config.provider {
        let provider: ProviderKind = provider_str.parse()?;
        let (key, source) = find_key_for_provider(&provider)?;
        return Ok(ResolvedCredentials { provider, api_key: key, source });
    }

    // Auto-detect: try each source in priority order
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Ok(ResolvedCredentials {
                provider: ProviderKind::Anthropic,
                api_key: key,
                source: "ANTHROPIC_API_KEY",
            });
        }
    }

    if let Ok(key) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !key.is_empty() {
            return Ok(ResolvedCredentials {
                provider: ProviderKind::Anthropic,
                api_key: key,
                source: "CLAUDE_CODE_OAUTH_TOKEN",
            });
        }
    }

    if let Some(key) = read_claude_code_token_from_keychain() {
        return Ok(ResolvedCredentials {
            provider: ProviderKind::Anthropic,
            api_key: key,
            source: "Claude Code (macOS Keychain)",
        });
    }

    if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
        if !key.is_empty() {
            return Ok(ResolvedCredentials {
                provider: ProviderKind::OpenRouter,
                api_key: key,
                source: "OPENROUTER_API_KEY",
            });
        }
    }

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
         1. If you have Claude Code installed, credentials are detected automatically\n\
         2. Or: export CLAUDE_CODE_OAUTH_TOKEN=\"sk-ant-oat01-...\"\n\
         \n\
         Anthropic API:\n\
         3. export ANTHROPIC_API_KEY=\"sk-ant-api03-...\"\n\
         \n\
         OpenRouter:\n\
         4. export OPENROUTER_API_KEY=\"sk-or-...\"\n\
         \n\
         OpenAI:\n\
         5. export OPENAI_API_KEY=\"sk-...\""
    )
}

fn find_key_for_provider(provider: &ProviderKind) -> Result<(String, &'static str)> {
    match provider {
        ProviderKind::Anthropic => {
            if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
                if !key.is_empty() { return Ok((key, "ANTHROPIC_API_KEY")); }
            }
            if let Ok(key) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
                if !key.is_empty() { return Ok((key, "CLAUDE_CODE_OAUTH_TOKEN")); }
            }
            if let Some(key) = read_claude_code_token_from_keychain() {
                return Ok((key, "Claude Code (macOS Keychain)"));
            }
            bail!("No Anthropic credentials found. Set ANTHROPIC_API_KEY or CLAUDE_CODE_OAUTH_TOKEN, or log in to Claude Code.")
        }
        ProviderKind::OpenAI => {
            let key = std::env::var("OPENAI_API_KEY")
                .context("OPENAI_API_KEY not set")?;
            Ok((key, "OPENAI_API_KEY"))
        }
        ProviderKind::OpenRouter => {
            let key = std::env::var("OPENROUTER_API_KEY")
                .context("OPENROUTER_API_KEY not set")?;
            Ok((key, "OPENROUTER_API_KEY"))
        }
    }
}

/// Try reading the Claude Code OAuth token from the macOS Keychain.
/// Returns None on non-macOS or if Claude Code is not configured.
fn read_claude_code_token_from_keychain() -> Option<String> {
    #[cfg(not(target_os = "macos"))]
    { return None; }

    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let raw = String::from_utf8(output.stdout).ok()?;
        let json: serde_json::Value = serde_json::from_str(raw.trim()).ok()?;

        json.get("claudeAiOauth")
            .and_then(|o| o.get("accessToken"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                json.get("accessToken")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
    }
}

fn default_model(provider: &ProviderKind) -> String {
    match provider {
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
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
