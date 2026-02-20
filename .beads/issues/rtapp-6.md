---
title: Port thread engine and event dispatch (rt-app.c core)
status: open
priority: 1
issue_type: task
depends_on:
  rtapp-3: blocks
  rtapp-8: blocks
  rtapp-1: parent-child
  rtapp-5: blocks
  rtapp-11: blocks
  rtapp-10: blocks
  rtapp-2: blocks
  rtapp-4: blocks
created_at: 2026-02-20T20:34:54.441190986+00:00
updated_at: 2026-02-20T20:36:21.713997763+00:00
---

# Description

Port the core of rt-app.c (1664 lines). Includes: waste_cpu_cycles() CPU burn loop using ldexp, calibrate_cpu_cycles() dual-method calibration, loadwait() duration-to-loop conversion, ioload() device write, memload() memset, run_event() dispatcher for all 19 event types, run() phase iterator, thread_body() main thread function with timing collection, create_thread() thread spawner, signal handling (SIGQUIT/SIGTERM/SIGHUP/SIGINT), shutdown sequence, thread synchronization (barrier for startup, joining_mutex, fork_mutex). Thread scheduling: set_thread_param dispatching to CFS/RT/DEADLINE/uclamp paths via pthread_setschedparam and sched_setattr.
