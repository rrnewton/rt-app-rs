# rtapp

A pure Rust port of [rt-app](https://github.com/scheduler-tools/rt-app), the
real-time workload simulator used for testing Linux scheduler behavior.

## Overview

`rtapp` reads a JSON task-set description and generates the corresponding
real-time workload. It's designed as a drop-in replacement for the C `rt-app`
with the same config format, output formats, and runtime behavior.

## Installation

```bash
cargo install rt-app-rs
```

Or build from source:

```bash
git clone https://github.com/rrnewton/rt-app-rs
cd rt-app-rs
cargo build --release
# Binary is at target/release/rtapp
```

## Usage

```bash
rtapp config.json           # Run workload from config file
rtapp -                      # Read config from stdin
rtapp --help                 # Show all options
```

## Configuration

See [`doc/tutorial.md`](doc/tutorial.md) for a comprehensive guide to the JSON
config format. Basic example:

```json
{
  "global": {
    "duration": 5,
    "calibration": "CPU0"
  },
  "tasks": {
    "worker": {
      "policy": "SCHED_FIFO",
      "priority": 10,
      "run": 10000,
      "sleep": 20000
    }
  }
}
```

This creates a FIFO-scheduled thread that runs for 10ms then sleeps for 20ms,
repeating for 5 seconds.

## Features

- All 19 event types from C rt-app (run, runtime, sleep, timer, lock/unlock,
  wait, signal, broadcast, barrier, suspend, resume, mem, iorun, yield, fork)
- CFS, RT (FIFO/RR), and SCHED_DEADLINE scheduling policies
- CPU affinity and cgroup/taskgroup support
- Gnuplot output generation
- Ftrace marker integration
- Built-in workgen preprocessor (no external Python script needed)

## Documentation

- [`doc/tutorial.md`](doc/tutorial.md) — Full tutorial with examples
- [`doc/examples/`](doc/examples/) — Example JSON configurations
- [`CHANGELOG.md`](CHANGELOG.md) — Release notes

## License

GPL-2.0-or-later (same as original rt-app)
