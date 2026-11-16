#!/usr/bin/env bash
# Tickless timer, cpufreq governor and idle accounting smoke test.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send "uname -a"
  send "sleep 1; echo slept"
  send "top -n 2 -b -d 1"
  send "cpupower"
  send "cpupower set powersave"
  send "cpupower"
  send "cpupower set schedutil"
  send "top -n 1 -b"
  send "poweroff" 3
} | timeout 150 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
