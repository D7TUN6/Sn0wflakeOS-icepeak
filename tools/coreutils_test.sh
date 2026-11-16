#!/usr/bin/env bash
# Coreutils + permissions regression run.
set -uo pipefail
cd "$(dirname "$0")/.."
source tools/qemu_boot_args.sh
start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

{
  sleep "$start"
  printf "root\r"; sleep 1; printf "root\r"; sleep 1
  send 'uname -a'
  send 'uname -o'
  send 'id'
  send 'whoami'
  send 'seq 1 5'
  send 'seq -w 8 10'
  send 'printf %s=%d x 42'
  send 'echo -e a\tb'
  send 'basename /a/b/c.txt .txt'
  send 'dirname /a/b/c.txt'
  send 'mkdir -p /t/a/b'
  send 'touch /t/a/b/f'
  send 'echo hello > /t/a/b/f'
  send 'cat /t/a/b/f'
  send 'wc -l /t/a/b/f'
  send 'head -n 1 /t/a/b/f'
  send 'tail -n 1 /t/a/b/f'
  send 'grep -n hello /t/a/b/f'
  send 'cp -r /t/a /t/c'
  send 'ls -l /t/c/b'
  send 'stat /t/a/b/f'
  send 'chmod 600 /t/a/b/f'
  send 'stat /t/a/b/f'
  send 'chmod u+x /t/a/b/f'
  send 'stat /t/a/b/f'
  send 'chown user /t/a/b/f'
  send 'stat /t/a/b/f'
  send 'su user -c whoami' 2
  send 'su user -c id' 2
  send 'du -h /t'
  send 'du -s /t'
  send 'df -h'
  send 'free -h'
  send 'ps -f'
  send 'top -n 1' 2
  send 'chrt -f 10 true' 2
  send 'chrt -p 1'
  send 'rm -rf /t'
  send 'ls /t'
  send 'true; echo $?'
  send 'false; echo $?'
  send 'poweroff' 3
} | timeout 240 nix develop --command qemu-system-x86_64 \
      -cdrom sn0wflake.iso -m 512M -smp "${SMP:-1}" -display none \
      -serial stdio -monitor none "${boot_args[@]}"
