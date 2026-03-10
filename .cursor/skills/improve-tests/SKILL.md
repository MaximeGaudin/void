---
name: improve-tests
description: Fix broken tests, audit existing tests for validity and coverage, identify missing tests across all crates, add them, and ensure the full suite passes. Use when the user asks to improve tests, add missing tests, fix failing tests, increase test coverage, or audit test quality.
---

# Improve Tests

Systematic workflow for fixing, auditing, and expanding the test suite in this Rust workspace.

## Prerequisites

Before starting, build a mental model of the codebase:

1. Read the workspace `Cargo.toml` to identify all crates
2. For each crate, read `src/lib.rs` (or `src/main.rs`) to understand the public API surface
3. Identify all existing test modules by searching for `#[cfg(test)]`

## Workflow

Copy this checklist and track progress:

```
Test Improvement Progress:
- [ ] Phase 1: Fix broken tests
- [ ] Phase 2: Audit existing tests
- [ ] Phase 3: Identify missing tests
- [ ] Phase 4: Add missing tests
- [ ] Phase 5: Final verification
```

---

### Phase 1: Fix Broken Tests

1. Run the full suite:

```bash
cargo test 2>&1
```

2. If any tests fail:
   - Read the failing test and the code it exercises
   - Determine root cause: is the **test** wrong or the **code** wrong?
   - If the code has a bug, fix the code
   - If the test expectation is stale, update the test
   - Re-run `cargo test` after each fix until green

3. Also run clippy on test code:

```bash
cargo clippy --tests -- -D warnings
```

4. Commit fixes:

```bash
git add -A && git commit -m "fix: repair broken tests (improve-tests phase 1)"
```

---

### Phase 2: Audit Existing Tests

For every `#[cfg(test)] mod tests` block, verify:

| Check | Action if failing |
|-------|-------------------|
| **Correctness** – Does the test actually assert the right behavior? | Fix assertion logic |
| **Relevance** – Does the tested code still exist and behave this way? | Update or remove stale tests |
| **Isolation** – Does the test depend on external state (network, filesystem)? | Use in-memory alternatives or temp dirs |
| **Naming** – Does the test name describe the scenario? | Rename to `<function>_<scenario>_<expected>` pattern |
| **Edge cases** – Does it only test the happy path? | Note gaps for Phase 3 |

If any tests were updated, removed, or renamed, commit:

```bash
git add -A && git commit -m "test: audit and clean up existing tests (improve-tests phase 2)"
```

---

### Phase 3: Identify Missing Tests

Systematically go through **every public and internal function** in each crate. For each function, ask:

- Is there at least one test covering the happy path?
- Are error/edge cases covered?
- Are boundary conditions tested?

#### What to focus on

**High priority** (pure logic, easy to test):
- Parsing functions, data transformations, formatters
- Config loading/saving, serialization/deserialization
- Database CRUD operations (use `Database::open_in_memory()`)
- Helper/utility functions

**Medium priority** (may need mocking or test infrastructure):
- Trait implementations with complex logic
- Functions with conditional branches

**Lower priority** (may require HTTP mocking infrastructure):
- API client methods that make network calls
- End-to-end command execution

Produce a concrete list of missing tests grouped by crate and module before writing any code.

---

### Phase 4: Add Missing Tests

#### Conventions

Follow the existing patterns in this workspace:

- Tests live **inline** in `#[cfg(test)] mod tests` blocks, co-located with the code
- Use only the **standard library** test framework (`#[test]`, `assert!`, `assert_eq!`)
- No external test dependencies unless absolutely necessary (if adding one, update the crate's `Cargo.toml` under `[dev-dependencies]`)
- For database tests, use `Database::open_in_memory()`
- For file I/O tests, use `std::env::temp_dir()` with a `uuid::Uuid::new_v4()` subdirectory, and clean up after
- All tests are currently **synchronous** — only introduce `#[tokio::test]` if testing async code that cannot be tested synchronously

#### Writing tests

- One test per behavior/scenario
- Name pattern: `<function_under_test>_<scenario>` (e.g., `parse_ts_valid_float`, `expand_tilde_no_home`)
- Keep tests small and focused — prefer many small tests over a few large ones
- Test both success and error paths
- Use descriptive assertion messages when the failure reason would be ambiguous

#### Adding tests to a file that already has a test module

Append new `#[test]` functions inside the existing `mod tests` block.

#### Adding tests to a file without a test module

Add at the bottom of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn my_test() {
        // ...
    }
}
```

#### Commit

After adding tests and verifying they pass (`cargo test`), commit:

```bash
git add -A && git commit -m "test: add missing tests (improve-tests phase 4)"
```

---

### Phase 5: Final Verification

1. Run the complete suite:

```bash
cargo test 2>&1
```

2. Ensure **zero failures**.

3. Run clippy on tests:

```bash
cargo clippy --tests -- -D warnings
```

4. If anything fails, fix it and re-run. Repeat until fully green.

5. If any final fixes were needed, commit:

```bash
git add -A && git commit -m "test: final verification fixes (improve-tests phase 5)"
```

6. Summarize what was done:
   - Tests fixed
   - Tests updated or removed
   - Tests added (list by crate/module)
   - Any code bugs found and fixed
