#!/usr/bin/env bash
# Minimal repeated-command hang probe.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14
gap="${GAP:-1}"

send() { printf '%s\r' "$1"; sleep "${2:-$gap}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send 'echo one'
  send 'echo two'
  send 'echo three'
  send 'echo four'
  send 'poweroff' 3
} | timeout 90 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
