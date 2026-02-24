---
name: sync-upstream
description: Sync rt-app-rs with upstream rt-app changes, implement new features, and validate compatibility
---

# Upstream Sync Skill

You are tasked with syncing rt-app-rs with the upstream C rt-app project. This is
a careful, methodical process that ensures our Rust port remains a drop-in
replacement for the original.

## Context

- **rt-app-rs**: Pure Rust port of rt-app (this repo)
- **rt-app-orig/**: Git submodule pointing to upstream C rt-app
- **bug_finding/**: Fuzzer that compares C and Rust behavior
- **PORT_STATUS.md**: Documents sync history and compatibility status

## Prerequisites

Before starting, verify:
1. You are on a clean `port-upstream-YYYYMMDD` branch
2. The rt-app-orig submodule has been updated to the new upstream commit
3. The working directory is the repository root

## Sync Procedure

### Phase 1: Analyze Upstream Changes

1. **Identify the commit range to analyze:**
   ```bash
   cd rt-app-orig
   git log --oneline HEAD@{1}..HEAD  # New commits since last sync
   ```

2. **For each new commit, examine:**
   - What files changed: `git show --stat <commit>`
   - The actual changes: `git show <commit>`
   - Focus on: `src/*.c`, `src/*.h`, `doc/`, and any JSON examples

3. **Categorize changes into:**
   - **New features**: New config fields, event types, CLI options
   - **Bug fixes**: Behavioral corrections we need to match
   - **Refactoring**: Internal changes that don't affect behavior
   - **Documentation**: Updates we should mirror

### Phase 2: Implement Changes in Rust

For each new feature or bug fix:

1. **Read the C implementation carefully.** Understand:
   - What config fields are added (check `rt-app_parse_config.c`)
   - What runtime behavior changes (check `rt-app.c`, `rt-app_utils.c`)
   - What new event types exist (check event handling code)

2. **Implement in idiomatic Rust:**
   - Config parsing: Update `src/config/` modules
   - Type definitions: Update `src/types.rs`
   - Runtime behavior: Update `src/engine/` modules
   - CLI options: Update `src/args.rs`

3. **Add tests for new functionality:**
   - Unit tests in the relevant module
   - Integration tests in `tests/integration_tests.rs`
   - Example configs in `tests/fixtures/` if appropriate

4. **Update documentation:**
   - `doc/tutorial.md` for user-facing changes
   - `doc/examples/template.json` for new config options

### Phase 3: Update the Fuzzer

If upstream added new JSON config fields or event types:

1. **Update `bug_finding/src/generator.rs`:**
   - Add new fields to the random config generator
   - Ensure valid value ranges match C expectations
   - Add new event types if applicable

2. **Rebuild the fuzzer:**
   ```bash
   cd bug_finding
   cargo build --release
   ```

### Phase 4: Validate with Fuzzer

1. **Build the C rt-app from submodule:**
   ```bash
   cd bug_finding
   ./target/release/rt-app-fuzzer --build --iterations 0
   ```

2. **Run comprehensive fuzz testing:**
   ```bash
   ./target/release/rt-app-fuzzer --iterations 100 --keep-failures
   ```

3. **Analyze results:**
   - **95%+ consistent**: Acceptable, investigate remaining divergences
   - **Below 95%**: Significant issues, must fix before proceeding

4. **For any divergences (`failures/*.json`):**
   - Read the failing config
   - Run manually through both implementations with verbose output
   - Identify root cause: is it our bug or expected difference?
   - Fix bugs or document expected deviations

### Phase 5: Run Full Validation

```bash
./validate.sh
```

All tests must pass before proceeding.

### Phase 6: Update Documentation

1. **Update PORT_STATUS.md** with a new entry:
   ```markdown
   ### YYYY-MM-DD: Sync with upstream <commit-hash>

   **Upstream commit:** `<full-hash>`
   **Upstream date:** <date>
   **Commit depth:** <N> commits

   #### Changes Implemented

   - Feature X: <description>
   - Bug fix Y: <description>

   #### Fuzz Testing Results

   - Iterations: 100
   - Consistent: XX/100 (XX%)
   - Divergences: <description of any>

   #### Notes

   <Any compatibility notes or deviations>
   ```

2. **Commit all changes** with descriptive messages referencing upstream commits.

### Phase 7: Final Decision

**If all tests pass and fuzzer shows 95%+ consistency:**
- Report SUCCESS
- The CI workflow will merge to main

**If there are unresolved issues:**
- Commit current progress with detailed explanation
- Report FAILURE with:
  - What was accomplished
  - What issues remain
  - Suggested next steps for manual review

## Error Handling

If you encounter errors at any phase:

1. **Build failures**: Fix compilation errors, check for API changes in dependencies
2. **Test failures**: Debug and fix; don't skip tests
3. **Fuzzer divergences**: Prioritize fixing actual bugs over documenting deviations
4. **Unclear upstream changes**: Document uncertainty and flag for human review

## Final Commit

**CRITICAL**: Your final commit message MUST include the status for CI detection.

After completing all phases, make a final commit with this format:

```bash
git add -A
git commit -m "$(cat <<'EOF'
Sync with upstream rt-app <short-hash>

Status: SUCCESS

<or>

Status: FAILURE
Reason: <brief explanation>

## Summary
- Upstream commits: X
- Features implemented: Y
- Fuzz results: XX/100 consistent
EOF
)"
```

The CI workflow checks for "Status: SUCCESS" to determine whether to auto-merge.

## Output Format

At the end of your work, also print a summary to stdout:

```
## Sync Summary

**Status**: SUCCESS | FAILURE | PARTIAL

**Upstream commits processed**: X
**New features implemented**:
- Feature 1
- Feature 2

**Bug fixes applied**:
- Fix 1

**Fuzzer results**: XX/100 consistent (XX%)

**Issues encountered**:
- Issue 1 (resolved/unresolved)

**Files modified**:
- path/to/file1.rs
- path/to/file2.rs

**Next steps** (if FAILURE/PARTIAL):
- Step 1
- Step 2
```

## Important Guidelines

- **Never skip tests** — all 362+ tests must pass
- **Prefer fixing over documenting** divergences when possible
- **Commit incrementally** — don't accumulate too many changes in one commit
- **Be conservative** — when in doubt, flag for human review
- **Match C behavior exactly** — we are a drop-in replacement, not an improvement
