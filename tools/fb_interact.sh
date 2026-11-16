#!/usr/bin/env bash
# Boot, type a command sequence, and capture the framebuffer mid-session.
#   ./tools/fb_interact.sh 'ls -l /bin' /tmp/shot.ppm
#   WAIT=26 ./tools/fb_interact.sh 'top' /tmp/top.ppm
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh

cmd="${1:-ls -l /bin}"
out="${2:-/tmp/shot_int.ppm}"
port="${PORT:-4455}"
wait_at="${WAIT:-22}"
login="${LOGIN:-root}"
start="${start:-9}"

{
  sleep "$start"
  printf '%s\r' "$login"; sleep 1
  if [ "$login" = "root" ]; then printf 'root\r'; sleep 1; fi
  printf '%s\r' "$cmd"
  sleep 120
} | timeout "${TIMEOUT:-60}" nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -no-shutdown -serial stdio \
      -monitor "tcp:127.0.0.1:$port,server,nowait" "${boot_args[@]}" \
      >/tmp/snf_fb_interact.log 2>&1 &

sleep "$wait_at"
exec 3<>"/dev/tcp/127.0.0.1/$port"
printf 'screendump %s\n' "$out" >&3
sleep 2
printf 'quit\n' >&3
sleep 1
echo "screenshot: $out"
