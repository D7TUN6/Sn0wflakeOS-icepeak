#!/usr/bin/env bash
# Focused test for the newly added file utilities.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  printf '%s\r' 'mkdir -p /t/a'; sleep 1
  printf '%s\r' 'echo hello > /t/a/f'; sleep 1
  printf '%s\r' 'head -n 1 /t/a/f'; sleep 2
  printf '%s\r' 'tail -n 1 /t/a/f'; sleep 2
  printf '%s\r' 'grep -n hello /t/a/f'; sleep 2
  printf '%s\r' 'printf %s=%d x 42'; sleep 2
  printf '%s\r' 'echo -e a\tb'; sleep 2
  printf '%s\r' 'stat /t/a/f'; sleep 2
  printf '%s\r' 'du -h /t'; sleep 2
  printf '%s\r' 'df -h'; sleep 2
  printf '%s\r' 'free -h'; sleep 2
  printf '%s\r' 'top -n 1'; sleep 3
  printf '%s\r' 'poweroff'; sleep 3
} | timeout 180 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
