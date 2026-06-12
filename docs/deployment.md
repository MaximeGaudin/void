# Deployment modes: local vs remote

`void` is two things in one binary: a **sync daemon** that mirrors every connected service into a SQLite database, and a **CLI** that reads that database and talks to the service APIs. Where you run the daemon defines the deployment mode.

There are exactly two modes:

| | **Local mode** (default) | **Remote mode** |
|---|---|---|
| Sync daemon runs on | your machine | an always-on server |
| Database lives on | your machine | the server (snapshot cached locally) |
| Credentials live on | your machine | the server only |
| Setup effort | none beyond `void setup` | one server + a small client config |
| Works offline | yes, fully | reads yes (cached), writes no |
| Sync when your laptop is off | **stops** | keeps running |
| Hooks fire when your laptop is off | no | yes |
| Use from several machines | no (one DB per machine) | yes, any laptop with SSH access |

## Local mode (default)

Everything runs on one machine. This is what `void setup` gives you out of the box — nothing else to configure.

```
your machine
┌──────────────────────────────────────────────┐
│  void (CLI) ◄──► SQLite ◄── sync daemon ──►──┼──► WhatsApp, Slack, Gmail, ...
└──────────────────────────────────────────────┘
```

```bash
void setup            # connect your accounts
void sync --daemon    # start the daemon
void inbox            # instant, offline-capable reads
```

**Strengths**

- Zero infrastructure: no server, no Docker, no cloud. All data stays in `~/.local/share/void`.
- Reads are instant and work fully offline.

**Limitation: sync follows your laptop's lid.** The daemon only syncs while the machine is awake. When your laptop is off or asleep:

- The database goes stale — nothing is lost (connectors backfill on the next sync), but `void inbox` reflects the last time the daemon ran.
- Scheduled and `new_message` [hooks](hooks.md) don't fire. A "ping me on Telegram when an important email lands" hook is only as reliable as your uptime.

If that trade-off is fine — you mostly use `void` interactively, on one machine — stay in local mode. If you want sync and hooks running 24/7, move the daemon to a server.

## Remote mode

The daemon, the authoritative config, the credentials, and the database all live on an always-on machine (home server, VPS, Raspberry Pi). Your laptop keeps a thin client profile and talks to the server over plain SSH — no extra service, no open ports beyond SSH, no cloud.

```
server (always on)                                  laptop(s)
┌──────────────────────────────────────┐            ┌─────────────────────────┐
│  sync daemon ──► SQLite              │ ◄── SSH ── │  void (CLI)             │
│  config.toml, credentials, hooks     │            │  cached DB snapshot     │
└───────────────────┬──────────────────┘            └─────────────────────────┘
                    └──► WhatsApp, Slack, Gmail, ...
```

On the server, configure and run `void` exactly as in local mode (`void setup`, `void sync --daemon`). On each laptop, the entire client config is:

```toml
# ~/.config/void/config.toml
[store]
mode = "remote"

[store.remote]
host = "homeserver"        # any host or ~/.ssh/config alias
```

The CLI then routes every command appropriately, transparently:

- **Reads** (`inbox`, `search`, `messages`, `calendar`, …) hit a local snapshot of the server's database, pulled over SSH and refreshed when stale (30 s by default). They stay instant and keep working offline.
- **Writes** (`send`, `reply`, `archive`, …) are proxied to the server over SSH and run there, where the credentials live. Your laptop never holds a token.
- **File transfers** work in both directions: `--file` attachments are staged to the server before sending, and downloads (`gmail attachment`, `whatsapp download`, `drive download`, …) are fetched on the server and pulled back to your local path.
- **Hooks** run on the server, around the clock.

`void remote status` shows SSH connectivity, cache age, and daemon state; `void remote refresh` force-refreshes the snapshot. Setup details, the full `[store.remote]` option reference, and a starter server config: [Remote store](remote-store.md).

## Moving from local to remote

Nothing about the data layout changes between modes — a server is just a machine in local mode that happens to never sleep.

1. Install `void` on the server and run `void setup` there (or copy your existing `~/.config/void/` and `~/.local/share/void/` over).
2. Start the daemon: `void sync --daemon`.
3. On your laptop, replace your config with the remote client profile above. Your local database and credentials are no longer used; remove them when you're confident.

To go back, point your laptop's config at `mode = "local"` again and restart a local daemon.
