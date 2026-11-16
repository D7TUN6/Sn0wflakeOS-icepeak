#!/usr/bin/env bash
# Login and Nix/FHS layout check. The live CD auto-logs in `user` when the
# login name is left empty; `root` authenticates with the password `root`.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  # Empty user name selects the live-CD account, which has no password.
  printf '\r'; sleep 2
  send 'id'
  send 'whoami'
  send 'echo $HOME'
  send 'echo $USER'
  send 'ls -l /bin/peaksh'
  send 'ls -l /'
  send 'stat /bin'
  send 'stat /bin/peaksh'
  send 'echo hi > /tmp/x; cat /tmp/x'
  send 'poweroff' 3
} | timeout 120 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-2}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
