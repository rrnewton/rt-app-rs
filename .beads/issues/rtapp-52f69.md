---
title: Port utilities module (rt-app_utils.c/h)
status: closed
priority: 1
issue_type: task
depends_on:
  rtapp-075d2: parent-child
  rtapp-450e3: blocks
  rtapp-1ec29: blocks
created_at: 2026-02-20T20:34:39.115930838+00:00
updated_at: 2026-02-20T21:09:42.647070743+00:00
closed_at: 2026-02-20T21:09:42.647070643+00:00
---

# Description

Port rt-app_utils.c (351 lines) and rt-app_utils.h (153 lines). Includes: timespec arithmetic (to_usec, to_nsec, from_usec, from_msec, add, sub, lower, sub_to_ns), log_timing() columnar output, gettid() wrapper, string_to_policy/policy_to_string, string_to_resource/resource_to_string, ftrace_setup() bitmask parsing, ftrace_write() variadic trace_marker writer. Logging macros become Rust log crate integration. Ftrace categories: MAIN=0x01, TASK=0x02, LOOP=0x04, EVENT=0x08, STATS=0x10, ATTRS=0x20.
