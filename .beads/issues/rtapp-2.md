---
title: 'Project scaffolding: Cargo.toml, module structure, CI'
status: open
priority: 0
issue_type: task
depends_on:
  rtapp-1: parent-child
created_at: 2026-02-20T20:34:23.829720626+00:00
updated_at: 2026-02-20T20:35:43.511928890+00:00
---

# Description

Set up Cargo.toml with dependencies (serde, serde_json, clap, nix, libc, log, env_logger). Create module structure: src/main.rs, src/types.rs, src/utils.rs, src/config.rs, src/args.rs, src/engine.rs, src/taskgroups.rs, src/syscalls.rs. Set up GitHub Actions CI with cargo build, cargo test, cargo clippy, cargo fmt --check. Add validate.sh script.
