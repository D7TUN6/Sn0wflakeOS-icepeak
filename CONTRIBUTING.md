# Contributing

Sn0wflakeOS is a hobby kernel. Contributions are welcome, but the bar is the
same as for the code that is already here: it has to build, it has to boot, and
someone has to understand it.

## Before you write code

- Open an issue or start a discussion for anything larger than a typo. Say what
  you want to change and why. This avoids work in the wrong direction.
- Small fixes (a broken link, an obvious bug, a test that does not run) can go
  straight to a pull request.
- Read [ROADMAP.md](ROADMAP.md) first. Work that belongs to a milestone is
  easier to accept than a new subsystem nobody planned.

## Build and test

The Nix flake is the supported build environment:

```sh
nix develop
./build.sh release
./run.sh
```

Run the suite before you open a pull request:

```sh
nix develop --command ./tools/suite.sh
UEFI=1 nix develop --command ./tools/suite.sh
```

If your change touches the scheduler, paging, interrupts or the console, also
run `tools/stress.sh` and `tools/fault_test.sh`. State in the pull request what
you ran and what the result was. "Should work" is not a test result.

## Code style

- Rust 2024, `no_std`, no external crates in the kernel beyond the in-tree
  `abi` crate.
- Keep the arch-specific code in `kernel/src/arch/`, the drivers in
  `kernel/src/drivers/`, and the syscall surface in `kernel/src/syscall/`.
- Comments explain why, not what. A comment that repeats the next line is
  noise, and so is a comment that records the history of a fix. Git has that.
- User-visible strings are lowercase subsystem prefixes: `smp:`, `persist:`,
  `[sched]`. See [DESIGN.md](DESIGN.md).
- No em dashes or curly quotes in code, docs or commit messages.
- SPDX headers are not required; the project is MIT as a whole.

## Commits and pull requests

- One logical change per commit. Subject in the imperative, under 72
  characters, no trailing period: `sched: wake only the waiters for an event`.
- Describe the test evidence in the pull request body, not in the commit
  subject.
- Keep unrelated reformatting out of a functional change.
- A pull request that changes the on-disk format, the syscall ABI or the boot
  protocol must say so in the title.

## AI-assisted contributions

AI-assisted pull requests are accepted, on one condition: a human is in the
loop.

That means:

- A person reviewed and understands every changed line, and can answer
  questions about it in review.
- A person ran the build and the tests on their own machine, and pasted the
  result.
- The pull request says which tool was used and for what (for example "draft
  of the ACPI parser, reviewed and tested by me").
- The person who submits it is responsible for it. "The model wrote it" is not
  an answer to a review comment.

Pull requests that are fully automated, unreviewed, or that cannot explain
their own changes are closed without a detailed review. This is not a ban on
AI tools; it is a ban on dumping generated code into someone else's kernel.

Do not paste credentials, private logs or other people's code into a model, and
do not submit code you cannot license under MIT.

## Reporting bugs

Include:

- the ISO version or commit,
- BIOS or UEFI, and the QEMU command line,
- the serial log, starting from the first line after the boot banner,
- what you expected and what happened.

A bug without a log is hard to reproduce. A bug with a log and a command line
usually gets fixed.
