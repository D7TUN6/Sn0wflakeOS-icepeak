#!/usr/bin/env bash
# Focused input-integrity probe: type commands back to back and let the kernel
# trace the bytes it receives. Used while debugging dropped serial input.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh

{
  sleep 9
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  printf 'mkdir /s\r'; sleep 0.6
  printf 'echo x > /s/a\r'; sleep 0.6
  for _ in $(seq 1 8); do
    printf 'cp /s/a /s/b\r'; sleep 0.35
    printf 'mv /s/b /s/c\r'; sleep 0.35
    printf 'rm /s/c\r'; sleep 0.35
  done
  printf 'poweroff\r'
  sleep 3
} | timeout 90 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
