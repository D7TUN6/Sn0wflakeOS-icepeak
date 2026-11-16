#!/usr/bin/env bash
# Pipeline and shell-syntax regression run.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send "echo 'a|b'"
  send 'echo "x|y"'
  send 'echo hello | wc -l'
  send 'seq 1 5 | wc -l'
  send 'seq 1 5 | grep 3'
  send 'seq 1 9 | head -n 3'
  send 'echo hi > /f'
  send 'cat /f | wc -c'
  send 'cat < /f | wc -l'
  send 'ls /bin | head -n 3'
  send 'seq 1 3 | grep 2 | wc -l'
  send 'seq 1 200000 | wc -l'
  send 'seq 1 1000000 | head -n 2'
  send 'echo after-epipe'
  send 'true | false; echo $?'
  send 'false | true; echo $?'
  send 'rm -f /f'
  send 'poweroff' 3
} | timeout 120 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
