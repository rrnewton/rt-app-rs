# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-03-05

### Removed

- **`"calibration": "precise"` global mode** — this mode made all run/runtime
  events use clock-only spinning, which is too coarse. Use per-event
  `"runtime": {"duration": N, "mode": "clockonly"}` instead.
- **`CpuBurnMode` enum** — with no global precise mode, the engine uses
  `NsPerLoop` directly.

### Added

- **Per-event runtime mode** via object-form syntax:
  `"runtime": {"duration": N, "mode": "clockonly"}` for exact wall-clock spin
  without FPU workload. The default `"runtime": N` (integer form) and
  `"mode": "loadwait"` continue to use calibrated busy-loop chunks with
  clock checking, matching upstream C rt-app behavior.
- **YAML config file support** — config files with `.yaml` or `.yml` extension
  are now accepted alongside JSON. Uses serde_yaml for parsing with the same
  config schema.
- **`--print-template=yaml` CLI option** — prints a comprehensive YAML config
  template as an alternative to `--print-template` (JSON). Both formats
  document all available options with inline comments.

## [0.2.0] - 2026-02-22

### Added

- **`//` line comments** in JSON config files — in addition to the existing
  `/* ... */` block comments, line comments (`// to end of line`) are now
  supported and stripped during preprocessing.
- **`"calibration": "precise"` mode** — new calibration mode where `run` and
  `runtime` events spin on `clock_gettime(CLOCK_MONOTONIC)` for exact
  wall-clock duration instead of using calibrated busy-loop iterations.
  Testing shows 0% variance vs 5-20% variance with calibrated mode.
- **`--print-template` CLI option** — prints a comprehensive JSON config
  template with syntax highlighting (when output is a TTY), documenting all
  available options and their defaults. Use `rtapp --print-template` for a
  quick reference without consulting documentation.
- **Expanded `doc/examples/template.json`** — now serves as a comprehensive
  quick-reference for all config options with inline comments explaining
  each field.
- **`PORT_STATUS.md`** — documents port history and upstream sync status.
- **Upstream sync workflow** in `CLAUDE.md` — instructions for syncing with
  upstream rt-app changes.

### Changed

- **Fuzzer requires submodule** — `rt-app-fuzzer` now requires the C rt-app
  to be built from the `rt-app-orig` submodule. Use `--build` flag to
  automatically compile it.

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

[0.3.0]: https://github.com/rrnewton/rt-app-rs/releases/tag/v0.3.0
[0.2.0]: https://github.com/rrnewton/rt-app-rs/releases/tag/v0.2.0
[0.1.1]: https://github.com/rrnewton/rt-app-rs/releases/tag/v0.1.1
