#!/usr/bin/env bash
# Random-input soak: feed the console a long stream of printable bytes,
# control characters and escape sequences while commands run underneath. The
# shell must stay responsive and the kernel must not take an exception.
#   SMP=4 DURATION=60 ./tools/fuzz_input.sh
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14
duration="${DURATION:-45}"

{
  sleep "$start"
  printf 'root\r'; sleep 1; printf 'root\r'; sleep 2
  # Deterministic pseudo-random stream: printable text mixed with control
  # bytes (backspace, escape sequences, tab, Ctrl-U, Ctrl-C) and newlines.
  end=$((SECONDS + duration))
  i=0
  while [ "$SECONDS" -lt "$end" ]; do
    i=$((i + 1))
    case $((i % 9)) in
      0) printf 'echo fuzz-%d\r' "$i" ;;
      1) printf 'ls /bin\r' ;;
      2) printf 'abc\x7f\x7f\x7fdef' ;;
      3) printf '\x1b[D\x1b[D\x1b[C\x1b[3~' ;;
      4) printf 'cat /etc/motd\r' ;;
      5) printf 'top -n 1 -b\r' ;;
      6) printf '\t\t' ;;
      7) printf 'pwd\r' ;;
      8) printf '\x15echo clean-%d\r' "$i" ;;
    esac
    sleep 0.05
  done
  printf '\r'; sleep 1
  printf 'echo fuzz-done\r'
  sleep 2
  printf 'poweroff\r'
  sleep 3
} | timeout $((duration + 60)) nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
