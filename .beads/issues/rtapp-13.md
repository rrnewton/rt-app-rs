---
title: 'Compatibility testing harness: C vs Rust comparison'
status: open
priority: 2
issue_type: task
depends_on:
  rtapp-1: parent-child
  rtapp-7: blocks
  rtapp-10: blocks
  rtapp-6: blocks
  rtapp-11: blocks
  rtapp-2: blocks
  rtapp-4: blocks
  rtapp-8: blocks
  rtapp-3: blocks
  rtapp-9: blocks
  rtapp-5: blocks
created_at: 2026-02-20T20:35:34.783946414+00:00
updated_at: 2026-02-20T20:36:59.714127257+00:00
---

# Description

Build a test harness that runs the original C rt-app binary and the Rust port side-by-side with identical JSON configs, then compares: (1) log file format compatibility (column layout, field values within tolerance), (2) exit codes, (3) gnuplot file generation, (4) ftrace marker format, (5) stderr logging format. This ensures the Rust port is a true drop-in replacement. Include all example JSON configs from doc/examples/.
