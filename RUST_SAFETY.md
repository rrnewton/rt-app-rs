# Unsafe Rust Usage in rt-app-rs

This document catalogs every use of `unsafe` in the codebase, why it is
required, and what invariants preserve safety.

## Summary

| # | Location | What | Why safe Rust cannot do it |
|---|----------|------|---------------------------|
| 1 | `syscalls.rs:116` | `sched_setattr` syscall | No libc/nix wrapper exists |
| 2 | `syscalls.rs:149` | `sched_getattr` syscall | No libc/nix wrapper exists |
| 3 | `engine/scheduling.rs:130` | Calls `sched_setattr` | Caller of unsafe fn #1 |
| 4 | `engine/mod.rs:406` | `pthread_setname_np` | No safe Rust API for thread naming |
| 5 | `engine/mod.rs:449` | `mlockall` | No safe Rust API for page locking |
| 6 | `engine/events.rs:244` | `sched_yield` | No safe Rust wrapper in std/nix |

Total: **6 unsafe call sites** across 4 files, plus 2 `unsafe fn` signatures.

Additionally, 2 `#[repr(C)]` structs exist to match the kernel ABI
(`SchedAttr` in both `types.rs` and `syscalls.rs`), and
`std::hint::black_box` is used in `calibration.rs` (safe, not unsafe).

---

## Detailed Analysis

### 1. `sched_setattr(2)` syscall — `src/syscalls.rs:116`

```rust
pub unsafe fn sched_setattr(pid: libc::pid_t, attr: &SchedAttr, flags: u32) -> io::Result<()>
```

**Why unsafe:** This is a direct `libc::syscall()` invocation for
`SYS_sched_setattr`, a Linux-specific system call that has no wrapper in
libc, nix, or any other crate. The kernel reads a `struct sched_attr`
from a raw pointer.

**Safety invariants:**
- `attr` must point to a valid `SchedAttr` with `size == 56` (enforced
  by `SchedAttr::new()` and unit-tested against `SCHED_ATTR_SIZE`)
- `pid` must be 0 (current thread) or a valid PID
- The `#[repr(C)]` layout matches the kernel's `linux/sched/types.h`
  definition (verified by `sched_attr_size_matches_kernel` test)

**Why this is safe in practice:** The only call site
(`engine/scheduling.rs:130`) always uses `gettid()` for the pid and
constructs `SchedAttr` via builder functions that set the size field.

---

### 2. `sched_getattr(2)` syscall — `src/syscalls.rs:149`

```rust
pub unsafe fn sched_getattr(pid: libc::pid_t, attr: &mut SchedAttr, flags: u32) -> io::Result<()>
```

**Why unsafe:** Same as #1 — direct syscall with no safe wrapper
available. The kernel writes into the `SchedAttr` buffer via raw pointer.

**Safety invariants:**
- `attr` must be a valid mutable reference to a `SchedAttr`
- The size passed to the kernel equals `mem::size_of::<SchedAttr>()`
  (computed inside the function, not caller-controlled)
- `pid` must be 0 or a valid PID

**Why this is safe in practice:** The size is hardcoded from
`mem::size_of`, and the only call site is a unit test with `pid=0`.

---

### 3. Calling `sched_setattr` — `src/engine/scheduling.rs:130`

```rust
unsafe { syscalls::sched_setattr(tid, attr, 0) }
```

**Why unsafe:** Calling an `unsafe fn` requires an unsafe block. This is
the sole call site for `sched_setattr` outside of tests.

**Safety invariants:** The `SchedAttr` is constructed by
`build_cfs_attr`, `build_rt_attr`, `build_deadline_attr`, or
`build_uclamp_attr` — all of which set `size` via `SchedAttr::new()`.
The `tid` comes from `nix::unistd::gettid()`, which always returns a
valid TID for the calling thread.

---

### 4. `pthread_setname_np` — `src/engine/mod.rs:406`

```rust
unsafe { libc::pthread_setname_np(libc::pthread_self(), c_name.as_ptr()) }
```

**Why unsafe:** `pthread_setname_np` is a POSIX extension with no safe
Rust wrapper. It takes a raw `*const c_char` pointer.

**Safety invariants:**
- `pthread_self()` always returns a valid thread ID for the caller
- `c_name` is a `CString` that is alive for the duration of the call,
  ensuring the pointer is valid and null-terminated
- Linux silently truncates names to 16 bytes — no buffer overflow

**Why not use `std::thread::Builder::name`:** Thread names must be set
before spawn with the std API. rt-app needs to rename threads after
spawn (e.g., when forking worker threads at runtime).

---

### 5. `mlockall` — `src/engine/mod.rs:449`

```rust
unsafe { libc::mlockall(libc::MCL_CURRENT | libc::MCL_FUTURE) }
```

**Why unsafe:** `mlockall(2)` is a Linux syscall with no safe Rust
wrapper. It affects the entire process's virtual memory.

**Safety invariants:**
- `MCL_CURRENT | MCL_FUTURE` are valid kernel constants
- The call has no memory safety implications — it only affects paging
  behavior (preventing page faults for real-time determinism)
- Failure is non-fatal: the code logs a warning and continues

**Why this exists:** Real-time workloads require locked pages to avoid
non-deterministic page fault latency. This matches the C original's
behavior.

---

### 6. `sched_yield` — `src/engine/events.rs:244`

```rust
unsafe { libc::sched_yield() };
```

**Why unsafe:** `libc::sched_yield` is declared `unsafe` in the libc
crate, though it has no preconditions and cannot cause undefined behavior.

**Safety invariants:** None needed — `sched_yield(2)` is unconditionally
safe. It takes no arguments and simply yields the CPU to the scheduler.
The `unsafe` annotation on the libc binding is overly conservative.

---

## `#[repr(C)]` Structs

Two `SchedAttr` structs use `#[repr(C)]` to match the kernel ABI:

| Location | Purpose |
|----------|---------|
| `types.rs:646` | Used by config parsing and engine for scheduling data |
| `syscalls.rs:25` | Used directly in syscall wrappers |

Both have identical field layouts matching `linux/sched/types.h`. The
duplication exists because the syscalls module was ported independently
from the types module. A future cleanup could unify them.

`#[repr(C)]` itself is not unsafe — it only controls struct layout.

## `std::hint::black_box`

Used in `engine/calibration.rs:63-66` to prevent the compiler from
optimizing away the CPU calibration busy loop. `black_box` is a safe
function (stabilized in Rust 1.66) and is not an unsafe operation.

## Design Principle

The project follows the CLAUDE.md guideline: *"Prefer safe Rust as much
as possible, and justify carefully in a SAFETY comment any use of unsafe,
why it is required, and why it preserves safety."*

All 6 unsafe sites are direct OS/kernel interactions with no safe
alternative. No application logic uses unsafe.
