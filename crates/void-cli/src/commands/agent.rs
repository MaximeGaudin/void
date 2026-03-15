use clap::Args;
use void_agent::AgentConfig;

#[derive(Debug, Args)]
pub struct AgentArgs {
    /// LLM model to use (default: claude-sonnet-4-20250514)
    #[arg(long)]
    pub model: Option<String>,

    /// Path to a file with additional instructions (appended to the system prompt)
    #[arg(long)]
    pub instructions: Option<String>,

    /// Inline additional instructions
    #[arg(long = "system")]
    pub system_prompt: Option<String>,

    /// Maximum tool-call turns per interaction
    #[arg(long, default_value = "30")]
    pub max_turns: usize,

    /// Initial prompt (skip interactive input for first message)
    #[arg(trailing_var_arg = true)]
    pub prompt: Vec<String>,
}

pub async fn run(args: &AgentArgs, verbose: bool) -> anyhow::Result<()> {
    let initial_prompt = if args.prompt.is_empty() {
        None
    } else {
        Some(args.prompt.join(" "))
    };

    let config = AgentConfig {
        model: args.model.clone(),
        instructions_file: args.instructions.clone(),
        inline_instructions: args.system_prompt.clone(),
        verbose,
        max_turns: Some(args.max_turns),
        initial_prompt,
    };

    void_agent::run(config).await
}
