use std::path::Path;

use void_core::config::{
    empty_settings, settings_set_string, settings_set_string_list, settings_set_u32,
    ConnectionConfig, VoidConfig,
};
use void_core::models::ConnectorType;

use void_withings::auth::{token_cache_path, OAUTH_REDIRECT_URI};
use void_withings::{parse_streams, Stream, ALL_STREAMS, DEFAULT_BACKFILL_DAYS};

use super::auth::{pick_connector_action, ConnectorAction};
use super::prompt::{prompt, prompt_default};

pub(crate) async fn setup_withings(
    cfg: &mut VoidConfig,
    store_path: &Path,
    add_only: bool,
) -> anyhow::Result<()> {
    eprintln!("⌚  WITHINGS");
    eprintln!();
    eprintln!("Syncs Withings health data read-only: body measurements from the scale");
    eprintln!("and blood-pressure monitor, daily activity, sleep, workouts, heart/ECG");
    eprintln!("recordings, and device battery levels.");
    eprintln!();
    eprintln!("First, register an application at https://developer.withings.com/dashboard");
    eprintln!();
    eprintln!("  Target environment   Development");
    eprintln!("  Application name     anything, e.g. Void");
    eprintln!("  Registered URL       {OAUTH_REDIRECT_URI}");
    eprintln!("                       (exactly this — Withings matches the string)");
    eprintln!("  Scopes               user.info, user.metrics, user.activity");
    eprintln!("                       (all three; a missing scope silently empties a stream)");
    eprintln!();
    eprintln!("Withings will warn that localhost and ports other than 80/443 are not");
    eprintln!("supported in production, and that the app is limited to 10 users.");
    eprintln!("Ignore it: the app is yours and you are its only user. Keep Development.");
    eprintln!();
    eprintln!("Then copy the Client ID and Client Secret it gives you.");

    let withings_type = ConnectorType::from_static(void_withings::CONNECTOR_ID);
    if !add_only {
        let existing: Vec<usize> = cfg
            .connections
            .iter()
            .enumerate()
            .filter(|(_, a)| a.connector_type == withings_type)
            .map(|(i, _)| i)
            .collect();

        match pick_connector_action("Withings", &existing, cfg) {
            ConnectorAction::Skip | ConnectorAction::Keep => return Ok(()),
            ConnectorAction::Replace(idx) => {
                cfg.connections.remove(idx);
            }
            ConnectorAction::Add => {}
        }
    }

    eprintln!();
    let client_id = prompt("Withings client ID: ");
    if client_id.trim().is_empty() {
        anyhow::bail!("Withings client ID is required");
    }

    eprintln!();
    let client_secret = prompt("Withings client secret: ");
    if client_secret.trim().is_empty() {
        anyhow::bail!("Withings client secret is required");
    }

    eprintln!();
    eprintln!("Which data streams should land in your inbox?");
    eprintln!("Options: measures (weight, body composition, blood pressure), activity,");
    eprintln!("sleep, workouts, heart (ECG and blood pressure), devices (battery levels).");
    let stream_input = prompt_default(
        "Streams",
        "measures, activity, sleep, workouts, heart, devices",
    );
    let stream_values: Vec<String> = stream_input
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let streams = parse_streams(&stream_values)?;

    eprintln!();
    eprintln!("How far back should the first sync reach?");
    let backfill_input = prompt_default("Backfill days", &DEFAULT_BACKFILL_DAYS.to_string());
    let backfill_days: u32 = backfill_input.parse().unwrap_or(DEFAULT_BACKFILL_DAYS);

    // The token file is keyed by connection id, so the name comes before the
    // browser flow.
    let connection_id = prompt_default("\nAccount name", "withings");

    eprintln!();
    eprintln!("Withings only gives 30 seconds to redeem the authorization code,");
    eprintln!("so the exchange happens the moment you approve access.");
    let client = void_withings::api::WithingsClient::new(
        client_id.trim(),
        client_secret.trim(),
        &token_cache_path(store_path, &connection_id),
    );
    client.authorize_interactive().await?;
    client.probe().await?;
    eprintln!("  ✓ Withings authorized.");

    let mut settings = empty_settings();
    settings_set_string(&mut settings, "client_id", client_id.trim());
    settings_set_string(&mut settings, "client_secret", client_secret.trim());
    settings_set_string_list(&mut settings, "streams", &stream_ids(&streams));
    settings_set_u32(&mut settings, "backfill_days", backfill_days);

    cfg.connections.push(ConnectionConfig {
        id: connection_id,
        connector_type: withings_type,
        ignore_conversations: vec![],
        settings,
    });
    eprintln!("  ✓ Withings configured.");
    Ok(())
}

/// Write the streams back in their canonical spelling, not the user's.
fn stream_ids(streams: &[Stream]) -> Vec<String> {
    if streams == ALL_STREAMS {
        // Every stream is the default; leave the list empty so the config stays
        // readable and picks up any stream added later.
        return vec![];
    }
    streams.iter().map(|s| s.id().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_streams_are_stored_as_the_default_empty_list() {
        assert!(stream_ids(&ALL_STREAMS).is_empty());
    }

    #[test]
    fn a_subset_is_stored_canonically() {
        assert_eq!(
            stream_ids(&[Stream::Activity, Stream::Sleep]),
            vec!["activity".to_string(), "sleep".to_string()]
        );
    }
}
