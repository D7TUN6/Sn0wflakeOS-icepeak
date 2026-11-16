#!/usr/bin/env bash
# Boot, feed commands too fast, then attach gdb to the gdbstub after the hang.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh

{
  sleep 9
  printf 'echo one\r'; sleep 1
  printf 'echo two\r'; sleep 1
  printf 'echo three\r'; sleep 1
  printf 'echo four\r'; sleep 40
} | timeout 80 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp 1 -display none \
      -serial stdio -monitor none -s "${boot_args[@]}" > /tmp/gq.log 2>&1 &
QPID=$!
sleep 25
nix develop --command gdb -batch \
  -ex 'set pagination off' \
  -ex 'target remote localhost:1234' \
  -ex 'info registers rip rsp rbp cr3' \
  -ex 'x/16i $rip' \
  -ex 'bt' \
  build/cargo/x86_64-unknown-linux-gnu/release/icepeak > /tmp/gdb.log 2>&1
kill "$QPID" 2>/dev/null
wait "$QPID" 2>/dev/null
echo ATTACHED
