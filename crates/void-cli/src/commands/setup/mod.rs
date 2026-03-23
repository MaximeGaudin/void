mod auth;
mod calendar;
mod config_ui;
mod connection_menu;
mod gdrive;
mod gmail;
mod hackernews;
mod prompt;
mod slack;
mod telegram;
mod whatsapp;
mod wizard;

use void_core::config::{self, VoidConfig};

use self::config_ui::{edit_config_file, show_configuration};
use self::connection_menu::{
    add_connection, reauthenticate_connection, remove_connection, rename_connection,
};
use self::prompt::select;
use self::wizard::{exit_setup, run_full_wizard};

pub async fn run() -> anyhow::Result<()> {
    let config_path = config::default_config_path();

    // If no config exists, create default and enter menu
    if !config_path.exists() {
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&config_path, config::default_config())?;
        eprintln!("Created default config at {}", config_path.display());
        eprintln!();
    }

    let mut cfg = VoidConfig::load_or_default(&config_path);
    let store_path = cfg.store_path();
    std::fs::create_dir_all(&store_path)?;

    loop {
        show_menu_header(&cfg);

        let options = if cfg.connections.is_empty() {
            vec![
                "Run full setup wizard",
                "Add a connection",
                "Show configuration",
                "Edit config file",
                "Done",
            ]
        } else {
            vec![
                "Add a connection",
                "Remove a connection",
                "Rename a connection",
                "Re-authenticate a connection",
                "Show configuration",
                "Edit config file",
                "Run full setup wizard",
                "Done",
            ]
        };

        if cfg.connections.is_empty() {
            eprintln!("No connections configured yet. Run the full setup wizard to get started.");
            eprintln!();
        }

        let choice = select("What would you like to do?", &options);

        let action_idx = if cfg.connections.is_empty() {
            match choice {
                0 => 7, // Wizard
                1 => 1, // Add
                2 => 5, // Show
                3 => 6, // Edit
                4 => 8, // Done
                _ => continue,
            }
        } else {
            choice + 1
        };

        match action_idx {
            1 => {
                add_connection(&mut cfg, &store_path).await?;
                cfg.save(&config_path)?;
                eprintln!("\nConfiguration saved.");
            }
            2 => {
                remove_connection(&mut cfg)?;
                cfg.save(&config_path)?;
                eprintln!("\nConnection removed. Configuration saved.");
            }
            3 => {
                rename_connection(&mut cfg, &store_path)?;
                cfg.save(&config_path)?;
                eprintln!("\nConnection renamed. Configuration saved.");
            }
            4 => {
                reauthenticate_connection(&cfg, &store_path).await?;
            }
            5 => {
                show_configuration(&config_path, &cfg);
            }
            6 => {
                edit_config_file(&config_path)?;
                // Reload config after edit
                cfg = VoidConfig::load_or_default(&config_path);
            }
            7 => {
                run_full_wizard(&mut cfg, &store_path, &config_path).await?;
                // Wizard saves and may prompt for sync; loop continues
            }
            8 => {
                return exit_setup(&cfg).await;
            }
            _ => {}
        }

        eprintln!();
    }
}

fn show_menu_header(cfg: &VoidConfig) {
    eprintln!("╔══════════════════════════════════════════════╗");
    eprintln!("║              Void — Setup                    ║");
    eprintln!("╚══════════════════════════════════════════════╝");
    eprintln!();

    if cfg.connections.is_empty() {
        eprintln!("Current connections: (none)");
    } else {
        eprintln!("Current connections:");
        for acc in &cfg.connections {
            eprintln!("  • {} ({})", acc.id, acc.connector_type);
        }
    }
    eprintln!();
}
