#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ISO="$ROOT/sn0wflake.iso"

MODE="${1:-gui}"

# Attach a persistent disk when one exists (or when DISK names one). The live
# ISO stays read-only; the log-structured store lives on this image.
disk_args=()
if [ -n "${DISK:-}" ]; then
  [ -f "$DISK" ] || truncate -s 64M "$DISK"
  disk_args=(-drive "file=$DISK,format=raw,if=ide,cache=writeback")
elif [ -f "$ROOT/disk.img" ]; then
  disk_args=(-drive "file=$ROOT/disk.img,format=raw,if=ide,cache=writeback")
fi

case "$MODE" in
  headless)
    exec qemu-system-x86_64 \
      -cdrom "$ISO" -m 512M -smp 4 "${disk_args[@]}" \
      -display none -serial stdio -monitor none \
      -no-reboot -no-shutdown
    ;;
  debug)
    exec qemu-system-x86_64 \
      -cdrom "$ISO" -m 512M -smp 4 "${disk_args[@]}" \
      -serial stdio -monitor none \
      -no-reboot -no-shutdown -s -S
    ;;
  uefi)
    : "${OVMF_FD:?run inside nix develop (OVMF_FD is not set)}"
    exec qemu-system-x86_64 \
      -cdrom "$ISO" -m 512M -smp 4 "${disk_args[@]}" \
      -display none -serial stdio -monitor none \
      -bios "$OVMF_FD" -boot order=d
    ;;
  uefi-gui)
    : "${OVMF_FD:?run inside nix develop (OVMF_FD is not set)}"
    exec qemu-system-x86_64 \
      -cdrom "$ISO" -m 512M -smp 4 "${disk_args[@]}" \
      -serial stdio -monitor none \
      -bios "$OVMF_FD" -boot order=d
    ;;
  *)
    exec qemu-system-x86_64 \
      -cdrom "$ISO" -m 512M -smp 4 "${disk_args[@]}" \
      -serial stdio -monitor none \
      -no-reboot -no-shutdown
    ;;
esac
