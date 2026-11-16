#!/usr/bin/env bash
# Run the regression suite sequentially and summarise failures.
#   SMP=4 ./tools/suite.sh [test ...]
set -uo pipefail
cd "$(dirname "$0")/.."

# The UEFI tests need OVMF_FD, which only exists inside the dev shell.
if [ "${UEFI:-0}" = "1" ] && [ -z "${OVMF_FD:-}" ]; then
  exec nix develop --command env UEFI=1 SMP="${SMP:-4}" "$0" "$@"
fi

tests=("$@")
if [ ${#tests[@]} -eq 0 ]; then
  tests=(coreutils_test shell_test pipe_test login_test libc_test fd_test proc_test tickless_test stress input_probe mash persist_test)
fi

fail=0
for t in "${tests[@]}"; do
  log="/tmp/suite_$t.log"
  SMP="${SMP:-4}" "./tools/$t.sh" >"$log" 2>&1
  rc=$?
  bad=$(grep -a -c -i -E "exception|panic|unhandled interrupt|command not found|killed:" "$log" || true)
  ok=$(grep -a -c "Shutting down" "$log" || true)
  # fd_test writes the guest transcript to its own file and reports a single
  # verdict line, so it carries its own pass criterion.
  if [ "$t" = "fd_test" ]; then
    ok=$(grep -a -c "fd_test: ok" "$log" || true)
  fi
  # persist_test boots four times and reports one verdict; the individual boot
  # logs live in /tmp/persist*.log.
  if [ "$t" = "persist_test" ]; then
    ok=$(grep -a -c "persist_test: ok" "$log" || true)
  fi
  if [ "$rc" -ne 0 ] || [ "$bad" -ne 0 ] || [ "$ok" -eq 0 ]; then
    echo "FAIL $t rc=$rc bad=$bad shutdown=$ok"
    fail=1
  else
    echo "ok   $t"
  fi
done
exit "$fail"
