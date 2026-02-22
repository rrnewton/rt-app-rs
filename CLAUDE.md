
This Project
=======================================================

This project is a simple pure Rust port of https://github.com/scheduler-tools/rt-app.
We will make some additions after the port is complete.

Prefer safe Rust as much as possible, and justify carefully in a
SAFETY comment any use of unsafe, why it is required, and why it
preservers safety.

Use minibeads (described below) issues to track the status of the port.

Idiomatic Rust and Library Choices
========================================

Use idiomatic, popular, well-maintained Rust crates rather than hand-rolling
functionality. The port should feel like natural Rust, not a transliteration of
C. Key library choices:

- **serde + serde_json** for JSON config parsing (derive-based deserialization,
  not manual token walking as the C code does with json-c)
- **clap** (derive API) for CLI argument parsing
- **nix** for POSIX/Linux syscall wrappers (sched, signal, mman, pthread, etc.)
- **libc** for low-level types and constants not covered by nix
- **log + env_logger** for structured logging
- **thiserror** or **anyhow** for error handling (no raw exit codes or fprintf+exit)

When a well-known crate exists for a task, use it. Do not reimplement standard
functionality (time arithmetic, bitflags, etc.) when a crate like `bitflags`
or std's `Duration` handles it idiomatically.

Development Guidelines
=======================================================

This document contains the development guidelines and instructions for the project. This guide OVERRIDES any default behaviors and MUST be followed exactly.

If you become stuck with an issue you cannot debug, you can file an issue for it and leave it to work on other topics. Of course, the tests should be always passing before each commit and achieve reasonably good code coverage as described below.

Coding conventions
========================================

You HATE duplicated code. You follow DRY religiously and seek clean abstractions where functions are short, and complexity is factored into helpers, traits, and centralized infrastructure that is shared as much as possible. You hate duplication so much that you would rather centralize repetitive code EVEN if it means the interface to the shared functionality becomes fairly complex (e.g. the shared logic uses callbacks with complex types for the pieces that vary between use cases).

You dislike function definitions that are longer than necesasry, and in particular those over 150 lines long. These would be better factored into a clear spine of calls to helper functions/macros that are factored out.

You also dislike long files. Whenever a file grows longer than 1500 lines you propose ideas for breaking it into separate modules.

PREFER STRONG TYPES. Do not use "u32" or "String" where you can have a more specific type or at least a type alias. "String" makes it very unclear which values are legal. We want explicit Enums to lock down the possibilities for our state, and we want separate types for numerical IDs and distinct, non-overlapping uses of basic integers.

Delete trailing spaces. Don't leave empty lines that consist only of whitespace. (Double newline is fine.)

Adhere to high-performance Rust patterns (unboxing, minimizing allocation, etc). In particular, adhere to the below programming patterns / avoid anti-patterns, which generally fall under the principle of "zero copy":

- Avoid the pattern of returning allocated collections when they are not necessary (e.g. return an iterator).
- Avoid clone: instead take a temporary reference to the object and manage lifetimes appropriately.
- Avoid collect: instead take an iterator with references to the original collection without copying.

Documentation and Analysis
========================================

When creating analysis documents, specifications, or other AI-generated documentation, place them in the `ai_docs/` directory. This keeps the top-level clean and makes it clear which documents are AI-generated analysis (and may become outdated) versus core project documentation.

Workflow: Commits and Version Control
================================================================================

Clean Start: Before beginning work on a task
--------------------------------------------

Make sure we start in a clean state. Check that we have no uncommitted changes in our working copy. Perform `git pull origin <BRANCH>` to make sure we are starting with the latest version on our branch. Check that `./validate.sh` passes in our starting state.

Pre-Commit: checks before committing to git
--------------------------------------------

Run `./validate.sh` and ensure that it passes or fix any problems before committing.

Also include a `Test Results Summary` section in every commit message that summarizes how many tests passed of what kind.

If you validate some changes with a new manual or temporary test, that test should be added to either the unit tests or integration tests and it should be called consistently from `./validate.sh`.

NEVER add binary files or large serialized artifacts to version control without explicit permission. Always carefully review what you are adding with `git add`, and update `.gitignore` as needed.

Testing Required Before Push
----------------------------------------

**NEVER push code that has only been compiled and not actually
tested. You can run tests yourself! If so go ahead and push.

If you cannot test due to sandboxing, permissions, or missing
dependencies that you have trouble installing:

1. **Do NOT push the code**
2. Ask the user to run the manual test command
3. Wait for confirmation that it works before pushing

Amending commits
----------------------------------------

It is fine to amend the most recent commit (git commit --amend) as long as it has NOT been pushed to the remote yet. If the commit has already been pushed, create a new commit instead.

Branches and pushing
----------------------------------------

The `main` branch is protected. Never push directly to main. Only push to feature branches after validation. Don't force push unless you're asked to or ask permission.

Issue Tracking
========================================

We use minibeads (`mb`) for local issue tracking. Run `mb quickstart` to learn
the commands. Use `mb ready` to find the next issue to work on, and update issue
status as you work (`mb update prefix-N --status in_progress`, `mb close prefix-N`).

File issues for bugs, TODOs, and feature work rather than leaving stale TODO
comments in code. Reference issue IDs (e.g. prefix-1) in commit messages when
closing issues.

Dependencies and Missing Software
========================================

Never work around a missing dependency with a compromised fallback. If software
is needed, install it — build from source, use the package manager, or ask the
user for help. The `~/bin/` directory is on `$PATH` for locally-built tools.
This would include the **mb** (minibeads): local issue tracker (cargo install minibeads).

Every TODO in source code MUST reference an issue: `TODO(prefix-XXXXX)`. Do not
leave TODOs without a tracking issue — file one first, then add the TODO.

Multi-Agent Team Mode
========================================

When you find yourself in a parent directory containing multiple
checkouts of the same repository (e.g., `repo1` `repo2` `repo3`
`repo4`), you are in **multi-agent team mode**. This enables parallel
development with multiple sub-agents working simultaneously.

Each checkout is a complete working copy. Sub-agents can work in different
checkouts simultaneously without conflicts.

Parallel Development Philosophy
----------------------------------------

1. **Commit and push early and often** to the same branch. Don't let work
   accumulate locally — push as soon as `./validate.sh` passes.

2. **Resolve conflicts early**. When multiple agents push to the same branch,
   the lead agent should pull, rebase, resolve conflicts, and push promptly.

3. **Keep tests passing**. Every commit must pass `./validate.sh` locally.
   Check CI status with `with-proxy gh run list` and fix failures immediately.

4. **Stay on the same branch** unless explicitly asked to create a feature
   branch for speculative work.

Spawning Sub-Agents
----------------------------------------

When spawning a sub-agent to work on a task:

1. **Always specify which checkout directory** the agent should use:
   ```
   You are working on the XYZ project in PATH
   ```

2. **Assign different checkouts** to parallel tasks to avoid conflicts.

3. **Include clear instructions** about pulling latest, running validation,
   committing, and pushing.

After Sub-Agent Completion
----------------------------------------

When sub-agents complete their work:

1. **Check for conflicts**: If multiple agents pushed, rebase and resolve.

2. **Sync all checkouts**: Pull the merged state to all working copies leaving them at the same tip commit.

3. **Close issues**: Use `mb close prefix-XXXXX` for completed work.

4. **Verify CI**: Check that the merged changes pass CI.

Upstream Sync Workflow
========================================

This project maintains feature parity with upstream
[rt-app](https://github.com/scheduler-tools/rt-app). The upstream C code is
included as a git submodule at `rt-app-orig/` for comparison testing.

When to Sync
----------------------------------------

Periodically check for new upstream commits and sync to maintain drop-in
replacement capability. Run this workflow when:
- Preparing a new release
- A user reports a missing feature from upstream
- Upstream has significant new commits

Sync Procedure
----------------------------------------

1. **Update the submodule to latest upstream master:**
   ```bash
   cd rt-app-orig
   git fetch origin
   git checkout origin/master
   cd ..
   git add rt-app-orig
   ```

2. **Review new upstream commits:**
   ```bash
   cd rt-app-orig
   git log --oneline <old-commit>..HEAD
   ```
   Read the commit messages and changed files to understand new features/fixes.

3. **Rebuild upstream C rt-app:**
   ```bash
   cd bug_finding
   ./target/release/rt-app-fuzzer --build --iterations 0
   ```

4. **Implement new features in Rust:**
   For each new feature or fix in upstream:
   - Read the C implementation
   - Implement equivalent functionality in idiomatic Rust
   - Add unit tests covering the new functionality
   - Update `doc/tutorial.md` if user-facing

5. **Update the fuzzer if needed:**
   If upstream adds new JSON config fields or event types, update
   `bug_finding/src/generator.rs` to generate workloads using them.

6. **Run fuzz testing:**
   ```bash
   cd bug_finding
   ./target/release/rt-app-fuzzer --iterations 50 --keep-failures
   ```
   Goal: 95%+ consistency. Investigate any divergences.

7. **Update PORT_STATUS.md:**
   Add a new entry documenting:
   - The new upstream commit hash and date
   - Features/fixes implemented
   - Fuzz testing results
   - Any new deviations or compatibility notes

8. **Run full validation and commit:**
   ```bash
   ./validate.sh
   git add -A
   git commit -m "Sync with upstream rt-app <commit>"
   ```

Submodule Management
----------------------------------------

The `rt-app-orig` submodule points to the upstream rt-app repository. Key
commands:

```bash
# Initialize submodule (first time clone)
git submodule update --init

# Update to latest upstream
cd rt-app-orig && git fetch origin && git checkout origin/master && cd ..

# Check current upstream commit
git submodule status
```

The fuzzer (`bug_finding/`) uses this submodule to build and run the C rt-app
for comparison testing. Always keep the submodule at a known-good commit that
we have verified compatibility with.
