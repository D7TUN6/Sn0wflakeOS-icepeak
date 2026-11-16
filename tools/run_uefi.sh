#!/usr/bin/env bash
# Boot the ISO through the UEFI path under OVMF and drive a short session.
set -uo pipefail
cd "$(dirname "$0")/.."

: "${OVMF_FD:?run inside nix develop (OVMF_FD is not set)}"

{
  sleep 12
  printf 'uname\r'
  sleep 1
  printf 'ls /bin\r'
  sleep 1
  printf 'ps\r'
  sleep 2
  printf 'fptest\r'
  sleep 2
  printf 'poweroff\r'
  sleep 3
} | timeout 90 qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
      -serial stdio -monitor none -bios "$OVMF_FD" -boot order=d
