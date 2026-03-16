use clap::Args;
use tracing::debug;
use void_core::config::{self, VoidConfig};
use void_core::db::Database;

#[derive(Debug, Args)]
pub struct MuteArgs {
    /// Channel/conversation names or IDs to mute (supports partial match)
    pub targets: Vec<String>,
    /// Unmute instead of mute
    #[arg(long)]
    pub unmute: bool,
    /// Filter by account (partial match on account_id)
    #[arg(long)]
    pub account: Option<String>,
    /// Filter by connector (slack, gmail, whatsapp)
    #[arg(long)]
    pub connector: Option<String>,
    /// List all currently muted conversations
    #[arg(long)]
    pub list: bool,
}

pub fn run(args: &MuteArgs, json: bool) -> anyhow::Result<()> {
    let cfg = VoidConfig::load_or_default(&config::default_config_path());
    let db = Database::open(&cfg.db_path())?;

    if args.list {
        return list_muted(&db, args, json);
    }

    if args.targets.is_empty() {
        anyhow::bail!("provide at least one channel/conversation name or ID, or use --list");
    }

    let is_muted = !args.unmute;
    let action = if is_muted { "muted" } else { "unmuted" };
    let mut results = Vec::new();

    for target in &args.targets {
        debug!(target, is_muted, "processing mute target");

        if let Some(conv) = db.get_conversation(target)? {
            db.update_conversation_mute(&conv.id, is_muted)?;
            let name = conv.name.as_deref().unwrap_or(&conv.id);
            eprintln!("{action}: {name} ({})", conv.id);
            results.push(serde_json::json!({
                "id": conv.id,
                "name": name,
                "is_muted": is_muted,
            }));
            continue;
        }

        let matches = db.list_channels(
            args.account.as_deref(),
            args.connector.as_deref(),
            Some(target),
            100,
            true,
        )?;

        let dm_matches = find_conversations_by_name(
            &db,
            target,
            args.account.as_deref(),
            args.connector.as_deref(),
        )?;

        let all_matches: Vec<_> = matches.into_iter().chain(dm_matches).collect();

        if all_matches.is_empty() {
            eprintln!("no conversation matching \"{target}\" found");
            results.push(serde_json::json!({
                "target": target,
                "error": "not found",
            }));
            continue;
        }

        for conv in &all_matches {
            db.update_conversation_mute(&conv.id, is_muted)?;
            let name = conv.name.as_deref().unwrap_or(&conv.id);
            eprintln!("{action}: {name} [{}] ({})", conv.connector, conv.id);
            results.push(serde_json::json!({
                "id": conv.id,
                "name": name,
                "connector": conv.connector,
                "is_muted": is_muted,
            }));
        }
    }

    if json {
        println!("{}", serde_json::json!({ "data": results, "error": null }));
    }
    Ok(())
}

fn list_muted(db: &Database, args: &MuteArgs, json: bool) -> anyhow::Result<()> {
    let all = db.list_conversations(
        args.account.as_deref(),
        args.connector.as_deref(),
        500,
        true,
    )?;
    let muted: Vec<_> = all.into_iter().filter(|c| c.is_muted).collect();

    if json {
        let items: Vec<_> = muted
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "connector": c.connector,
                    "kind": c.kind.to_string(),
                })
            })
            .collect();
        println!("{}", serde_json::json!({ "data": items, "error": null }));
    } else if muted.is_empty() {
        eprintln!("no muted conversations");
    } else {
        eprintln!("{} muted conversation(s):\n", muted.len());
        for c in &muted {
            let name = c.name.as_deref().unwrap_or(&c.id);
            eprintln!("  {} [{}] ({})", name, c.connector, c.id);
        }
    }
    Ok(())
}

fn find_conversations_by_name(
    db: &Database,
    search: &str,
    account_filter: Option<&str>,
    connector_filter: Option<&str>,
) -> anyhow::Result<Vec<void_core::models::Conversation>> {
    let all = db.list_conversations(account_filter, connector_filter, 500, true)?;
    let lower = search.to_lowercase();
    Ok(all
        .into_iter()
        .filter(|c| {
            c.name
                .as_ref()
                .is_some_and(|n| n.to_lowercase().contains(&lower))
        })
        .collect())
}
