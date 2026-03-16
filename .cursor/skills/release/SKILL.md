---
name: release
description: Prepare and tag a new release — bump version, update CHANGELOG.md, build, install, commit, and create an annotated git tag. Use when the user asks to release, tag, cut a release, bump version, or update the changelog.
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

Verify with `cargo check`.

## 5. Build and install

```
cargo install --path crates/void-cli
cp ~/.cargo/bin/void ~/bin/void
xattr -cr ~/bin/void && codesign -s - ~/bin/void   # macOS only
void --version   # confirm new version
```

## 6. Commit and tag

```
git add -A
git commit -m "chore: release vX.Y.Z"
git tag -a X.Y.Z -m "Release X.Y.Z"
```

Use an **annotated** tag (`-a`), not a lightweight tag.

## 7. Verify

```
git log --oneline -1
git tag -l "X.Y.Z" -n1
void --version
```

## Checklist

Copy and track:

```
Release X.Y.Z:
- [ ] Determine version number
- [ ] Gather commits since last tag
- [ ] Update CHANGELOG.md
- [ ] Bump version in Cargo.toml
- [ ] cargo check
- [ ] cargo install --path crates/void-cli
- [ ] Update ~/bin/void (cp + codesign)
- [ ] void --version shows new version
- [ ] git commit
- [ ] git tag -a X.Y.Z
- [ ] Verify tag
```

## Notes

- The `~/bin/void` binary takes precedence over `~/.cargo/bin/void` in PATH. Always copy after `cargo install`.
- On macOS, the copied binary needs `xattr -cr` and `codesign -s -` to avoid quarantine/gatekeeper hangs.
- Do NOT push tags unless the user explicitly asks (`git push origin X.Y.Z`).
