#!/usr/bin/env bash
# Pseudo-filesystem checks: /proc content is generated on access, /dev/null is
# a sink and /dev/zero yields zeros.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send 'cat /proc/version'
  send 'cat /proc/uptime'
  send 'cat /proc/meminfo'
  send 'cat /proc/cpuinfo'
  send 'ls /proc'
  send 'ls -l /dev'
  send 'echo sink > /dev/null'
  send 'cat /dev/null'
  send 'wc -c /dev/null'
  send 'head -c 4096 /dev/zero | wc -c'
  send 'echo proc-ok'
  send 'poweroff' 3
} | timeout 90 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
