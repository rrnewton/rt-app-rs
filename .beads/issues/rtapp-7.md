---
title: Port CLI argument parsing (rt-app_args.c)
status: open
priority: 2
issue_type: task
depends_on:
  rtapp-3: blocks
  rtapp-1: parent-child
  rtapp-2: blocks
created_at: 2026-02-20T20:34:59.758737270+00:00
updated_at: 2026-02-20T20:35:54.554966197+00:00
---

# Description

Port rt-app_args.c (120 lines). Replace getopt_long with clap. Options: -h/--help, -v/--version (PACKAGE VERSION BUILD_DATE), -l/--log <level> (10=ERROR, 50=NOTICE default, 75=INFO, 100=DEBUG). Positional arg: JSON config file path or - for stdin. Exit codes: EXIT_SUCCESS=0, EXIT_FAILURE=1, EXIT_INV_CONFIG=2, EXIT_INV_COMMANDLINE=3.
