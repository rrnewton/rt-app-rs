
This Project
=======================================================

This project is a simple pure Rust port of https://github.com/scheduler-tools/rt-app.
We will make some additions after the port is complete.

Prefer safe Rust as much as possible, and justify carefully in a
SAFETY comment any use of unsafe, why it is required, and why it
preservers safety.

Use minibeads (described below) issues to track the status of the port.

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

Manual Testing Required Before Push
----------------------------------------

**NEVER push code that has only been validated by automated tests if the feature
requires manual testing.** Features involving external tools (vng, wprof, rt-app),
VM execution, or system-level behavior MUST be manually tested before pushing.

If you cannot test due to sandboxing, permissions, or missing dependencies:
1. **Do NOT push the code**
2. Ask the user to run the manual test command
3. Wait for confirmation that it works before pushing

Examples of features requiring manual testing:
- `--real-run vm` execution
- `--wprof` tracing
- Any new vng flags or kernel parameters
- External tool integrations

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
