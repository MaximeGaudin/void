use anyhow::{Context, Result};
use rig::client::CompletionClient;
use rig::completion::Prompt;
use rig::completion::message::Message;
use rig::providers::anthropic;

use crate::hook::AgentHook;
use crate::prompt::build_system_prompt;
use crate::tools::{ShellCommandTool, VoidCommandTool};

const DEFAULT_MODEL: &str = "claude-sonnet-4-20250514";
const DEFAULT_MAX_TOKENS: u64 = 16384;
const DEFAULT_MAX_TURNS: usize = 30;

pub struct AgentConfig {
    pub model: Option<String>,
    pub instructions_file: Option<String>,
    pub inline_instructions: Option<String>,
    pub verbose: bool,
    pub max_turns: Option<usize>,
    pub initial_prompt: Option<String>,
}

pub async fn run(config: AgentConfig) -> Result<()> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .context("ANTHROPIC_API_KEY environment variable is required for void agent")?;

    let client = anthropic::Client::new(&api_key)?;

    let custom_instructions = load_instructions(&config)?;
    let system_prompt = build_system_prompt(custom_instructions.as_deref());

    let model_name = config.model.as_deref().unwrap_or(DEFAULT_MODEL);
    let max_turns = config.max_turns.unwrap_or(DEFAULT_MAX_TURNS);

    let hook = AgentHook {
        verbose: config.verbose,
    };

    let agent = client
        .agent(model_name)
        .preamble(&system_prompt)
        .max_tokens(DEFAULT_MAX_TOKENS)
        .tool(VoidCommandTool)
        .tool(ShellCommandTool)
        .build();

    eprintln!(
        "\x1b[1mVoid Agent\x1b[0m (model: {}, max turns: {})",
        model_name, max_turns
    );
    eprintln!("Type your request, or 'quit' to exit.\n");

    let mut history: Vec<Message> = Vec::new();

    let first_input = match config.initial_prompt {
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
