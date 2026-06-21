use std::path::Path;

use void_core::config::VoidConfig;

use crate::connectors;

pub(crate) fn show_configuration(config_path: &Path, cfg: &VoidConfig) {
    eprintln!("Config file: {}", config_path.display());
    eprintln!("Store path:  {}", cfg.store_path().display());
    eprintln!();

    eprintln!("[sync]");
    for plugin in connectors::all() {
        if let Some(default) = plugin.default_poll_interval_secs {
            let secs = cfg.sync.poll_interval_secs(plugin.id, default);
            eprintln!(
                "  {plugin_id}_poll_interval_secs = {secs}",
                plugin_id = plugin.id
            );
        }
    }
    eprintln!(
        "  linkedin_backfill_days        = {}",
        cfg.sync.linkedin_backfill_days()
    );
    eprintln!();

    if cfg.connections.is_empty() {
        eprintln!("No connections configured.");
    } else {
        eprintln!("Connections ({}):", cfg.connections.len());
        for acc in &cfg.connections {
            eprintln!("  - {} ({})", acc.id, acc.connector_type);
            if !acc.ignore_conversations.is_empty() {
                eprintln!("    ignore_conversations: {:?}", acc.ignore_conversations);
            }
            if let Some(plugin) = connectors::by_id(acc.connector_type.as_str()) {
                let mut out = String::new();
                if (plugin.show_config)(&acc.settings, &mut out).is_ok() && !out.is_empty() {
                    eprint!("{out}");
                }
            }
        }
    }
}

pub(crate) fn edit_config_file(config_path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    let fallback_editor = "notepad";
    #[cfg(not(windows))]
    let fallback_editor = "vi";

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| fallback_editor.into());
    let status = std::process::Command::new(&editor)
        .arg(config_path)
        .status()?;
    if !status.success() {
        anyhow::bail!("Editor exited with status: {}", status);
    }
    Ok(())
}
