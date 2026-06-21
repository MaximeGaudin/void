# Adding a Connector

This guide walks through adding a new communication connector to Void. The compile-time plugin registry keeps connector wiring in one place per connector instead of ~13 central files.

Replace placeholders throughout:

- `Acme` — PascalCase name (e.g. `Telegram`, `GitHub`)
- `acme` — lowercase id (e.g. `telegram`, `github`)
- `AC` — two-letter badge (e.g. `TG`, `GH`)
- `am` — short CLI alias (e.g. `tg`, `gh`)

---

## Architecture

```
Cargo.toml                          # workspace members + deps
crates/
  void-acme/                        # NEW — connector crate
    src/lib.rs                      # pub const CONNECTOR_ID = "acme"
    src/connector/                  # Connector trait impl, sync, send, …
  void-cli/src/
    connectors/
      mod.rs                        # ConnectorPlugin + inventory::collect!
      acme.rs                       # NEW — one inventory::submit! descriptor
    commands/setup/acme.rs          # Interactive setup (optional)
    commands/acme.rs                # Connector-specific subcommands (optional)
```

`void-core` stores connector identity as a string (`ConnectorType`) and connection settings as a generic `toml::Table`. It does **not** list which connectors exist — the CLI registry is the source of truth.

---

## Step 1 — Connector crate

Create `crates/void-acme/` following existing connectors (`void-hackernews` is a good minimal template; `void-slack` for full messaging).

In `lib.rs`:

```rust
pub const CONNECTOR_ID: &str = "acme";
```

Return `ConnectorType::from_static(CONNECTOR_ID)` from `connector_type()`.

Add the crate to the workspace `Cargo.toml` and `void-cli` dependencies.

---

## Step 2 — Plugin descriptor

Add `crates/void-cli/src/connectors/acme.rs`:

```rust
inventory::submit! {
    ConnectorPlugin {
        id: void_acme::CONNECTOR_ID,
        aliases: &["acme", "am"],
        menu_label: "Acme",
        badge: "AC",
        default_poll_interval_secs: Some(3600),  // or None
        reply_id_style: ReplyIdStyle::MsgOnly, // or ConvMsg
        supports_scheduling: false,
        uses_daemon_rpc: false,
        prompt_token_reauth: false,
        session_files: session_files,
        build: build,
        setup: setup,
        parse_settings,
        show_config,
    }
}
```

Wire `build` to your connector's `::new()`, `setup` to `commands::setup::acme::setup_acme`, and implement:

- `parse_settings` — validate required keys at config load
- `show_config` — redacted display for `void setup` (use `redact_token` for secrets)
- `session_files` — paths to rename/delete on connection rename or `--clear-connector`

Register the module in `connectors/mod.rs`:

```rust
mod acme;
```

---

## Step 3 — Setup wizard (if needed)

Add `commands/setup/acme.rs` with `setup_acme(cfg, store_path, add_only)`. Build settings with `empty_settings()` and `settings_set_*` helpers from `void_core::config`.

Use `ConnectorType::from_static(void_acme::CONNECTOR_ID)` when creating `ConnectionConfig`.

The add-connection menu and full wizard iterate the registry automatically — no menu edits required.

---

## Step 4 — Connector-specific CLI (optional)

If the connector needs subcommands (`void acme …`), add `commands/acme.rs` and register in `cli.rs`. Most read-only connectors skip this.

---

## Step 5 — Tests

- Crate tests: use `CONNECTOR_ID` and `ConnectorType::from_static(CONNECTOR_ID)`.
- Registry tests in `connectors/mod.rs` cover unique ids/badges and alias resolution.
- Add `parse_settings` round-trip coverage in the descriptor module if validation is non-trivial.

---

## What you no longer edit

These central files are registry-driven — do **not** add match arms or enum variants:

- `void-core` `ConnectorType` enum (removed — string newtype)
- `ConnectionSettings` enum (removed — `toml::Table`)
- `connector_factory.rs` match (uses `plugin.build`)
- `output.rs` alias table (uses `by_alias_or_id`)
- `reply.rs` / `send.rs` scheduling and daemon guards (capability flags)
- `sync/engine.rs` session cleanup (uses `session_files`)
- `config_ui.rs` settings display (uses `show_config`)

---

## Sync poll intervals

Optional per-connector default: set `default_poll_interval_secs` on the plugin. Users override with `{id}_poll_interval_secs` in `[sync]` (backward compatible with existing keys).

---

## Checklist

1. `crates/void-acme/` with `CONNECTOR_ID`
2. Workspace + `void-cli` Cargo entries
3. `connectors/acme.rs` + `mod acme;`
4. `setup/acme.rs` if interactive setup is needed
5. `cargo test --workspace` and `./scripts/check.sh` green
