use clap::{Args, Subcommand};

use void_core::config::{
    settings_set_string, settings_set_string_list, settings_string, settings_string_list,
    ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;

#[derive(Debug, Args)]
pub struct GoogleNewsArgs {
    #[command(subcommand)]
    pub command: GnCommand,
}

#[derive(Debug, Subcommand)]
pub enum GnCommand {
    /// Show current Google News configuration
    Config,
    /// Manage watched keywords
    Keywords(KeywordsArgs),
    /// Set the recency window (e.g. 24h, 7d; empty to clear)
    When(WhenArgs),
    /// Set the UI language (hl parameter, e.g. fr, en)
    Language(LanguageArgs),
    /// Set the country edition (gl parameter, e.g. FR, US)
    Country(CountryArgs),
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
pub struct WhenArgs {
    /// Recency window, e.g. 24h, 7d (empty to clear)
    #[arg(default_value = "")]
    pub value: String,
}

#[derive(Debug, Args)]
pub struct LanguageArgs {
    /// UI language code (hl), e.g. fr, en
    pub value: String,
}

#[derive(Debug, Args)]
pub struct CountryArgs {
    /// Country edition code (gl), e.g. FR, US
    pub value: String,
}

pub fn run(args: &GoogleNewsArgs) -> anyhow::Result<()> {
    match &args.command {
        GnCommand::Config => run_config(),
        GnCommand::Keywords(kw) => run_keywords(kw),
        GnCommand::When(w) => run_when(w),
        GnCommand::Language(l) => run_language(l),
        GnCommand::Country(c) => run_country(c),
    }
}

struct GnSettings {
    keywords: Vec<String>,
    when: String,
    language: String,
    country: String,
}

fn run_config() -> anyhow::Result<()> {
    let cfg = crate::context::void_config();
    let s = get_gn_settings(cfg)?;

    let out = serde_json::json!({
        "data": {
            "keywords": s.keywords,
            "when": s.when,
            "language": s.language,
            "country": s.country,
        },
        "error": null,
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

fn run_keywords(args: &KeywordsArgs) -> anyhow::Result<()> {
    if matches!(args.action, KeywordsAction::List) {
        let cfg = crate::context::void_config();
        let s = get_gn_settings(cfg)?;
        let out = serde_json::json!({ "data": s.keywords, "error": null });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }

    let config_path = crate::context::client_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let mut keywords = get_gn_settings(&cfg)?.keywords;

    match &args.action {
        KeywordsAction::List => return Ok(()),
        KeywordsAction::Add(v) => {
            for kw in parse_csv(&v.value) {
                if !keywords.contains(&kw) {
                    keywords.push(kw);
                }
            }
        }
        KeywordsAction::Remove(v) => {
            let remove = parse_csv(&v.value);
            keywords.retain(|k| !remove.contains(k));
        }
        KeywordsAction::Set(v) => {
            keywords = parse_csv(&v.value);
        }
    }

    set_gn_keywords(&mut cfg, keywords.clone())?;
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": keywords, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

/// Which scalar GoogleNews field a setter targets.
enum GnField {
    When,
    Language,
    Country,
}

fn run_when(args: &WhenArgs) -> anyhow::Result<()> {
    update_setting(GnField::When, args.value.trim().to_lowercase(), "when")
}

fn run_language(args: &LanguageArgs) -> anyhow::Result<()> {
    update_setting(
        GnField::Language,
        args.value.trim().to_lowercase(),
        "language",
    )
}

fn run_country(args: &CountryArgs) -> anyhow::Result<()> {
    update_setting(
        GnField::Country,
        args.value.trim().to_uppercase(),
        "country",
    )
}

/// Set a single scalar field on the GoogleNews settings, save, and print the new value.
fn update_setting(field: GnField, value: String, field_name: &str) -> anyhow::Result<()> {
    let config_path = crate::context::client_config_path();
    let mut cfg = VoidConfig::load(&config_path)
        .map_err(|e| anyhow::anyhow!("Cannot load config: {e}\nRun `void setup` first."))?;

    let conn = find_gn_connection_mut(&mut cfg)?;
    match field {
        GnField::When => settings_set_string(&mut conn.settings, "when", &value),
        GnField::Language => settings_set_string(&mut conn.settings, "language", &value),
        GnField::Country => settings_set_string(&mut conn.settings, "country", &value),
    }
    cfg.save(&config_path)?;

    let out = serde_json::json!({ "data": { field_name: value }, "error": null });
    println!("{}", serde_json::to_string_pretty(&out)?);
    eprintln!("Restart `void sync` for changes to take effect.");
    Ok(())
}

fn gn_connection_not_found() -> anyhow::Error {
    anyhow::anyhow!("No Google News connection found in config. Run `void setup` to add one.")
}

fn find_gn_connection(cfg: &VoidConfig) -> anyhow::Result<&ConnectionConfig> {
    cfg.connections
        .iter()
        .find(|c| c.connector_type == ConnectorType::from_static(void_googlenews::CONNECTOR_ID))
        .ok_or_else(gn_connection_not_found)
}

fn find_gn_connection_mut(cfg: &mut VoidConfig) -> anyhow::Result<&mut ConnectionConfig> {
    cfg.connections
        .iter_mut()
        .find(|c| c.connector_type == ConnectorType::from_static(void_googlenews::CONNECTOR_ID))
        .ok_or_else(gn_connection_not_found)
}

fn get_gn_settings(cfg: &VoidConfig) -> anyhow::Result<GnSettings> {
    let conn = find_gn_connection(cfg)?;
    Ok(GnSettings {
        keywords: settings_string_list(&conn.settings, "keywords"),
        when: settings_string(&conn.settings, "when")
            .unwrap_or_default()
            .to_string(),
        language: settings_string(&conn.settings, "language")
            .unwrap_or_default()
            .to_string(),
        country: settings_string(&conn.settings, "country")
            .unwrap_or_default()
            .to_string(),
    })
}

fn set_gn_keywords(cfg: &mut VoidConfig, keywords: Vec<String>) -> anyhow::Result<()> {
    let conn = find_gn_connection_mut(cfg)?;
    settings_set_string_list(&mut conn.settings, "keywords", &keywords);
    Ok(())
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
    fn parse_csv_splits_trims_lowercases() {
        assert_eq!(
            parse_csv(" Rust , , AI "),
            vec!["rust".to_string(), "ai".to_string()]
        );
    }

    #[test]
    fn parse_csv_empty_yields_empty() {
        assert!(parse_csv("").is_empty());
    }
}
