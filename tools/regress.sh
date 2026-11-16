#!/usr/bin/env bash
# Interactive regression driver: heavy typing, repeated full-screen updates and
# many short-lived processes, looking for exceptions and stalls.
#   SMP=4 ./tools/regress.sh
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

{
  sleep "$start"
  printf 'root\r'; sleep 1; printf 'root\r'; sleep 2
  # Type a long command one byte at a time so every keystroke repaints.
  for ch in l s space minus l space slash b i n; do
    case "$ch" in space) printf ' ' ;; minus) printf '-' ;; *) printf '%s' "$ch" ;; esac
    sleep 0.08
  done
  printf '\r'
  sleep 3
  printf 'top -n 3 -d 1\r'
  sleep 6
  printf 'free\r'
  sleep 2
  printf 'ps\r'
  sleep 2
  printf 'echo one two three four five six seven eight nine ten\r'
  sleep 2
  printf 'help\r'
  sleep 3
  printf 'uname -a\r'
  sleep 2
  printf 'poweroff\r'
  sleep 3
} | timeout 120 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
