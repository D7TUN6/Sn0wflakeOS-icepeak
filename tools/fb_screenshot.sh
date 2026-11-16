#!/usr/bin/env bash
# Capture the guest framebuffer through the QEMU monitor.
#   WAIT=30 UEFI=1 SMP=2 ./tools/fb_screenshot.sh /tmp/shot.ppm
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
out="${1:-/tmp/shot.ppm}"
serial_log="${SERIAL_LOG:-/tmp/snf_fb_serial.log}"

{
  sleep "${WAIT:-30}"
  echo "screendump $out"
  sleep 2
  echo quit
} | timeout "${TIMEOUT:-70}" nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial "file:$serial_log" -monitor stdio "${boot_args[@]}" \
      >/tmp/snf_fb_monitor.log 2>&1
echo "screenshot: $out"
