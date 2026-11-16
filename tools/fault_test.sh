#!/usr/bin/env bash
# Ring-3 fault containment: a crashing user program must die alone, the shell
# must survive, and a bogus pointer must be rejected with EFAULT in ring 0.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send 'faulttest divzero' 2
  send 'echo survived-divzero'
  send 'faulttest badptr' 2
  send 'echo survived-badptr'
  send 'faulttest kernel' 2
  send 'echo survived-kernel'
  send 'poweroff' 3
} | timeout 90 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
