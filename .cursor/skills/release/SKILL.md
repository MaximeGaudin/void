---
name: release
description: Prepare, tag, and publish a new release — bump version, update CHANGELOG.md, build/install via project script, commit, create annotated tag, and optionally push + trigger CI release workflow when requested. Use when the user asks to release, tag, cut a release, bump version, update the changelog, or publish.
---

# Release

Step-by-step procedure for cutting a new Void CLI release.

## 1. Determine the new version

Read the current version:

```
grep '^version' Cargo.toml   # workspace version
git tag --list --sort=-v:refname | head -5
```

Choose the next version following [Semantic Versioning](https://semver.org):

| Change type | Bump |
|---|---|
| Breaking CLI/trait changes | Major (X.0.0) |
| New features, commands, connectors | Minor (0.X.0) |
| Bug fixes, refactors, dependency updates | Patch (0.0.X) |

## 2. Gather changes since last tag

```
git log <LAST_TAG>..HEAD --oneline --reverse
```

Categorize every commit into **Added**, **Changed**, **Fixed**, **Removed** sections per [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## 3. Update CHANGELOG.md

Open `CHANGELOG.md` at the project root. Insert a new section **above** the previous release:

```markdown
## [X.Y.Z] - YYYY-MM-DD

### Added
- ...

### Changed
- ...

### Fixed
- ...
```

Rules:
- Use present tense ("Add X", not "Added X") in commit messages but past tense in the changelog ("Added").
- Group by category, not by crate. Prefix with the crate/scope in bold when relevant (e.g. `**Gmail** — ...`).
- Each bullet should be user-facing and understandable without reading the code.
- Do NOT include merge commits, CI-only changes, or trivial reformatting.

## 4. Bump the workspace version

Edit `Cargo.toml` (root workspace):

```toml
[workspace.package]
version = "X.Y.Z"
```

All crates inherit `version.workspace = true`, so a single edit propagates everywhere.

## 5. Pre-flight checks (mirror CI)

Run every check that CI enforces **before** committing, so failures are caught locally:

```bash
cargo fmt --check        # formatting
cargo clippy -- -D warnings   # lints (warnings = errors, same as CI RUSTFLAGS)
cargo test               # all tests
```

If any step fails, fix the issue and re-run before proceeding.

These three commands match the CI matrix exactly (`ci.yml` runs them on both Ubuntu and Windows). Catching them here avoids a push → fail → amend → force-push cycle.

## 6. Build and install

```
./scripts/build-install.sh
void --version   # confirm new version
```

Windows:

```powershell
.\scripts\build-install.ps1
void --version
```

Important:
- **Always** use project install scripts. Do not use `cp` or manual binary copy.
- The script safely stops the sync daemon and performs an atomic replace.

## 7. Commit and tag

```
git add -A
git commit -m "chore: release vX.Y.Z"
git tag -a X.Y.Z -m "Release X.Y.Z"
```

Use an **annotated** tag (`-a`), not a lightweight tag.

## 8. Verify

```
git log --oneline -1
git tag -l "X.Y.Z" -n1
void --version
```

## 9. Publish (only when explicitly requested)

If the user asks to publish the release, push the commit and tag:

```bash
git push origin HEAD
git push origin X.Y.Z
```

Then trigger the CI release workflow, which builds cross-platform binaries and creates the GitHub release automatically:

```bash
gh workflow run release.yml -f tag=X.Y.Z
```

Monitor the workflow run to confirm it started:

```bash
sleep 5
gh run list --workflow=release.yml --limit=1
```

The CI workflow (`release.yml`) handles:
- Building binaries for macOS (arm64/amd64), Linux (arm64/amd64), and Windows (amd64)
- Extracting the changelog section for the release notes
- Creating (or updating) the GitHub release with all artifacts attached

## Checklist

Copy and track:

```
Release X.Y.Z:
- [ ] Determine version number
- [ ] Gather commits since last tag
- [ ] Update CHANGELOG.md
- [ ] Bump version in Cargo.toml
- [ ] Pre-flight: cargo fmt --check
- [ ] Pre-flight: cargo clippy -- -D warnings
- [ ] Pre-flight: cargo test
- [ ] Build and install locally (./scripts/build-install.sh)
- [ ] void --version shows new version
- [ ] git commit
- [ ] git tag -a X.Y.Z
- [ ] Verify tag
- [ ] (If requested) Push commit and tag
- [ ] (If requested) Trigger CI release workflow
```

## Notes

- Use `./scripts/build-install.sh` (or `build-install.ps1` on Windows) instead of manual copy.
- Do NOT push commits/tags or trigger the CI release unless the user explicitly asks.
- The GitHub release and cross-platform binaries are created by CI (`release.yml`), not locally. Always use `gh workflow run` to trigger it.
