---
title: Port CPU affinity and NUMA binding
status: closed
priority: 2
issue_type: task
depends_on:
  rtapp-1ec29: blocks
  rtapp-450e3: blocks
  rtapp-075d2: parent-child
created_at: 2026-02-20T20:35:22.356735970+00:00
updated_at: 2026-02-20T20:52:04.470651850+00:00
closed_at: 2026-02-20T20:52:04.470651739+00:00
---

# Description

Port CPU affinity management: create_cpuset_str() converts cpu_set_t to human-readable string, set_thread_affinity() applies phase>task>default precedence using pthread_setaffinity_np/sched_setaffinity. Port NUMA binding: set_thread_membind() using libnuma (conditional feature). Parse cpuset from JSON array of CPU indices, numaset from JSON array of node indices.
