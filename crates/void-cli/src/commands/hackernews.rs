use clap::{Args, Subcommand};

use void_core::config::{self, ConnectionConfig, ConnectionSettings, VoidConfig};
use void_core::models::ConnectorType;

#[derive(Debug, Args)]
pub struct HackerNewsArgs {
    #[command(subcommand)]
    pub command: HnCommand,
}

#[derive(Debug, Subcommand)]
pub enum HnCommand {
    /// Show current Hacker News configuration
    Config,
    /// Manage watched keywords
    Keywords(KeywordsArgs),
    /// Set the minimum score threshold for stories
    MinScore(MinScoreArgs),
}

#[derive(Debug, Args)]
pub struct KeywordsArgs {
    #[command(subcommand)]
    pub action: KeywordsAction,
}

#[derive(Debug, Subcommand)]
pub enum KeywordsAction {
    /// List current keywords
    List,
    /// Add one or more keywords (comma-separated)
    Add(KeywordValue),
    /// Remove one or more keywords (comma-separated)
    Remove(KeywordValue),
    /// Replace all keywords (comma-separated, or empty to clear)
    Set(KeywordValue),
}

#[derive(Debug, Args)]
pub struct KeywordValue {
    /// Keywords (comma-separated)
    #[arg(default_value = "")]
    pub value: String,
}

#[derive(Debug, Args)]
pub struct MinScoreArgs {
    /// New minimum score
    pub score: u32,
}

pub fn run(args: &HackerNewsArgs) -> anyhow::Result<()> {
    match &args.command {
        HnCommand::Config => run_config(),
        HnCommand::Keywords(kw) => run_keywords(kw),
        HnCommand::MinScore(s) => run_min_score(s),
    }
}

fn run_config() -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let (keywords, min_score) = get_hn_settings(&cfg)?;

    let out = serde_json::json!({
        "data": {
            "keywords": keywords,
            "min_score": min_score,
        },
        "error": null,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn run_keywords(args: &KeywordsArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let (mut keywords, _) = get_hn_settings(&cfg)?;

    match &args.action {
        KeywordsAction::List => {
            let out = serde_json::json!({ "data": keywords, "error": null });
            println!("{}", serde_json::to_string_pretty(&out)?);
            return Ok(());
        }
        KeywordsAction::Add(v) => {
            let new: Vec<String> = parse_csv(&v.value);
            for kw in new {
                if !keywords.contains(&kw) {
                    keywords.push(kw);
                }
            }
        }
        KeywordsAction::Remove(v) => {
            let remove: Vec<String> = parse_csv(&v.value);
            keywords.retain(|k| !remove.contains(k));
        }
        KeywordsAction::Set(v) => {
            keywords = parse_csv(&v.value);
        }
    }

    set_hn_keywords(&mut cfg, keywords.clone())?;
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": keywords, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

fn run_min_score(args: &MinScoreArgs) -> anyhow::Result<()> {
    let config_path = config::default_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    set_hn_min_score(&mut cfg, args.score)?;
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": { "min_score": args.score }, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

fn hn_connection_not_found() -> anyhow::Error {
    anyhow::anyhow!("No Hacker News connection found in config. Run `void setup` to add one.")
}

fn find_hn_connection(cfg: &VoidConfig) -> anyhow::Result<&ConnectionConfig> {
    cfg.connections
        .iter()
        .find(|c| c.connector_type == ConnectorType::HackerNews)
        .ok_or_else(hn_connection_not_found)
}

fn find_hn_connection_mut(cfg: &mut VoidConfig) -> anyhow::Result<&mut ConnectionConfig> {
    cfg.connections
        .iter_mut()
        .find(|c| c.connector_type == ConnectorType::HackerNews)
        .ok_or_else(hn_connection_not_found)
}

fn get_hn_settings(cfg: &VoidConfig) -> anyhow::Result<(Vec<String>, u32)> {
    let conn = find_hn_connection(cfg)?;
    match &conn.settings {
        ConnectionSettings::HackerNews {
            keywords,
            min_score,
        } => Ok((keywords.clone(), *min_score)),
        _ => anyhow::bail!("Unexpected settings type for HackerNews connection"),
    }
}

fn set_hn_keywords(cfg: &mut VoidConfig, keywords: Vec<String>) -> anyhow::Result<()> {
    let conn = find_hn_connection_mut(cfg)?;
    match &mut conn.settings {
        ConnectionSettings::HackerNews {
            keywords: ref mut kw,
            ..
        } => {
            *kw = keywords;
            Ok(())
        }
        _ => anyhow::bail!("Unexpected settings type for HackerNews connection"),
    }
}

fn set_hn_min_score(cfg: &mut VoidConfig, score: u32) -> anyhow::Result<()> {
    let conn = find_hn_connection_mut(cfg)?;
    match &mut conn.settings {
        ConnectionSettings::HackerNews {
            min_score: ref mut ms,
            ..
        } => {
            *ms = score;
            Ok(())
        }
        _ => anyhow::bail!("Unexpected settings type for HackerNews connection"),
    }
}

fn parse_csv(s: &str) -> Vec<String> {
    s.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_csv;

    #[test]
    fn parse_csv_empty_yields_empty() {
        assert!(parse_csv("").is_empty());
    }

    #[test]
    fn parse_csv_splits_trims_lowercases() {
        assert_eq!(
            parse_csv(" Rust , , AI "),
            vec!["rust".to_string(), "ai".to_string()]
        );
    }

    #[test]
    fn parse_csv_single_token() {
        assert_eq!(parse_csv("keyword"), vec!["keyword".to_string()]);
    }
}
