---
title: 'Port rt-app C to Rust: master epic'
status: closed
priority: 0
issue_type: epic
created_at: 2026-02-20T20:34:17.762268068+00:00
updated_at: 2026-02-20T21:25:44.078014768+00:00
closed_at: 2026-02-20T21:25:44.078014668+00:00
---

# Description

Complete port of the rt-app real-time workload simulator from C to Rust. rt-app is ~4500 lines of C across 7 compilation units. It creates periodic pthreads to simulate real-time scheduling loads, driven by JSON config files. Subsystems: types/data model, utilities (time/logging/ftrace), JSON config parsing, thread engine (calibration, event dispatch, scheduling), cgroup/taskgroup management, SCHED_DEADLINE syscall wrappers, CLI argument parsing, gnuplot generation. The Rust port must be a drop-in replacement, passing all original example configs and producing compatible log output.
