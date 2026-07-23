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

/// Global CLI flags the MCP `run` tool already injects (or must not override).
const REJECTED_GLOBALS: &[&str] = &[
    "--store",
    "--config",
    "--verbose",
    "-v",
    "--no-context",
    "--local-store",
];

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

    // Stdio MCP is sequential; wrap in `spawn_blocking` only if a concurrent
    // transport (e.g. SSE) is added later.
    let output = cmd
        .output()
        .map_err(|e| anyhow::anyhow!("failed to execute void subprocess: {e}"))?;

    Ok(ExecResult {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn flag_name(arg: &str) -> &str {
    arg.split('=').next().unwrap_or(arg)
}

fn is_flag(arg: &str) -> bool {
    arg.starts_with('-')
}

/// First non-flag token is the subcommand (skips any leading option tokens).
fn first_subcommand(args: &[String]) -> Option<&str> {
    args.iter().map(String::as_str).find(|a| !is_flag(a))
}

fn positional_args(args: &[String]) -> impl Iterator<Item = &str> {
    args.iter().map(String::as_str).filter(|a| !is_flag(a))
}

pub fn validate_run_args(args: &[String]) -> anyhow::Result<()> {
    for arg in args {
        let name = flag_name(arg);
        if REJECTED_GLOBALS.contains(&name) {
            anyhow::bail!(
                "do not pass `{name}` inside run args — the MCP server already injects \
                 --store/--config (and exposes no_context on the tool)"
            );
        }
    }

    if first_subcommand(args).is_none() {
        anyhow::bail!("args must include a void subcommand (e.g. [\"slack\", \"saved\"])");
    }

    // Scan every positional token so a leading flag+value (e.g. `-n 10 mcp`)
    // cannot hide a blocked subcommand.
    for sub in positional_args(args) {
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

    #[test]
    fn validate_finds_subcommand_after_non_global_flags() {
        // args[0] is a flag; the blocklist must still see `mcp`.
        assert!(validate_run_args(&["-n".into(), "10".into(), "mcp".into()]).is_err());
    }

    #[test]
    fn validate_rejects_global_flags() {
        assert!(
            validate_run_args(&["--config".into(), "/tmp/c.toml".into(), "inbox".into()]).is_err()
        );
        assert!(validate_run_args(&["inbox".into(), "--verbose".into()]).is_err());
        assert!(validate_run_args(&["-v".into(), "inbox".into()]).is_err());
        assert!(validate_run_args(&["--store=/tmp/x".into(), "inbox".into()]).is_err());
        assert!(validate_run_args(&["inbox".into(), "--no-context".into()]).is_err());
        assert!(validate_run_args(&["inbox".into(), "--local-store".into()]).is_err());
    }

    #[test]
    fn validate_allows_sync_status() {
        assert!(validate_run_args(&["sync".into(), "--status".into()]).is_ok());
    }
}
