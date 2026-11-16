#!/usr/bin/env bash
# Mash the keyboard: rapid bytes, long lines, many redraws, repeated commands.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

{
  sleep "$start"
  printf 'root\r'; sleep 1; printf 'root\r'; sleep 2
  # A long line typed at full speed.
  printf 'echo aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r'
  sleep 1
  # Rapid keystrokes, including cursor movement and deletion.
  printf 'ls /bin/../../bin'; printf '\x1b[D\x1b[D\x1b[D'; printf '\x7f\x7f'; printf '\r'
  sleep 2
  for i in $(seq 1 30); do
    printf 'echo run%s\r' "$i"
    sleep 0.15
  done
  sleep 2
  for i in $(seq 1 10); do
    printf 'ls -l /bin\r'
    sleep 0.2
  done
  sleep 2
  printf 'top -n 2 -d 1\r'
  sleep 5
  printf 'poweroff\r'
  sleep 3
} | timeout 150 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
