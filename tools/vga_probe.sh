#!/usr/bin/env bash
# Drive the emulated PS/2 keyboard and capture the VGA text screen.
#   KEYS="l s spc slash b i n ret" ./tools/vga_probe.sh
# Uses the QEMU monitor over TCP; results land in $SHOT (default /tmp/shot.ppm).
set -uo pipefail
cd "$(dirname "$0")/.."

: "${OVMF_FD:=}"
SHOT="${SHOT:-/tmp/shot.ppm}"
SERIAL_LOG="${SERIAL_LOG:-/tmp/vga_serial.log}"
PORT="${PORT:-4455}"
KEYS="${KEYS:-l s spc slash b i n ret}"
WAIT="${WAIT:-10}"
EXTRA_KEYS="${EXTRA_KEYS:-}"

rm -f "$SHOT" "$SERIAL_LOG"
qemu-system-x86_64 \
  -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" \
  -display none -serial "file:$SERIAL_LOG" \
  -monitor "tcp:127.0.0.1:$PORT,server,nowait" &
QPID=$!
sleep "$WAIT"

exec 3<>"/dev/tcp/127.0.0.1/$PORT" || { kill "$QPID"; exit 1; }
for k in $KEYS; do
  printf 'sendkey %s\n' "$k" >&3
  sleep 0.12
done
sleep 1
printf 'screendump %s\n' "$SHOT" >&3
sleep 1

if [ -n "$EXTRA_KEYS" ]; then
  for k in $EXTRA_KEYS; do
    printf 'sendkey %s\n' "$k" >&3
    sleep 0.12
  done
  sleep 1
  printf 'screendump %s\n' "${SHOT%.ppm}-2.ppm" >&3
  sleep 1
fi

exec 3<&-
kill "$QPID" 2>/dev/null
wait "$QPID" 2>/dev/null
echo "vga probe done: $SHOT"
