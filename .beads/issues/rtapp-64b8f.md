---
title: 'Compatibility testing harness: C vs Rust comparison'
status: closed
priority: 2
issue_type: task
depends_on:
  rtapp-52f69: blocks
  rtapp-075d2: parent-child
  rtapp-821aa: blocks
  rtapp-49a52: blocks
  rtapp-1ec29: blocks
  rtapp-f4bb8: blocks
  rtapp-450e3: blocks
  rtapp-4b15d: blocks
  rtapp-f81ac: blocks
  rtapp-812a2: blocks
  rtapp-7c9cd: blocks
created_at: 2026-02-20T20:35:34.783946414+00:00
updated_at: 2026-02-20T21:25:44.076283079+00:00
closed_at: 2026-02-20T21:25:44.076282988+00:00
---

# Description

Build a test harness that runs the original C rt-app binary and the Rust port side-by-side with identical JSON configs, then compares: (1) log file format compatibility (column layout, field values within tolerance), (2) exit codes, (3) gnuplot file generation, (4) ftrace marker format, (5) stderr logging format. This ensures the Rust port is a true drop-in replacement. Include all example JSON configs from doc/examples/.
