#!/usr/bin/env bash
# Shell syntax compatibility run: quoting, separators, variables.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send "echo 'a b'"
  send 'echo "x y"'
  send 'echo a\ b'
  send 'true; echo $?'
  send 'false; echo $?'
  send 'false && echo no'
  send 'false || echo ok'
  send 'true && echo yes'
  send 'echo hello # comment'
  send 'FOO=bar'
  send 'echo $FOO'
  send 'FOO=42; echo $FOO'
  send 'echo "$FOO x"'
  send 'echo ${FOO}y'
  send 'echo $USER'
  send 'echo "quoted $PWD"'
  send 'mkdir -p /s && cd /s && pwd'
  send 'cd /'
  send 'false || true; echo $?'
  send 'poweroff' 3
} | timeout 120 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
