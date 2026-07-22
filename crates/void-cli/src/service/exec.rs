use std::path::Path;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ExecResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct ExecParams<'a> {
    pub args: &'a [String],
    pub store: Option<&'a Path>,
    pub config: Option<&'a Path>,
    pub no_context: bool,
}

/// Run a void CLI subcommand in a child process (same binary) and capture output.
///
/// This keeps MCP in full parity with the CLI without duplicating every subcommand as a tool.
pub fn run_subcommand(params: &ExecParams<'_>) -> anyhow::Result<ExecResult> {
    validate_run_args(params.args)?;

    let exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("cannot resolve void executable path: {e}"))?;

    let mut cmd = Command::new(exe);
    if let Some(store) = params.store {
        cmd.arg("--store").arg(store);
    }
    if let Some(config) = params.config {
        cmd.arg("--config").arg(config);
    }
    if params.no_context {
        cmd.arg("--no-context");
    }
    cmd.args(params.args);

    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("failed to execute void subprocess: {e}"))?;

    Ok(ExecResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub fn validate_run_args(args: &[String]) -> anyhow::Result<()> {
    let Some(sub) = args.first().map(String::as_str) else {
        anyhow::bail!("args must include a void subcommand (e.g. [\"slack\", \"saved\"])");
    };

    match sub {
        "mcp" => anyhow::bail!("cannot invoke `void mcp` via the run tool"),
        "setup" => anyhow::bail!("`void setup` is interactive — run it in a terminal"),
        "sync"
            if args
                .iter()
                .any(|a| a == "--daemon" || a == "--daemon-inner") =>
        {
            anyhow::bail!("`void sync --daemon` must be started from a terminal")
        }
        _ => {}
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_empty_args() {
        assert!(validate_run_args(&[]).is_err());
    }

    #[test]
    fn validate_rejects_mcp_recursion() {
        assert!(validate_run_args(&["mcp".into()]).is_err());
    }

    #[test]
    fn validate_rejects_setup() {
        assert!(validate_run_args(&["setup".into()]).is_err());
    }

    #[test]
    fn validate_rejects_sync_daemon() {
        assert!(validate_run_args(&["sync".into(), "--daemon".into()]).is_err());
    }

    #[test]
    fn validate_allows_slack_saved() {
        assert!(validate_run_args(&["slack".into(), "saved".into()]).is_ok());
    }
}
