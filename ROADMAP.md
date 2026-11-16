# Sn0wflakeOS roadmap

Status: 0.1.0-alpha. The hobby milestones are complete: BIOS and UEFI boot,
SMP, preemptive multitasking, ring-3 user programs, a fish-like shell, 40
coreutils, Linux-like users/permissions, a framebuffer console and
POSIX-style shell scripts (quotes, `;`, `&&`, `||`, pipes, variables). This
file tracks the work that turns the demo into a real system.

Each milestone is marked done, partial or planned against the code in this
tree. Nothing is listed here as done unless it is in the image.

## Current baseline

- Kernel: `kernel/src/`, Multiboot 1 entry at 1 MiB, identity map of the first
  4 GiB, 6 MiB kernel heap, bitmap frame allocator.
- Processes: one `cr3` per task (`paging::new_user_cr3`), user mappings at
  `0x400000000000`, 32 KiB kernel stacks, 256 KiB user stacks, `MAX_TASKS = 64`,
  `MAX_CPUS = 8`. A ring-3 fault kills only the offending task (exit code
  `128 + vector`); kernel-mode faults still halt. Every user pointer is
  range-checked against the page tables before it is dereferenced in ring 0,
  so a bad pointer returns `EFAULT` instead of faulting the kernel.
  Baseline `free`: ~11.6 MiB physical used, ~3.2 MiB of the 6 MiB heap.
- Scheduling: tickless one-shot timer (TSC/HPET/PM/PIT clock, local-APIC
  one-shot or TSC-deadline), dynamic slices, real `Blocked` state with
  per-waiter wakeups, `SCHED_OTHER/FIFO/RR`, priorities 0..99, per-CPU idle
  contexts, reschedule IPIs.
- Power: C1 `hlt`/`mwait`, C2/C3 through the FADT PM2_CNT/PM_TMR method when
  the firmware advertises them, `schedutil`/`performance`/`powersave`
  governors, Intel HWP/EIST and AMD CPPC/hardware-P-state drivers.
- ACPI: FADT, DSDT `_S5`, HPET, MCFG, MADT, reset register, C-state
  latencies, all parsed in-tree.
- Console: one `Term` with a shadow cell grid, ANSI subset, VGA text or a
  linear framebuffer (BIOS VBE and UEFI GOP both hand off through the same
  `VideoInfo` block at `0x9000`), embedded 8x16 font. Cells whose contents did
  not change are not repainted, scrolling moves framebuffer rows, and
  erase-line/erase-display only touch the non-blank runs, so interactive
  typing and full-screen tools repaint incrementally instead of flickering.
- Drivers: static probe/attach registry (ata, serial, keyboard, speaker) walked
  by `arch::interrupts::init`; `sync::SpinLock` guards the keyboard event ring and
  the UART receive state machine against IRQ/syscall/other-CPU reentry.
- ABI: `abi/src/lib.rs`; `SYS_*` numbers are stable for the current binaries
  (through `SYS_SYNC = 47`).
- Filesystem: in-memory ramfs with POSIX mode/uid/gid and symlinks; the image
  ships a single store closure at `/nix/store/snf-coreutils-0.1` with
  `/bin`, `/usr/bin`, `/sbin` and `/run/current-system` as symlinks into it.
  `/etc/passwd`, `/etc/shadow`, `/etc/group`, `/etc/os-release`,
  `/etc/hostname` and `/etc/motd` are seeded at boot. Mutations of persistent
  paths are appended to a checksummed log on the first ATA disk
  (`fs/journal.rs`); the log is replayed and compacted at boot. `/proc`,
  `/dev`, `/tmp`, `/run` and `/nix/store` are never logged. Without a disk
  the image is memory-only.
- Storage: ATA PIO (`drivers/ata.rs`), no DMA, no PCI dependency, no
  interrupt handler.
- User programs: static no_std ELF64 built from `user/`, plus C programs
  built by `tools/snf-cc` against `libc/` (freestanding libc: malloc, printf,
  file descriptors, strings). `init` (pid 1) starts `login`; the live CD
  auto-logs in `user` and `root` authenticates with `root`.

## Milestone A - real address spaces and demand paging (planned)

Goal: stop pre-mapping every page a program can touch and let the page fault
handler populate memory on first use.

1. Track `vm_area` records per task: `start`, `end`, `flags`, `kind`
   (text/data/heap/stack), with a sorted list in `Task`.
2. Replace `paging::map_user_range` eager allocation in the ELF loader with
   `vm_area` registration. Reserve virtual ranges; map only what is touched.
3. Page-fault handler (`vector 14`):
   - read `CR2`, decode the error code (present/write/user/instruction),
   - find the owning `vm_area`, allocate a zeroed frame, map it, retry,
   - `SIGSEGV`-style kill when no area matches or permissions fail,
   - stack growth up to a limit, `brk`/`mmap` growth for the heap.
4. Copy-on-write for `fork`/`exec`: mark writable private pages read-only and
   resolve on write fault. Keep a per-frame refcount.
5. Free `vm_area` pages in `paging::destroy_user_cr3`, including frames shared
   with other address spaces.

Acceptance: `fptest` still passes, `free` shows low resident usage after boot,
`top` shows per-task RSS, and a program touching 100 MiB of heap succeeds
without 100 MiB of physical memory.

## Milestone B - native binaries and a userland toolchain (partial)

Goal: build real programs for Sn0wflakeOS instead of the current fixed-ABI
stubs.

1. Freeze `abi/src/lib.rs` as `snf-abi` with a version word and a syscall
   table (`syscall(number, args)` is already the only entry point).
2. Add `user/crt0`: `_start` parses `argc/argv/envp`, sets up TLS, calls
   `main`, then `exit`. Keep the current `_start(argc, argv)` path for
   existing binaries during the transition.
3. Provide `libsnf`: a small libc subset - `malloc/free`, `printf` family,
   `open/read/write/close`, `stat`, `opendir/readdir`, `fork/exec/wait`,
   `signal`, `pipe`, `getenv/setenv`, `time`.
4. Ship a `snf-gcc` wrapper or a `clang` target triple. Until a real
   cross-compiler exists, keep building no_std Rust binaries and add a
   `user/sdk` crate that links against `snf-abi`.
5. Move the coreutils to the new SDK one program at a time; keep
   `build.sh`'s `USER_BINS` list as the source of truth.

Acceptance: a "hello, world" C program compiled with the SDK runs in ring 3,
and `ps` shows the same ABI version as the kernel.

## Milestone C - POSIX compatibility layer (partial)

Goal: run unmodified POSIX source with minimal shims.

1. Syscall layer (`kernel/src/syscall/mod.rs`):
   - file descriptors: a real per-process fd table (`stdin/stdout/stderr`,
     files, directories, pipes), replacing `current_out_file`/`in_file`,
   - `open` flags (`O_RDONLY`, `O_WRONLY`, `O_CREAT`, `O_TRUNC`, `O_APPEND`,
     `O_DIRECTORY`), `lseek`, `dup`, `dup2`, `fcntl`,
   - `fork`, `execve`, `waitpid`, `getpid/getppid`, `kill`, `signal`,
   - `mmap/munmap/mprotect`, `brk`,
   - `clock_gettime`, `nanosleep`, `gettimeofday`,
   - `getcwd/chdir`, `stat/lstat/fstat`, `readdir`, `unlink`, `rename`.
2. VFS: mount points, inode-like handles, path resolution with symlinks, and a
   ramfs backend first; add a persistent disk backend after Milestone D.
3. Signals: `SIGINT/SIGTERM/SIGKILL/SIGSEGV/SIGCHLD`, a pending mask, and
   default actions. Wire `Ctrl+C` to the foreground process group.
4. Process groups and job control: `setpgid`, `tcsetpgrp`, `SIGTSTP/SIGCONT`;
   the shell gets `fg`/`bg`/`jobs` and `&` background execution.

Acceptance: `sh` runs a small POSIX script using functions, `case`, `for`,
redirection and pipelines; `cat` and `grep` work on pipes without temp files.

## Milestone D - streaming pipes, TTY and a real shell (partial)

1. Kernel pipe objects: 64 KiB ring buffer, `SYS_PIPE`, blocking read/write,
   `POLLIN/POLLOUT`, close propagation (`EPIPE`).
2. Replace peaksh's temp-file pipeline with real pipes. Keep the temp-file
   path as a fallback for ramfs-only builds.
3. TTY layer: line discipline (canonical mode, echo, `Ctrl+C`, `Ctrl+D`,
   `Ctrl+Z`), `ioctl(TIOCGWINSZ)`, per-terminal foreground group.
4. Shell: `for/while/if/case`, functions, `$(...)`, `${var:-default}`,
   arithmetic `$(( ))`, here-documents, `set -e`.

Acceptance: `yes | head -n 3`, `cat | cat | wc -l`, and a `while read` loop
work interactively and in scripts.

## Milestone E - persistence and robustness (partial)

Done: an ATA PIO block driver, an append-only checksummed log with replay and
compaction, volatile `/proc`, `/dev`, `/tmp` and `/run`, and per-task fault
containment for ring-3 faults.

Remaining:

1. A real on-disk filesystem under the log: block cache, allocation map, and
   a recovery check on mount instead of replaying the whole journal.
2. W^X user pages, stack guard pages, ASLR for user mappings, and
   `SMEP/SMAP/NX` where the CPU supports them.
3. A panic screen with a register dump, a task dump and a reboot key, plus a
   kernel log ring buffer readable via `dmesg`.
4. `dmesg` and a boot log that survives into userland.

Acceptance: files survive reboot, `dmesg` shows a clean boot, and a
deliberately faulting program dies without taking down the kernel.

## Milestone F - system userland (mostly done)

Goal: boot into a real Unix session instead of a single hardcoded shell.

Done: `init` (pid 1, respawns `login` and reaps children), `login` with
`/etc/passwd` + `/etc/shadow` (live CD `user` has no password, `root` uses
`root`), a freestanding C libc (`malloc/free`, `printf`, `open/read/write/
close/lseek`, strings), `tools/snf-cc` + `crt0` + headers, the Nix-style store
with FHS symlinks, and a compiled C `libc-test` running in ring 3.

Remaining:

1. `init`: mount pseudo-filesystems (`/proc`, `/dev`), handle `SIGCHLD`
   instead of polling `wait`, and run shutdown/reboot targets.
2. Persistence: a writable on-disk filesystem under `/nix`, replacing the ISO
   store that is rebuilt on every boot.
3. Real descriptor tables in the kernel so `open`/`read`/`write` do not need
   the whole-file emulation the first libc uses.

Done since the first draft:

- Pseudo-filesystems: `/proc/meminfo`, `/proc/cpuinfo`, `/proc/uptime`,
  `/proc/version`, `/dev/null` and `/dev/zero` are `ramfs::Dynamic` nodes, so
  path resolution, permissions, `stat` and `ls` work unchanged while the
  contents are generated on access (`/dev/null` is a write sink).
- Driver split: `drivers/mod.rs` holds a `Driver { name, probe, attach }` table
  (serial, keyboard, speaker) that `arch::interrupts::init` walks, so the
  kernel no longer hardcodes device setup. The framebuffer lives behind
  `video.rs` + `console.rs`.
- Framebuffer console: both loaders select a linear-framebuffer mode (BIOS
  VBE 1024x768x32/800x600/640x480, UEFI GOP with the same preference) and
  publish a `VideoInfo` block at `0x9000`. The console keeps a shadow cell
  grid, renders it with the embedded 8x16 font, and falls back to VGA text.
- Input integrity: the keyboard event ring and the UART receive state machine
  are now guarded by interrupt-safe spinlocks (`sync::SpinLock`); the ring had
  multiple producers (IRQ 4, syscalls, console writes on other CPUs) and was
  silently dropping events. The console drains the UART during long writes,
  scrolling moves framebuffer rows instead of repainting every cell, and the
  16550 trigger level is one byte. `tools/input_probe.sh` is the regression
  probe for typed input under load.

Acceptance: the live CD boots to a login prompt (or auto-login), `id` reports
the session user, a compiled C program runs, and `/bin/ls` resolves through
the Nix-style store layout.

## Not started

These were promised by older READMEs and are not in the tree. They stay here
so the gap is visible.

- 32-bit x86 (i386) kernel and boot path. 0.1.0 boots on x86_64 only, even
  though the 0.0.x lineage was a 32-bit protected-mode system.
- Network stack, TCP/IP and network drivers.
- Graphics stack: modesetting beyond the boot framebuffer, a Wayland-like
  compositor, Mesa, and graphics drivers.
- Audio server (PipeWire-inspired) and audio drivers.
- Package manager and system manager with a Nix-like store and reproducible
  builds, plus `/nix/store` as a read-only mount instead of an ISO payload.
- Ports: OpenSSH, PAM, Python, a Helix-like editor.
- A block filesystem that survives power failure by design (working name
  FStwndopf), replacing the replay log.
- Real hardware support. Everything is tested in QEMU.

## Test matrix (run after every milestone)

| Test | Command |
| --- | --- |
| Full matrix (dev+release, BIOS+UEFI) | `SMP=4 nix develop --command ./tools/matrix.sh` |
| Full suite (BIOS) | `SMP=4 nix develop --command ./tools/suite.sh` |
| Full suite (UEFI) | `UEFI=1 SMP=4 nix develop --command ./tools/suite.sh` |
| BIOS boot + coreutils | `SMP=1 nix develop --command ./tools/coreutils_test.sh` |
| BIOS shell syntax | `SMP=1 nix develop --command ./tools/shell_test.sh` |
| BIOS pipelines | `SMP=1 nix develop --command ./tools/pipe_test.sh` |
| Login + FHS/store layout | `SMP=2 nix develop --command ./tools/login_test.sh` |
| C toolchain + libc | `SMP=2 nix develop --command ./tools/libc_test.sh` |
| Tickless idle + cpufreq | `SMP=2 nix develop --command ./tools/tickless_test.sh` |
| UEFI boot + coreutils | `UEFI=1 SMP=4 nix develop --command ./tools/coreutils_test.sh` |
| UEFI login + libc | `UEFI=1 SMP=2 nix develop --command ./tools/login_test.sh` |
| SMP stress | `SMP=1/4/8 nix develop --command ./tools/stress.sh` |
| Ring-3 fault containment | `SMP=2 nix develop --command ./tools/fault_test.sh` |
| `/proc` + `/dev` pseudo-files | `SMP=2 nix develop --command ./tools/proc_test.sh` |
| Typed-input integrity under load | `SMP=4 nix develop --command ./tools/input_probe.sh` |
| Interactive typing + full-screen repaint | `SMP=4 nix develop --command ./tools/regress.sh` |
| Rapid input + many redraws | `SMP=4 nix develop --command ./tools/mash.sh` |
| Hang regression | `GAP=1 nix develop --command ./tools/hang_test.sh` |
| Framebuffer OCR | `WAIT=24 start=13 ./tools/fb_interact.sh 'ls -l /bin' /tmp/shot.ppm` then `perl tools/ppmocr.pl /tmp/shot.ppm` |

A run passes when it reaches `Shutting down...`, `fptest: ok` is printed in
the stress run, and no `panic`, `CPU EXCEPTION`, `unhandled interrupt`,
`command not found`, or `killed:` appears. `fault_test.sh` additionally expects
each `faulttest` run to print a `killed:` line and the shell to survive.
`tools/fb_screenshot.sh` plus `tools/ppmocr.pl` capture the guest framebuffer
and OCR it against `kernel/src/font.rs` to confirm the console renders on both
firmware paths.

## Verification (2026-11-17)

- `./build.sh release` on the 0.1.0-alpha tree produces `sn0wflake.iso`
  (9117 sectors), boots in QEMU with `-m 512M -smp 4`, and the console runs
  `uname -a` and `free -h`. The capture is `docs/screenshot.png`.
- Earlier full runs on 2026-11-15: `tools/suite.sh` green on BIOS and UEFI,
  dev and release (`SMP=4`):
  coreutils, shell syntax, pipelines, login, libc, `/proc`+`/dev`, tickless
  idle, SMP stress, input probe, rapid-input mash.
- `tools/fault_test.sh`: `faulttest divzero` and `faulttest badptr` die alone
  with exit codes 128 and 142, a bogus kernel pointer returns `EFAULT` from
  `write` instead of faulting in ring 0, and the shell survives all three.
- `tools/fuzz_input.sh`: 40 s of mixed typing, control bytes and escape
  sequences produced no exception, panic or killed task.
- Framebuffer OCR of `ls -l /bin` and `top -d 1` shows aligned glyphs and no
  stale rows after repeated full-screen repaints.
- Baseline `free`: ~12.6 MiB physical used, ~3.2 MiB of the 6 MiB heap.
- One release/BIOS `pipe_test` run hit its 120 s timeout after printing
  `Shutting down...`; three immediate reruns were clean, so it is recorded as
  a host-side flake rather than a guest hang.
