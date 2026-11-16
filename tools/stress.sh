#!/usr/bin/env bash
# Spawn/reap stress under SMP: drive many short-lived user programs while the
# shell, timer and IPIs keep every CPU busy.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  printf 'mkdir /s\r'
  sleep 1
  printf 'echo x > /s/a\r'
  sleep 1
  for _ in $(seq 1 20); do
    printf 'cp /s/a /s/b\r';  sleep 0.35
    printf 'mv /s/b /s/c\r';  sleep 0.35
    printf 'wc /s/c\r';       sleep 0.35
    printf 'rm /s/c\r';       sleep 0.35
  done
  printf 'ps\r'
  sleep 2
  printf 'fptest\r'
  sleep 2
  printf 'poweroff\r'
  sleep 3
} | timeout 150 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
