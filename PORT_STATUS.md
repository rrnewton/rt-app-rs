# Port Status

This document tracks the porting relationship between rt-app-rs and the
upstream [rt-app](https://github.com/scheduler-tools/rt-app) project.

## Current Status

**rtapp v0.1.1** is a feature-complete port of upstream rt-app.

## Port History

### 2026-02-21: Initial Port (v0.1.1)

**Upstream commit:** `9e6899a2a8f848cf1456ea414a95a7b8d4632057`
**Upstream date:** 2025-09-17
**Commit depth:** 477 commits
**Commit message:** Merge pull request #142 from glemco/master

#### Feature Completeness

All core features from upstream rt-app are implemented:

- **JSON config format** — identical syntax, same field names and semantics
- **All 19 event types** — run, runtime, sleep, timer, lock/unlock, wait,
  signal, broadcast, sig_and_wait, barrier, suspend, resume, mem, iorun,
  yield, fork
- **Scheduling policies** — SCHED_OTHER, SCHED_FIFO, SCHED_RR, SCHED_DEADLINE
- **CPU calibration** — dual-method convergence matching C behavior
- **CPU affinity** — per-thread pinning with phase > task > default precedence
- **Cgroup/taskgroup** — cgroup v1 cpu controller support
- **Gnuplot output** — per-thread and aggregate plots in EPS format
- **Ftrace integration** — trace_marker with category filtering
- **Log format** — columnar output compatible with existing analysis tools

#### Verification

Fuzz testing comparing random workloads between C rt-app and rtapp shows
96%+ consistency. Remaining divergences are timing edge cases (timeout
boundaries) rather than functional differences.

#### Deviations from Upstream

- **Built-in workgen** — duplicate JSON keys are handled internally; no
  external `workgen` Python script needed
- **Binary name** — `rtapp` instead of `rt-app` to distinguish the Rust port
- **Comment syntax** — supports `//` line comments in addition to `/* */`

---

## Upstream Sync Procedure

When catching up with upstream changes, create a new entry above this section
documenting:

1. The new upstream commit being synced to
2. New features/fixes implemented
3. Fuzz testing results after sync
4. Any new deviations or compatibility notes
