#!/usr/bin/env bash
# C toolchain + libc smoke test. The binary is built by build.sh from
# libc/test/libc-test.c with the same flags tools/snf-cc uses.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  # Live-CD user: no password.
  printf '\r'; sleep 2
  send 'libc-test alpha beta'
  send 'echo $?'
  send 'stat /tmp/libc.txt'
  send 'cat /tmp/libc.txt'
  send 'poweroff' 3
} | timeout 120 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
