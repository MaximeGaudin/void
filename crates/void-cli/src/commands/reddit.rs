use clap::{Args, Subcommand};

use void_core::config::{
    redact_token, settings_set_string_list, settings_set_u32, settings_string,
    settings_string_list, settings_u32, ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;
use void_reddit::api::sanitize_subreddit;

#[derive(Debug, Args)]
pub struct RedditArgs {
    #[command(subcommand)]
    pub command: RedditCommand,
}

#[derive(Debug, Subcommand)]
pub enum RedditCommand {
    /// Show current Reddit configuration
    Config,
    /// Manage watched subreddits
    Subreddits(SubredditsArgs),
    /// Manage watched keywords
    Keywords(KeywordsArgs),
    /// Set the minimum score threshold for posts
    MinScore(MinScoreArgs),
}

#[derive(Debug, Args)]
pub struct SubredditsArgs {
    #[command(subcommand)]
    pub action: SubredditsAction,
}

#[derive(Debug, Subcommand)]
pub enum SubredditsAction {
    /// List watched subreddits
    List,
    /// Add one or more subreddits (comma-separated)
    Add(SubredditValue),
    /// Remove one or more subreddits (comma-separated)
    Remove(SubredditValue),
    /// Replace all subreddits (comma-separated)
    Set(SubredditValue),
}

#[derive(Debug, Args)]
pub struct SubredditValue {
    /// Subreddits (comma-separated, r/ prefix optional)
    #[arg(default_value = "")]
    pub value: String,
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

struct RedditSettings {
    client_id: String,
    client_secret: String,
    subreddits: Vec<String>,
    keywords: Vec<String>,
    min_score: u32,
}

pub fn run(args: &RedditArgs) -> anyhow::Result<()> {
    match &args.command {
        RedditCommand::Config => run_config(),
        RedditCommand::Subreddits(sr) => run_subreddits(sr),
        RedditCommand::Keywords(kw) => run_keywords(kw),
        RedditCommand::MinScore(s) => run_min_score(s),
    }
}

fn run_config() -> anyhow::Result<()> {
    let cfg = crate::context::void_config();
    let s = get_reddit_settings(cfg)?;

    let out = serde_json::json!({
        "data": {
            "client_id": redact_token(&s.client_id),
            "client_secret": redact_token(&s.client_secret),
            "subreddits": s.subreddits,
            "keywords": s.keywords,
            "min_score": s.min_score,
        },
        "error": null,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn run_subreddits(args: &SubredditsArgs) -> anyhow::Result<()> {
    if matches!(args.action, SubredditsAction::List) {
        let cfg = crate::context::void_config();
        let s = get_reddit_settings(cfg)?;
        let out = serde_json::json!({ "data": s.subreddits, "error": null });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let config_path = crate::context::client_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let mut subreddits = get_reddit_settings(&cfg)?.subreddits;

    match &args.action {
        SubredditsAction::List => return Ok(()),
        SubredditsAction::Add(v) => {
            for sub in parse_subreddits(&v.value) {
                if !subreddits.contains(&sub) {
                    subreddits.push(sub);
                }
            }
        }
        SubredditsAction::Remove(v) => {
            let remove = parse_subreddits(&v.value);
            subreddits.retain(|s| !remove.contains(s));
        }
        SubredditsAction::Set(v) => {
            subreddits = parse_subreddits(&v.value);
        }
    }

    if subreddits.is_empty() {
        anyhow::bail!("At least one subreddit is required");
    }

    set_reddit_subreddits(&mut cfg, subreddits.clone())?;
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": subreddits, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

fn run_keywords(args: &KeywordsArgs) -> anyhow::Result<()> {
    if matches!(args.action, KeywordsAction::List) {
        let cfg = crate::context::void_config();
        let s = get_reddit_settings(cfg)?;
        let out = serde_json::json!({ "data": s.keywords, "error": null });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let config_path = crate::context::client_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let mut keywords = get_reddit_settings(&cfg)?.keywords;

    match &args.action {
        KeywordsAction::List => return Ok(()),
        KeywordsAction::Add(v) => {
            for kw in parse_keywords(&v.value) {
                if !keywords.contains(&kw) {
                    keywords.push(kw);
                }
            }
        }
        KeywordsAction::Remove(v) => {
            let remove = parse_keywords(&v.value);
            keywords.retain(|k| !remove.contains(k));
        }
        KeywordsAction::Set(v) => {
            keywords = parse_keywords(&v.value);
        }
    }

    set_reddit_keywords(&mut cfg, keywords.clone())?;
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": keywords, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

fn run_min_score(args: &MinScoreArgs) -> anyhow::Result<()> {
    let config_path = crate::context::client_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    set_reddit_min_score(&mut cfg, args.score)?;
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": { "min_score": args.score }, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

fn reddit_connection_not_found() -> anyhow::Error {
    anyhow::anyhow!("No Reddit connection found in config. Run `void setup` to add one.")
}

fn find_reddit_connection(cfg: &VoidConfig) -> anyhow::Result<&ConnectionConfig> {
    cfg.connections
        .iter()
        .find(|c| c.connector_type == ConnectorType::from_static(void_reddit::CONNECTOR_ID))
        .ok_or_else(reddit_connection_not_found)
}

fn find_reddit_connection_mut(cfg: &mut VoidConfig) -> anyhow::Result<&mut ConnectionConfig> {
    cfg.connections
        .iter_mut()
        .find(|c| c.connector_type == ConnectorType::from_static(void_reddit::CONNECTOR_ID))
        .ok_or_else(reddit_connection_not_found)
}

fn get_reddit_settings(cfg: &VoidConfig) -> anyhow::Result<RedditSettings> {
    let conn = find_reddit_connection(cfg)?;
    Ok(RedditSettings {
        client_id: settings_string(&conn.settings, "client_id")
            .ok_or_else(|| anyhow::anyhow!("missing client_id"))?,
        client_secret: settings_string(&conn.settings, "client_secret")
            .ok_or_else(|| anyhow::anyhow!("missing client_secret"))?,
        subreddits: settings_string_list(&conn.settings, "subreddits"),
        keywords: settings_string_list(&conn.settings, "keywords"),
        min_score: settings_u32(&conn.settings, "min_score").unwrap_or(0),
    })
}

fn set_reddit_subreddits(cfg: &mut VoidConfig, subreddits: Vec<String>) -> anyhow::Result<()> {
    let conn = find_reddit_connection_mut(cfg)?;
    settings_set_string_list(&mut conn.settings, "subreddits", &subreddits);
    Ok(())
}

fn set_reddit_keywords(cfg: &mut VoidConfig, keywords: Vec<String>) -> anyhow::Result<()> {
    let conn = find_reddit_connection_mut(cfg)?;
    settings_set_string_list(&mut conn.settings, "keywords", &keywords);
    Ok(())
}

fn set_reddit_min_score(cfg: &mut VoidConfig, score: u32) -> anyhow::Result<()> {
    let conn = find_reddit_connection_mut(cfg)?;
    settings_set_u32(&mut conn.settings, "min_score", score);
    Ok(())
}

fn parse_keywords(s: &str) -> Vec<String> {
    s.split(',')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn parse_subreddits(s: &str) -> Vec<String> {
    s.split(',')
        .map(|s| sanitize_subreddit(s.trim()))
        .filter(|s| !s.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keywords_empty_yields_empty() {
        assert!(parse_keywords("").is_empty());
    }

    #[test]
    fn parse_keywords_splits_trims_lowercases() {
        assert_eq!(
            parse_keywords(" Rust , , AI "),
            vec!["rust".to_string(), "ai".to_string()]
        );
    }

    #[test]
    fn parse_subreddits_strips_prefix_and_sanitizes() {
        assert_eq!(
            parse_subreddits("r/Rust, programming, start-ups!"),
            vec![
                "rust".to_string(),
                "programming".to_string(),
                "startups".to_string()
            ]
        );
    }
}
