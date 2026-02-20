---
title: Port cgroup/taskgroup management (rt-app_taskgroups.c)
status: open
priority: 2
issue_type: task
depends_on:
  rtapp-450e3: blocks
  rtapp-1ec29: blocks
  rtapp-075d2: parent-child
created_at: 2026-02-20T20:35:06.945797173+00:00
updated_at: 2026-02-20T20:39:02.681385684+00:00
---

# Description

Port rt-app_taskgroups.c (407 lines). Implements cgroups v1 cpu controller integration. Functions: initialize_taskgroups (alloc array, max 32), alloc/find_taskgroup, set/reset_thread_taskgroup (write pid to cgroup tasks file), cgroup_check_cpu_controller (parse /proc/cgroups), cgroup_get_cpu_controller_mount_point (parse /proc/mounts), cgroup_mkdir/rmdir (nested directory creation/removal with offset tracking), add/remove_cgroups, cgroup_attach_task. Constraint: taskgroups only with SCHED_OTHER or SCHED_IDLE.
