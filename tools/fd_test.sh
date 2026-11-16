#!/usr/bin/env bash
# Kernel file-descriptor checks: streaming reads/writes, seek, append, dup2 and
# the /dev sinks, driven by the `fdtest` user program.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

log=/tmp/fd_test_guest.log
{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send 'fdtest' 2
  send 'echo $?'
  send 'poweroff' 3
} | timeout 120 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -serial stdio -monitor none "${boot_args[@]}" >"$log" 2>&1

rc=$?
ok=$(grep -ac "fdtest: ok" "$log" || true)
bad=$(grep -ac -E "fdtest: .* failed|panic|CPU EXCEPTION" "$log" || true)
if [ "$rc" -ne 0 ] || [ "$ok" -eq 0 ] || [ "$bad" -ne 0 ]; then
  echo "fd_test: failed rc=$rc ok=$ok bad=$bad"
  exit 1
fi
echo "fd_test: ok"
