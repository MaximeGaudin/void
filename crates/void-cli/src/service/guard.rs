//! Outbound guardrails for agents.
//!
//! An agent spawned by a hook reads untrusted inbound messages; one reached
//! through MCP may too. A prompt injection in those messages could make the
//! agent send the user's data to an attacker, so every outbound path (send,
//! reply, forward, Slack schedule / edit) calls into this module first:
//!
//! - **Hook agents** (`VOID_HOOK` set by the hook executor) are refused unless
//!   `[hooks] allow_send = true`. A non-empty `[hooks] allowed_recipients`
//!   then restricts who they may write to.
//! - **MCP** (`void mcp`, and subprocesses of its `run` tool via `VOID_MCP`)
//!   keeps its current behavior unless `[mcp] allowed_recipients` is set.
//!
//! Gmail drafts are deliberately not guarded: a draft needs a human to send it.

use std::sync::atomic::{AtomicBool, Ordering};

use void_core::config::VoidConfig;
use void_core::hooks::HOOK_ENV_VAR;

/// Set on subprocesses of the MCP `run` tool so they apply `[mcp]` rules.
pub const MCP_ENV_VAR: &str = "VOID_MCP";

static MCP_SERVER: AtomicBool = AtomicBool::new(false);

/// Mark this process as the MCP server (called once by `void mcp`).
pub fn mark_mcp_server() {
    MCP_SERVER.store(true, Ordering::Relaxed);
}

/// Who is driving this process, as far as the guardrails are concerned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentContext {
    /// Hook name when spawned (directly or transitively) by a hook agent.
    pub hook: Option<String>,
    /// True for the MCP server and its `run` subprocesses.
    pub mcp: bool,
}

impl AgentContext {
    /// Read the context from the environment and the MCP server flag.
    pub fn detect() -> Self {
        Self::from_parts(
            std::env::var(HOOK_ENV_VAR).ok(),
            std::env::var(MCP_ENV_VAR).ok(),
            MCP_SERVER.load(Ordering::Relaxed),
        )
    }

    fn from_parts(hook: Option<String>, mcp_env: Option<String>, mcp_server: bool) -> Self {
        let hook = hook.filter(|h| !h.trim().is_empty());
        let mcp = mcp_server || mcp_env.is_some_and(|v| !v.trim().is_empty());
        Self { hook, mcp }
    }
}

/// One recipient, known under one or more identifiers (raw `--to`, resolved
/// id, conversation id, external id). It is allowed when any identifier is
/// on the allow-list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    aliases: Vec<String>,
}

impl Recipient {
    pub fn new<I, S>(aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut out: Vec<String> = Vec::new();
        for alias in aliases {
            for a in expand_alias(alias.as_ref()) {
                if !out.contains(&a) {
                    out.push(a);
                }
            }
        }
        Self { aliases: out }
    }

    fn display(&self) -> &str {
        self.aliases.first().map(String::as_str).unwrap_or("")
    }
}

/// `Alice <alice@x.com>` also matches `alice@x.com`.
fn expand_alias(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return vec![];
    }
    let mut out = vec![trimmed.to_string()];
    if let (Some(start), Some(end)) = (trimmed.rfind('<'), trimmed.rfind('>')) {
        if start < end {
            let inner = trimmed[start + 1..end].trim();
            if !inner.is_empty() {
                out.push(inner.to_string());
            }
        }
    }
    out
}

/// Comma-separated recipient list (`--to`, `--cc`, `--bcc`) → one recipient each.
pub fn recipients_from_list(list: Option<&str>) -> Vec<Recipient> {
    list.map(|s| {
        s.split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|p| Recipient::new([p]))
            .collect()
    })
    .unwrap_or_default()
}

/// Refuse outbound `action` outright when a hook agent has not been opted in.
///
/// Called first on every outbound path, before any lookup or network call.
pub fn ensure_agent_may_send(
    cfg: &VoidConfig,
    ctx: &AgentContext,
    action: &str,
) -> anyhow::Result<()> {
    if let Some(hook) = &ctx.hook {
        if !cfg.hooks.allow_send {
            anyhow::bail!(
                "`void {action}` is disabled for agents started by a hook (hook '{hook}', {HOOK_ENV_VAR} is set). \
                 Inbound messages can carry prompt injection, so hook agents cannot send by default. \
                 Create a draft instead with `void gmail draft create`, or opt in with `[hooks] allow_send = true` \
                 (and ideally `[hooks] allowed_recipients = [...]`) in config.toml."
            );
        }
    }
    Ok(())
}

/// Check every recipient of an outbound `action` against the allow-lists that
/// apply to `ctx`. Also enforces [`ensure_agent_may_send`].
pub fn check_recipients(
    cfg: &VoidConfig,
    ctx: &AgentContext,
    action: &str,
    recipients: &[Recipient],
) -> anyhow::Result<()> {
    ensure_agent_may_send(cfg, ctx, action)?;
    if ctx.hook.is_some() {
        check_allow_list(
            &cfg.hooks.allowed_recipients,
            "[hooks] allowed_recipients",
            action,
            recipients,
        )?;
    }
    if ctx.mcp {
        check_allow_list(
            &cfg.mcp.allowed_recipients,
            "[mcp] allowed_recipients",
            action,
            recipients,
        )?;
    }
    Ok(())
}

/// Detect the agent context and check `action` to explicit recipient lists
/// (each entry comma-separated, e.g. `--to`, `--cc`, `--bcc`).
pub fn check_outbound_lists(
    cfg: &VoidConfig,
    action: &str,
    lists: &[Option<&str>],
) -> anyhow::Result<()> {
    let recipients: Vec<Recipient> = lists
        .iter()
        .flat_map(|l| recipients_from_list(*l))
        .collect();
    check_recipients(cfg, &AgentContext::detect(), action, &recipients)
}

/// In remote store mode the command is proxied over SSH before recipients
/// can be resolved, and the agent marker does not travel with it. Refuse
/// when a rule would need that resolution; allow otherwise.
pub fn check_before_proxy(
    cfg: &VoidConfig,
    ctx: &AgentContext,
    action: &str,
) -> anyhow::Result<()> {
    ensure_agent_may_send(cfg, ctx, action)?;
    let hook_list = ctx.hook.is_some() && !cfg.hooks.allowed_recipients.is_empty();
    let mcp_list = ctx.mcp && !cfg.mcp.allowed_recipients.is_empty();
    if hook_list || mcp_list {
        anyhow::bail!(
            "`void {action}` with an `allowed_recipients` list cannot be checked in remote store mode; \
             run the agent on the host that holds the store."
        );
    }
    Ok(())
}

fn check_allow_list(
    allowed: &[String],
    key: &str,
    action: &str,
    recipients: &[Recipient],
) -> anyhow::Result<()> {
    if allowed.is_empty() {
        return Ok(());
    }
    if recipients.is_empty() {
        anyhow::bail!("`void {action}` refused: no recipient to check against {key}");
    }
    for r in recipients {
        let ok = r.aliases.iter().any(|a| {
            allowed
                .iter()
                .any(|allowed| allowed.trim().eq_ignore_ascii_case(a))
        });
        if !ok {
            anyhow::bail!(
                "`void {action}` refused: recipient '{}' is not in {key}. \
                 Create a draft instead with `void gmail draft create`, or add it to {key} in config.toml.",
                r.display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook_ctx() -> AgentContext {
        AgentContext {
            hook: Some("triage".into()),
            mcp: false,
        }
    }

    fn mcp_ctx() -> AgentContext {
        AgentContext {
            hook: None,
            mcp: true,
        }
    }

    fn cfg_with(allow_send: bool, hook_list: &[&str], mcp_list: &[&str]) -> VoidConfig {
        let mut cfg = VoidConfig::default();
        cfg.hooks.allow_send = allow_send;
        cfg.hooks.allowed_recipients = hook_list.iter().map(|s| s.to_string()).collect();
        cfg.mcp.allowed_recipients = mcp_list.iter().map(|s| s.to_string()).collect();
        cfg
    }

    #[test]
    fn context_detects_hook_marker_and_mcp() {
        let ctx = AgentContext::from_parts(Some("triage".into()), None, false);
        assert_eq!(ctx.hook.as_deref(), Some("triage"));
        assert!(!ctx.mcp);

        let ctx = AgentContext::from_parts(Some("  ".into()), Some("1".into()), false);
        assert!(ctx.hook.is_none(), "blank marker is ignored");
        assert!(ctx.mcp);

        let ctx = AgentContext::from_parts(None, None, true);
        assert!(ctx.mcp);

        assert_eq!(
            AgentContext::from_parts(None, None, false),
            AgentContext::default()
        );
    }

    #[test]
    fn hook_marker_blocks_send_reply_forward_by_default() {
        let cfg = VoidConfig::default();
        for action in ["send", "reply", "forward"] {
            let err = ensure_agent_may_send(&cfg, &hook_ctx(), action)
                .unwrap_err()
                .to_string();
            assert!(err.contains(&format!("`void {action}`")), "{err}");
            assert!(err.contains("void gmail draft create"), "{err}");
            assert!(err.contains("allow_send = true"), "{err}");
            assert!(
                check_recipients(&cfg, &hook_ctx(), action, &[Recipient::new(["a@x.com"])])
                    .is_err()
            );
        }
    }

    #[test]
    fn no_agent_context_is_unrestricted() {
        let cfg = cfg_with(false, &["boss@x.com"], &["boss@x.com"]);
        let ctx = AgentContext::default();
        assert!(ensure_agent_may_send(&cfg, &ctx, "send").is_ok());
        assert!(check_recipients(&cfg, &ctx, "send", &[Recipient::new(["evil@x.com"])]).is_ok());
    }

    #[test]
    fn hook_opt_in_allows_any_recipient_when_list_empty() {
        let cfg = cfg_with(true, &[], &[]);
        assert!(check_recipients(
            &cfg,
            &hook_ctx(),
            "send",
            &[Recipient::new(["anyone@x.com"])]
        )
        .is_ok());
    }

    #[test]
    fn hook_allow_list_rejects_unknown_recipient() {
        let cfg = cfg_with(true, &["Me@Example.com", "C0123"], &[]);
        let ok = [
            Recipient::new(["me@example.com"]),
            Recipient::new(["#general", "C0123"]),
        ];
        assert!(check_recipients(&cfg, &hook_ctx(), "send", &ok).is_ok());

        let bad = [
            Recipient::new(["me@example.com"]),
            Recipient::new(["attacker@evil.com"]),
        ];
        let err = check_recipients(&cfg, &hook_ctx(), "forward", &bad)
            .unwrap_err()
            .to_string();
        assert!(err.contains("attacker@evil.com"), "{err}");
        assert!(err.contains("[hooks] allowed_recipients"), "{err}");
    }

    #[test]
    fn allow_list_with_no_recipients_fails_closed() {
        let cfg = cfg_with(true, &["me@example.com"], &[]);
        assert!(check_recipients(&cfg, &hook_ctx(), "send", &[]).is_err());
    }

    #[test]
    fn mcp_default_allows_everything() {
        let cfg = VoidConfig::default();
        assert!(check_recipients(
            &cfg,
            &mcp_ctx(),
            "send",
            &[Recipient::new(["anyone@x.com"])]
        )
        .is_ok());
    }

    #[test]
    fn mcp_allow_list_rejects_unknown_recipient() {
        let cfg = cfg_with(false, &[], &["me@example.com"]);
        assert!(check_recipients(
            &cfg,
            &mcp_ctx(),
            "reply",
            &[Recipient::new(["me@example.com"])]
        )
        .is_ok());
        let err = check_recipients(&cfg, &mcp_ctx(), "reply", &[Recipient::new(["x@evil.com"])])
            .unwrap_err()
            .to_string();
        assert!(err.contains("[mcp] allowed_recipients"), "{err}");
    }

    #[test]
    fn hook_inside_mcp_applies_both_rules() {
        let ctx = AgentContext {
            hook: Some("triage".into()),
            mcp: true,
        };
        // Hook not opted in: blocked even though MCP has no list.
        assert!(check_recipients(&VoidConfig::default(), &ctx, "send", &[]).is_err());
        // Opted in, but the MCP list still applies.
        let cfg = cfg_with(true, &[], &["me@example.com"]);
        assert!(check_recipients(&cfg, &ctx, "send", &[Recipient::new(["x@evil.com"])]).is_err());
    }

    #[test]
    fn display_name_address_matches_inner_email() {
        let cfg = cfg_with(true, &["alice@example.com"], &[]);
        let r = recipients_from_list(Some("Alice <alice@example.com>, "));
        assert_eq!(r.len(), 1);
        assert!(check_recipients(&cfg, &hook_ctx(), "send", &r).is_ok());
    }

    #[test]
    fn bcc_recipients_are_checked() {
        let cfg = cfg_with(true, &["alice@example.com"], &[]);
        let mut r = vec![Recipient::new(["alice@example.com"])];
        r.extend(recipients_from_list(Some("alice@example.com,spy@evil.com")));
        assert!(check_recipients(&cfg, &hook_ctx(), "send", &r).is_err());
    }

    #[test]
    fn before_proxy_blocks_unverifiable_lists() {
        assert!(check_before_proxy(&VoidConfig::default(), &hook_ctx(), "send").is_err());
        assert!(check_before_proxy(&cfg_with(true, &[], &[]), &hook_ctx(), "send").is_ok());
        assert!(
            check_before_proxy(&cfg_with(true, &["a@x.com"], &[]), &hook_ctx(), "send").is_err()
        );
        assert!(check_before_proxy(&VoidConfig::default(), &mcp_ctx(), "send").is_ok());
        assert!(
            check_before_proxy(&cfg_with(false, &[], &["a@x.com"]), &mcp_ctx(), "send").is_err()
        );
        assert!(
            check_before_proxy(&VoidConfig::default(), &AgentContext::default(), "send").is_ok()
        );
    }
}
