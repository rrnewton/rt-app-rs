---
title: Port gnuplot generation
status: open
priority: 2
issue_type: task
depends_on:
  rtapp-075d2: parent-child
  rtapp-1ec29: blocks
  rtapp-450e3: blocks
created_at: 2026-02-20T20:35:17.410367715+00:00
updated_at: 2026-02-20T20:39:02.681696141+00:00
---

# Description

Port gnuplot generation from rt-app.c. Functions: setup_thread_gnuplot() generates per-thread .plot files, setup_main_gnuplot() generates aggregate period and run time overlay plots. Output format: PostScript EPS. Plots: load vs time, run vs time, period vs time. Uses columnar log file data.
