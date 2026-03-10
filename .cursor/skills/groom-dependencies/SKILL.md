---
name: groom-dependencies
description: Audit, update, clean up, and secure Rust (Cargo) dependencies. Use when the user asks to groom, audit, update, clean, or review dependencies, or mentions outdated crates, unused deps, dependency security, or supply-chain safety.
---

# Groom Dependencies

Comprehensive dependency grooming workflow for Rust/Cargo projects. Runs through six phases: outdated check, update, build & test, unused removal, alternative suggestions, and security audit.

## Prerequisites

Ensure the following cargo subcommands are available. Install any that are missing before proceeding:

| Tool | Install | Purpose |
|------|---------|---------|
| `cargo-outdated` | `cargo install cargo-outdated` | List outdated crates |
| `cargo-audit` | `cargo install cargo-audit` | Security vulnerability scan (RustSec DB) |
| `cargo-machete` | `cargo install cargo-machete` | Detect unused dependencies |
| `cargo-deny` | `cargo install cargo-deny` | License & advisory checks (optional, enhances security phase) |

Check which are installed by running:

```bash
command -v cargo-outdated && command -v cargo-audit && command -v cargo-machete
```

Install any missing tools before continuing. Do **not** skip phases because a tool is missing—install it first.

## Workflow

Use the TodoWrite tool to track progress through these phases:

```
- [ ] Phase 1: Check outdated dependencies
- [ ] Phase 2: Update dependencies
- [ ] Phase 3: Build & test
- [ ] Phase 4: Remove unused dependencies
- [ ] Phase 5: Suggest alternatives
- [ ] Phase 6: Security audit
- [ ] Phase 7: Final report
```

---

### Phase 1: Check Outdated Dependencies

Run:

```bash
cargo outdated --root-deps-only
```

Capture the full output. Note every crate where **Latest** differs from **Project** version. Classify updates:

- **Patch** (0.0.x): safe, apply freely
- **Minor** (0.x.0): usually safe, review changelog
- **Major** (x.0.0): breaking, requires careful review

If all dependencies are current, note "all deps up to date" and move on.

### Phase 2: Update Dependencies

**For patch/minor updates:**

```bash
cargo update
```

This respects semver constraints in `Cargo.toml` and only bumps `Cargo.lock`.

**For major updates** that `cargo update` won't cover:

1. Edit `Cargo.toml` to bump the version constraint for each crate.
2. Only bump major versions when:
   - The new major version has been stable for a reasonable time (check crates.io publish date).
   - The migration path is clear (skim the changelog/migration guide).
3. If a major bump looks risky, flag it in the final report rather than applying it.

After making changes, run `cargo check` to catch compile errors early before the full build in Phase 3.

#### Commit

```bash
git add -A && git commit -m "chore(deps): update dependencies (groom phase 2)"
```

### Phase 3: Build & Test

```bash
cargo build 2>&1
cargo test 2>&1
```

If either fails:

1. Read the error output carefully.
2. Fix the issue (version pin, code change, feature flag adjustment).
3. Re-run until both pass.

Do **not** proceed to Phase 4 until build and tests are green.

### Phase 4: Remove Unused Dependencies

```bash
cargo machete
```

`cargo-machete` reports crates listed in `Cargo.toml` that don't appear to be used in source code.

For each flagged crate:

1. **Verify** the finding—some crates are used only via macros, build scripts, or feature re-exports and may be false positives.
2. If truly unused, remove the entry from `Cargo.toml`.
3. Run `cargo build && cargo test` after each removal to confirm nothing breaks.

#### Commit

If any dependencies were removed, commit:

```bash
git add -A && git commit -m "chore(deps): remove unused dependencies (groom phase 4)"
```

### Phase 5: Suggest Alternatives

Review the dependency list in `Cargo.toml` and suggest better alternatives when:

- A crate is **unmaintained** (no commits/releases in 2+ years, archived repo).
- A crate has a **widely-adopted successor** (e.g., `chrono` vs `time`, `reqwest` vs `ureq` for different use cases).
- A crate can be **replaced by std** (e.g., using `std::sync::LazyLock` instead of `once_cell` on recent MSRV).
- A **lighter-weight** alternative exists for the features actually used.

For each suggestion, provide:

- Current crate and why it's worth reconsidering
- Recommended alternative
- Migration effort estimate (trivial / moderate / significant)
- Trade-offs

Do **not** automatically apply these changes—present them in the final report for the user to decide.

### Phase 6: Security Audit

```bash
cargo audit
```

If `cargo-deny` is installed, also run:

```bash
cargo deny check advisories
cargo deny check licenses
```

For each advisory found:

1. Note the advisory ID (RUSTSEC-XXXX-XXXX), severity, and affected crate/version.
2. Check if upgrading resolves it (cross-reference with Phase 2 results).
3. If no fix is available, note the workaround or risk acceptance rationale.

#### Commit

If any security-related changes were made (version bumps, patches), commit:

```bash
git add -A && git commit -m "chore(deps): fix security advisories (groom phase 6)"
```

### Phase 7: Final Report

Present a concise summary to the user using this template:

```markdown
# Dependency Grooming Report

## Updates Applied
| Crate | Old Version | New Version | Update Type |
|-------|-------------|-------------|-------------|
| ... | ... | ... | patch/minor/major |

## Unused Dependencies Removed
- `crate_name` — reason it was unused

## Suggested Alternatives
| Current | Suggested | Effort | Reason |
|---------|-----------|--------|--------|
| ... | ... | trivial/moderate/significant | ... |

## Security
| Advisory | Crate | Severity | Status |
|----------|-------|----------|--------|
| RUSTSEC-... | ... | low/medium/high/critical | fixed / no fix available / risk accepted |

## Summary
- X dependencies updated (Y patch, Z minor, W major)
- X unused dependencies removed
- X alternative suggestions
- X security advisories (X fixed, X remaining)
```

If a section has no items, include it with "None" to confirm it was checked.

## Important Notes

- Always work on a clean git state. If there are uncommitted changes, warn the user before proceeding.
- Commit changes at the end of each phase that modifies files (Phases 2, 4, 6). This keeps progress incremental and easy to revert.
- If the project uses a workspace, run from the workspace root so all member crates are covered.
- When in doubt about a major version bump or crate removal, err on the side of caution and flag it in the report rather than applying it.
