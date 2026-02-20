---
title: Port core types and data model (rt-app_types.h)
status: closed
priority: 1
issue_type: task
depends_on:
  rtapp-450e3: blocks
  rtapp-075d2: parent-child
created_at: 2026-02-20T20:34:32.436200287+00:00
updated_at: 2026-02-20T21:09:42.645036958+00:00
closed_at: 2026-02-20T21:09:42.645036858+00:00
---

# Description

Port all type definitions from rt-app_types.h (313 lines) to Rust. Includes: policy_t enum (Other/Idle/RR/FIFO/Deadline/Same), resource_t enum (19 event types), all structs: rtapp_mutex, rtapp_cond, rtapp_barrier, rtapp_timer, rtapp_iomem_buf, rtapp_iodev, rtapp_fork, rtapp_resource_t (union of resource subtypes), event_data_t, cpuset_data_t, numaset_data_t, sched_data_t, taskgroup_data_t, phase_data_t, thread_data_t, rtapp_options_t, timing_point_t, log_data_t. Use strong typing: newtype wrappers for durations (usec/nsec), thread indices, resource indices. Use enums with data variants instead of C unions.
