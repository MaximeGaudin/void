#[derive(Debug, Clone)]
pub struct HookExecResult {
    pub input_prompt: String,
    pub raw_output: String,
    pub result_summary: String,
    pub success: bool,
    pub error: Option<String>,
}

pub fn execute_hook_public(
    agent: &str,
    prompt: &str,
    max_turns: usize,
) -> anyhow::Result<HookExecResult> {
    execute_hook_blocking(agent, prompt, max_turns)
}

pub(crate) fn execute_hook_blocking(
    agent: &str,
    prompt: &str,
    max_turns: usize,
) -> anyhow::Result<HookExecResult> {
    let mut cmd = std::process::Command::new(agent);
    cmd.args(["-p", prompt]);
    cmd.args(["--verbose"]);
    cmd.args(["--output-format", "stream-json"]);
    cmd.args(["--max-turns", &max_turns.to_string()]);
    cmd.args(["--allowedTools", "Bash(void *),Bash(date *),Bash(echo *)"]);
    cmd.stdin(std::process::Stdio::null());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let output = cmd.output()?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        let error_msg = format!("{} exited with {}: {}", agent, output.status, stderr.trim());
        return Ok(HookExecResult {
            input_prompt: prompt.to_string(),
            raw_output: if stdout.is_empty() {
                stderr.clone()
            } else {
                stdout
            },
            result_summary: String::new(),
            success: false,
            error: Some(error_msg),
        });
    }

    let result_summary = extract_result_from_stream(&stdout);

    Ok(HookExecResult {
        input_prompt: prompt.to_string(),
        raw_output: stdout,
        result_summary,
        success: true,
        error: None,
    })
}

fn extract_result_from_stream(stream: &str) -> String {
    let mut result = String::new();
    for line in stream.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if json.get("type").and_then(|t| t.as_str()) == Some("result") {
                if let Some(r) = json.get("result").and_then(|v| v.as_str()) {
                    return r.to_string();
                }
            }
            if json.get("type").and_then(|t| t.as_str()) == Some("assistant") {
                if let Some(content) = json.pointer("/message/content") {
                    if let Some(arr) = content.as_array() {
                        for block in arr {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                                    if !text.is_empty() {
                                        result = text.to_string();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    result
}
