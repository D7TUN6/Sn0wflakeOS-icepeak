#!/usr/bin/env bash
# Build both profiles and run the regression suite through both firmware paths.
#   SMP=4 ./tools/matrix.sh
set -uo pipefail
cd "$(dirname "$0")/.."

# OVMF_FD only exists inside the dev shell; re-exec there once.
if [ -z "${OVMF_FD:-}" ]; then
  exec nix develop --command env SMP="${SMP:-4}" "$0" "$@"
fi

fail=0
for profile in dev release; do
  echo "=== build $profile ==="
  ./build.sh "$profile" >/tmp/matrix_build_$profile.log 2>&1 || {
    echo "build $profile FAILED"
    tail -20 /tmp/matrix_build_$profile.log
    exit 1
  }
  for firmware in bios uefi; do
    uefi=0
    [ "$firmware" = "uefi" ] && uefi=1
    echo "=== $profile $firmware suite ==="
    UEFI="$uefi" SMP="${SMP:-4}" ./tools/suite.sh || fail=1
    if [ "$uefi" = "0" ]; then
      UEFI=0 SMP="${SMP:-4}" ./tools/fault_test.sh >/tmp/matrix_fault.log 2>&1
      rc=$?
      killed=$(grep -ac "killed:" /tmp/matrix_fault.log || true)
      if [ "$rc" -ne 0 ] || [ "$killed" -ne 2 ]; then
        echo "FAIL fault_test rc=$rc killed=$killed"
        fail=1
      else
        echo "ok   fault_test"
      fi
    fi
  done
done
exit "$fail"
