# Security Policy

## Supported versions

Only the [latest release](https://github.com/MaximeGaudin/void/releases/latest) is supported with security fixes.

## Reporting a vulnerability

Please **do not open a public issue** for security vulnerabilities.

Report privately via [GitHub private vulnerability reporting](https://github.com/MaximeGaudin/void/security/advisories/new), or by email to **me@maxime.ly** with `[void security]` in the subject.

You can expect an acknowledgment within a few days. Please include reproduction steps and the affected version (`void --version`).

## Threat model notes

Void handles sensitive material by design. What you should know:

- **Everything is local.** Messages, contacts, and events are stored in a SQLite database under your store directory (default `~/.local/share/void`). Nothing is sent to any third-party service operated by this project — void only talks to the APIs of the services you connect (and Unipile for LinkedIn).
- **Credentials at rest.** OAuth tokens, WhatsApp/Telegram session files, and Slack tokens live unencrypted in the store directory, protected by filesystem permissions only. Anyone with read access to that directory can act as you. Treat backups of it accordingly.
- **Hooks execute an external agent CLI** (e.g. `claude`) with prompts that may contain message content. Review hook prompts and the agent's tool permissions (`extra_args`) before enabling a hook.
- **Remote store mode** transports data over your own SSH connection; no additional service is introduced.

Hardening contributions (e.g. OS keychain integration for tokens) are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md).

## Embedded Google OAuth client

The repository ships a Google OAuth client (`crates/void-gmail/google-credentials.json`, compiled into the binary) so Gmail and Calendar work without a Google Cloud project. Secret scanners flag the `client_secret` in this file — that is expected and is **not** a credential leak:

- It is an OAuth client of type **Desktop / installed application** (`redirect_uri = http://localhost`), which is a **public client**. Per [Google's OAuth model](https://developers.google.com/identity/protocols/oauth2/native-app), the `client_secret` of an installed app is **not treated as confidential** — it is meant to be distributed inside the application. The security boundary is the redirect URI and the per-user consent + tokens, not this value. This is the same approach taken by `rclone`, `gcloud`, and similar tools.
- It grants no access on its own: every user still completes Google's consent flow and receives their own tokens, which stay local on their machine.

What it does *not* protect against: all users authenticate through this shared client, so OAuth requests count against the maintainer's Cloud project quota, and the consent screen shows that project's name. If you prefer full isolation, provide your own client via `credentials_file` in the connection config (see [Configuration](../docs/configuration.md)).

The corresponding secret-scanning alerts are resolved as "won't fix" for these reasons.
