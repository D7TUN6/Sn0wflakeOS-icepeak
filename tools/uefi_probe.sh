#!/usr/bin/env bash
# Probe the UEFI boot path under OVMF.
#   MODE=iso     boot the dual-entry ISO (default)
#   MODE=bootd   boot the ISO with CD first in the QEMU boot order
#   MODE=disk    boot build/efi.img directly as a USB disk
set -uo pipefail
cd "$(dirname "$0")/.."
: "${OVMF_FD:?run inside nix develop (OVMF_FD is not set)}"

MODE="${MODE:-iso}"
LOG="${LOG:-/tmp/uefi_probe.log}"
rm -f "$LOG"

common=(-m 512M -smp "${SMP:-4}" -display none -serial "file:$LOG" -monitor none -bios "$OVMF_FD")

case "$MODE" in
  disk)
    timeout 70 qemu-system-x86_64 "${common[@]}" \
      -drive file=build/efi.img,format=raw,if=none,id=d0 \
      -device qemu-xhci -device usb-storage,drive=d0
    ;;
  bootd)
    timeout 70 qemu-system-x86_64 "${common[@]}" -boot order=d -cdrom sn0wflake.iso
    ;;
  *)
    timeout 70 qemu-system-x86_64 "${common[@]}" -cdrom sn0wflake.iso
    ;;
esac
echo "probe done: $MODE -> $LOG"
