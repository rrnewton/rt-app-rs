---
title: Port JSON config parser (rt-app_parse_config.c)
status: open
priority: 1
issue_type: task
depends_on:
  rtapp-3: blocks
  rtapp-4: blocks
  rtapp-2: blocks
  rtapp-1: parent-child
created_at: 2026-02-20T20:34:46.762365088+00:00
updated_at: 2026-02-20T20:35:57.180707193+00:00
---

# Description

Port rt-app_parse_config.c (1385 lines) — the largest source file. Replace json-c with serde_json. Parse global config (duration, calibration, default_policy, log settings, ftrace, lock_pages, pi_enabled, io_device, mem_buffer_size, cumulative_slack), resources (mutexes, timers, condvars, barriers, mem buffers, io devices), tasks (per-task cpuset, numa, sched params, phases with events). Support all 19 event types: run, runtime, sleep, timer, timer_unique, lock, unlock, wait, signal, broadcast, sig_and_wait, barrier, suspend, resume, mem, iorun, yield, fork. Handle auto-creation of implicit resources. Support both single-phase shorthand and multi-phase array syntax.
