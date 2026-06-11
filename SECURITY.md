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
