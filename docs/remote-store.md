# Remote store (SSH)

Run the sync daemon on an always-on machine (home server, VPS) and use `void` from any laptop against the same data. The local machine keeps a thin client profile; the authoritative `config.toml`, credentials, and database live on the server. Everything goes over plain SSH — no extra service to run.

> New to remote mode? [Deployment modes](deployment.md) compares it with the local default and covers migration. This page is the configuration reference.

## Server side

Configure and run sync as usual:

```bash
void setup
void sync --daemon
```

A starter server config lives at [`scripts/server-config.toml.example`](../scripts/server-config.toml.example).

## Client side

Point your local config at the server:

```toml
# Local ~/.config/void/config.toml
[store]
mode = "remote"

[store.remote]
host = "homeserver"
user = "mgaudin"
remote_config_path = "~/.config/void/config.toml"

[store.remote.cache]
path = "~/.cache/void/remote/homeserver"
config_ttl_secs = 300
database_ttl_secs = 30
```

All `[store.remote]` options:

| Field | Default | Description |
|-------|---------|-------------|
| `host` | — | SSH host (can be an alias from `~/.ssh/config`) |
| `user` | — | SSH user (optional) |
| `remote_config_path` | `~/.config/void/config.toml` | Path of the server's config |
| `remote_store_path` | from server config | Override the server's store path |
| `proxy_writes` | `true` | Proxy write commands to the server over SSH |
| `[store.remote.ssh] port` | 22 | SSH port |
| `[store.remote.ssh] identity_file` | — | SSH private key |
| `[store.remote.cache] path` | — | Local cache directory |
| `[store.remote.cache] config_ttl_secs` | 300 | How long the cached remote config stays fresh |
| `[store.remote.cache] database_ttl_secs` | 30 | How long the cached DB snapshot stays fresh |

## How commands behave

- **Read commands** (`inbox`, `search`, `messages`, …) use a cached database snapshot pulled over SSH — instant after the first fetch, refreshed when older than `database_ttl_secs`
- **Write commands** (`send`, `reply`, `archive`, …) are proxied to the server via SSH
- **File attachments** — `--file` on proxied sends/replies/drafts is staged to the remote store over SCP before the command runs; download commands (`gmail attachment`, `whatsapp download`, `telegram download`, `linkedin download`, `drive download --output`) write to a remote temp path and the result is pulled back to your local `--out`/`--output` path

## Inspecting and refreshing

```bash
# SSH connectivity, cache age, remote daemon state
void remote status

# Force-refresh config + DB snapshot
void remote refresh
```

Useful flags:

- `--config <path>` — pick a different local client profile
- `--store <path>` — override the remote store path (in local mode: overrides `store.path`)
