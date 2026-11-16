#!/usr/bin/env bash
# Quick boot smoke test: shell works and poweroff is clean at a given SMP count.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  printf 'uname\r'
  sleep 1
  printf 'ls /bin\r'
  sleep 1
  printf 'ps\r'
  sleep 2
  printf 'poweroff\r'
  sleep 3
} | timeout 60 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
