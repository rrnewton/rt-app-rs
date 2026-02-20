---
title: Port SCHED_DEADLINE syscall wrappers (libdl/)
status: open
priority: 2
issue_type: task
depends_on:
  rtapp-1ec29: blocks
  rtapp-4b15d: blocks
  rtapp-450e3: blocks
  rtapp-075d2: parent-child
created_at: 2026-02-20T20:35:13.127784465+00:00
updated_at: 2026-02-20T20:39:02.681537923+00:00
---

# Description

Port libdl/dl_syscalls.c (17 lines) and dl_syscalls.h (133 lines). Provides sched_setattr/sched_getattr syscall wrappers. Define sched_attr struct, SCHED_FLAG_* constants (RESET_ON_FORK, RECLAIM, DL_OVERRUN, KEEP_POLICY, KEEP_PARAMS, UTIL_CLAMP_MIN/MAX). Architecture-specific syscall numbers for x86_64, i386, ARM, AArch64. In Rust, use libc::syscall() or nix crate. Consider whether the nix crate already provides these.
