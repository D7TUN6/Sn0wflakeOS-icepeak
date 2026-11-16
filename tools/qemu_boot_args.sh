#!/usr/bin/env bash
# Shared QEMU boot arguments for the test drivers.
#   UEFI=1 boots through OVMF (the CD must be first in the boot order).
#   DISK_IMG=<path> attaches that image as the primary IDE disk; the kernel
#   uses it for the persistent store. Without it the guest is memory-only.
boot_args=()
if [ "${UEFI:-0}" = "1" ]; then
  : "${OVMF_FD:?run inside nix develop (OVMF_FD is not set)}"
  boot_args=(-bios "$OVMF_FD" -boot order=d)
fi
if [ -n "${DISK_IMG:-}" ]; then
  boot_args+=(-drive "file=$DISK_IMG,format=raw,if=ide,cache=writeback")
fi
