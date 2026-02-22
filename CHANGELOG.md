# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`//` line comments** in JSON config files — in addition to the existing
  `/* ... */` block comments, line comments (`// to end of line`) are now
  supported and stripped during preprocessing.
- **`"calibration": "precise"` mode** — new calibration mode where `run` and
  `runtime` events spin on `clock_gettime(CLOCK_MONOTONIC)` for exact
  wall-clock duration instead of using calibrated busy-loop iterations.
  Useful when you need precise timing without calibration overhead.
- **`--print-template` CLI option** — prints a comprehensive JSON config
  template with syntax highlighting (when output is a TTY), documenting all
  available options and their defaults. Use `rtapp --print-template` for a
  quick reference without consulting documentation.
- **Expanded `doc/examples/template.json`** — now serves as a comprehensive
  quick-reference for all config options with inline comments explaining
  each field.

## [0.1.1] - 2026-02-21

Initial release. Faithful port of the original C
[rt-app](https://github.com/scheduler-tools/rt-app) to idiomatic Rust.
This release aims for exact feature parity with the C original — same JSON
config format, same output formats, same runtime behavior.

### Added

- **Full runtime pipeline** — complete end-to-end execution: config parsing →
  type conversion → calibration → engine state construction → signal handling →
  thread spawning → shutdown. The binary `rtapp` runs real workloads.
- **JSON config parser** — serde_json (derive-based) replacement for json-c
  manual parsing. Supports all 19 event types, single-phase shorthand and
  multi-phase syntax, auto-creation of implicit resources, C-style comment
  stripping, and trailing comma tolerance.
- **Built-in workgen preprocessor** — duplicate JSON keys (e.g., repeated
  `"run"` entries) are automatically renamed with numeric suffixes (`"run"`,
  `"run1"`, `"run2"`), eliminating the need for the external Python `workgen`
  script from the C original.
- **Thread engine** — thread lifecycle, phase/loop execution, event dispatch
  for all 19 event types (run, runtime, sleep, timer, lock/unlock, wait,
  signal, broadcast, sig_and_wait, barrier, suspend, resume, mem, iorun,
  yield, fork).
- **CPU calibration** — dual-method calibration (idle+burst and continuous
  burst), `waste_cpu_cycles` burn loop with optimization barriers,
  `loadwait` with 1-second burst splitting. Pins to calibration CPU during
  measurement (matching C behavior).
- **Scheduling** — CFS, RT (FIFO/RR), SCHED_DEADLINE, and uclamp parameter
  setup via `sched_setattr`/`sched_getattr` syscall wrappers. Scheduling
  failures are fatal (matching C exit behavior).
- **CPU affinity** — per-thread CPU pinning with phase > task > default
  precedence. NUMA support behind `numa` feature flag.
- **Cgroup/taskgroup management** — cgroup v1 cpu controller: mount point
  discovery, nested directory creation/cleanup, thread attachment.
- **Gnuplot generation** — per-thread and aggregate period/run overlay plots
  in PostScript EPS format.
- **Ftrace integration** — trace_marker writing with category bitmask
  filtering (main, task, loop, event, stats, attrs).
- **CLI** — clap-based argument parsing matching the original interface.
- **Logging** — columnar timing output matching the C format for compatibility
  with existing analysis scripts.
- **`doc/tutorial.md`** — comprehensive markdown rewrite of the original C
  tutorial (`doc/tutorial.txt`), with proper headings, fenced code blocks,
  tables, and updated documentation for the built-in JSON preprocessor.
- **JSON fuzzer** (`bug_finding/`) — standalone stress-testing tool that
  generates random valid JSON workloads and compares C rt-app vs rtapp
  behavior (exit codes, timing, output).

### Dependencies

- serde + serde_json (preserve_order) for config parsing
- clap (derive) for CLI
- nix for POSIX/Linux syscall wrappers
- libc for low-level constants
- bitflags for SchedFlags and FtraceLevel
- log + env_logger for structured logging
- thiserror for typed errors
- signal-hook for graceful signal handling

[0.1.1]: https://github.com/rrnewton/rt-app-rs/releases/tag/v0.1.1
