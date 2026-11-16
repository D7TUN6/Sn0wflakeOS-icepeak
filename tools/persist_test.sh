#!/usr/bin/env bash
# Persistence: write files on one boot, read them back on the next.
#
# The image is created fresh here so the run is independent of any disk left
# over from interactive use. The kernel only mounts the store when a primary
# IDE disk is present; without one it stays memory-only.
set -uo pipefail
cd "$(dirname "$0")/.."

IMG="${IMG:-/tmp/sn0wflake-persist.img}"
truncate -s 64M "$IMG"
export DISK_IMG="$IMG"
source tools/qemu_boot_args.sh

start=9
[ "${UEFI:-0}" = "1" ] && start=14

send() { printf '%s\r' "$1"; sleep "${2:-1}"; }

boot() {
  local log="$1"
  shift
  {
    sleep "$start"
    printf "root\r"; sleep 1; printf "root\r"; sleep 1
    for cmd in "$@"; do
      send "$cmd" 2
    done
    send 'poweroff' 3
  } | timeout 120 nix develop --command qemu-system-x86_64 \
        -cdrom sn0wflake.iso -m 512M -smp "${SMP:-4}" -display none \
        -serial stdio -monitor none "${boot_args[@]}" >"$log" 2>&1
  return $?
}

fail=0
check() {
  local log="$1" needle="$2" label="$3"
  if ! grep -aq -- "$needle" "$log"; then
    echo "persist_test: missing $label ($needle)"
    fail=1
  fi
}

# First boot: create a directory tree, a file and a restrictive mode.
boot /tmp/persist1.log \
  'mkdir -p /home/user/docs' \
  'echo hello > /home/user/docs/a.txt' \
  'chmod 600 /home/user/docs/a.txt' \
  'sync' \
  'echo first-done'
check /tmp/persist1.log 'first-done' 'first boot completion'
check /tmp/persist1.log 'persist: ' 'store mount'

# Second boot: the file, its contents and its mode must survive.
boot /tmp/persist2.log \
  'cat /home/user/docs/a.txt' \
  'ls -l /home/user/docs' \
  'echo second >> /home/user/docs/a.txt' \
  'sync' \
  'echo second-done'
check /tmp/persist2.log 'hello' 'file contents after reboot'
check /tmp/persist2.log '-rw-------' 'file mode after reboot'
check /tmp/persist2.log 'second-done' 'second boot completion'

# Third boot: both appended lines are there; delete the file.
boot /tmp/persist3.log \
  'cat /home/user/docs/a.txt' \
  'rm -f /home/user/docs/a.txt' \
  'sync' \
  'echo third-done'
check /tmp/persist3.log 'second' 'appended line after reboot'
check /tmp/persist3.log 'third-done' 'third boot completion'

# Fourth boot: the delete stuck, and the read-only store still resolves.
boot /tmp/persist4.log \
  'ls -a /home/user/docs' \
  'ls /bin | head -n 2' \
  'echo fourth-done'
check /tmp/persist4.log 'fourth-done' 'fourth boot completion'
if grep -aq 'a.txt' <(sed -n '/ls -a \/home\/user\/docs/,/fourth-done/p' /tmp/persist4.log); then
  echo "persist_test: deleted file came back"
  fail=1
fi
check /tmp/persist4.log 'hello' 'store binaries after reboot'

for i in 1 2 3 4; do
  bad=$(grep -ac -i -E 'exception|panic|unhandled interrupt' "/tmp/persist$i.log" || true)
  if [ "$bad" -ne 0 ]; then
    echo "persist_test: boot $i had $bad kernel faults"
    fail=1
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "persist_test: failed"
  exit 1
fi
echo "persist_test: ok"
