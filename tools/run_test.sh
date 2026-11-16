#!/usr/bin/env bash
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

{
  sleep "$start"; printf 'root\r'; sleep 1; printf 'root\r'; sleep 1; printf 'help\r'
  sleep 2;  printf 'uname\r'
  sleep 1;  printf 'ls /bin\r'
  sleep 2;  printf 'mkdir /d\r'
  sleep 1;  printf 'echo hello-file > /d/f\r'
  sleep 2;  printf 'cat /d/f\r'
  sleep 2;  printf 'cp /d/f /d/g\r'
  sleep 1;  printf 'mv /d/g /d/h\r'
  sleep 1;  printf 'ls /d\r'
  sleep 2;  printf 'wc /d/f\r'
  sleep 2;  printf 'rm /d/h\r'
  sleep 1;  printf 'rm /d/f\r'
  sleep 1;  printf 'rmdir /d\r'
  sleep 1;  printf 'ps\r'
  sleep 2;  printf 'fptest\r'
  sleep 3;  printf 'poweroff\r'
  sleep 3
} | timeout 60 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
