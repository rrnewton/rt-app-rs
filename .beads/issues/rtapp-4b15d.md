---
title: Port thread engine and event dispatch (rt-app.c core)
status: closed
priority: 1
issue_type: task
depends_on:
  rtapp-7c9cd: blocks
  rtapp-52f69: blocks
  rtapp-450e3: blocks
  rtapp-1ec29: blocks
  rtapp-f81ac: blocks
  rtapp-075d2: parent-child
  rtapp-49a52: blocks
  rtapp-821aa: blocks
created_at: 2026-02-20T20:34:54.441190986+00:00
updated_at: 2026-02-20T21:09:42.648770604+00:00
closed_at: 2026-02-20T21:09:42.648770504+00:00
---

# Description

Port the core of rt-app.c (1664 lines). Includes: waste_cpu_cycles() CPU burn loop using ldexp, calibrate_cpu_cycles() dual-method calibration, loadwait() duration-to-loop conversion, ioload() device write, memload() memset, run_event() dispatcher for all 19 event types, run() phase iterator, thread_body() main thread function with timing collection, create_thread() thread spawner, signal handling (SIGQUIT/SIGTERM/SIGHUP/SIGINT), shutdown sequence, thread synchronization (barrier for startup, joining_mutex, fork_mutex). Thread scheduling: set_thread_param dispatching to CFS/RT/DEADLINE/uclamp paths via pthread_setschedparam and sched_setattr.
