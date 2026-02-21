# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Built-in workgen preprocessor** — duplicate JSON keys (e.g., repeated
  `"run"` entries) are automatically renamed with numeric suffixes (`"run"`,
  `"run1"`, `"run2"`), eliminating the need for the external Python `workgen`
  script from the C original.
- **`doc/tutorial.md`** — comprehensive markdown rewrite of the original C
  tutorial (`doc/tutorial.txt`), with proper headings, fenced code blocks,
  tables, and updated documentation for the built-in JSON preprocessor.
- **Full runtime pipeline** — complete end-to-end execution: config parsing →
  type conversion → calibration → engine state construction → signal handling →
  thread spawning → shutdown. The binary now actually runs workloads.
- **JSON fuzzer** (`bug_finding/`) — standalone stress-testing tool that
  generates random valid JSON workloads and compares C rt-app vs rt-app-rs
  behavior (exit codes, timing, output). Includes the original C rt-app as
  a git submodule for comparison testing.

## [0.1.0] - 2026-02-20

Faithful port of the original C [rt-app](https://github.com/scheduler-tools/rt-app)
to idiomatic Rust. This release aims for exact feature parity with the C original
— same JSON config format, same output formats, same runtime behavior.

### Ported modules

- **JSON config parser** — serde_json (derive-based) replacement for json-c manual
  parsing. Supports all 19 event types, single-phase shorthand and multi-phase
  syntax, auto-creation of implicit resources, C-style comment stripping, and
  trailing comma tolerance.
- **Thread engine** — thread lifecycle, phase/loop execution, event dispatch for
  all 19 event types (run, runtime, sleep, timer, lock/unlock, wait, signal,
  broadcast, sig_and_wait, barrier, suspend, resume, mem, iorun, yield, fork).
- **CPU calibration** — dual-method calibration (idle+burst and continuous burst),
  `waste_cpu_cycles` burn loop, `loadwait` with 1-second burst splitting.
- **Scheduling** — CFS, RT (FIFO/RR), SCHED_DEADLINE, and uclamp parameter
  setup via `sched_setattr`/`sched_getattr` syscall wrappers.
- **CPU affinity** — per-thread CPU pinning with phase > task > default
  precedence. NUMA support behind `numa` feature flag.
- **Cgroup/taskgroup management** — cgroup v1 cpu controller: mount point
  discovery, nested directory creation/cleanup, thread attachment.
- **Gnuplot generation** — per-thread and aggregate period/run overlay plots
  in PostScript EPS format.
- **Ftrace integration** — trace_marker writing with category bitmask filtering
  (main, task, loop, event, stats, attrs).
- **CLI** — clap-based argument parsing matching the original interface.
- **Logging** — columnar timing output matching the C format for compatibility
  with existing analysis scripts.

### Dependencies

- serde + serde_json (preserve_order) for config parsing
- clap (derive) for CLI
- nix for POSIX/Linux syscall wrappers
- libc for low-level constants
- bitflags for SchedFlags and FtraceLevel
- log + env_logger for structured logging
- thiserror for typed errors

[0.1.0]: https://github.com/rrnewton/rt-app-rs/releases/tag/v0.1.0
