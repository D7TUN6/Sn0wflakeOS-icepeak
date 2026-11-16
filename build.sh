#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROFILE="${1:-release}"
case "$PROFILE" in
    dev|release) ;;
    *)
        echo "usage: $0 [dev|release]" >&2
        exit 2
        ;;
esac
if [ "$PROFILE" = "release" ]; then
    CARGO_FLAGS=(--release)
    OUTDIR="release"
else
    CARGO_FLAGS=()
    OUTDIR="debug"
fi

BUILD="$ROOT/build"
KERNEL="$ROOT/kernel"
USER_BINS="hello faulttest peaksh sync ls cat echo pwd mkdir rmdir rm touch cp mv ps uname sleep clear wc beep poweroff fptest true false whoami id basename dirname seq printf head tail grep chmod chown stat du df free top chrt su cpupower login init libc-test fdtest"

export CARGO_TARGET_DIR="$BUILD/cargo"

echo "[build] profile=$PROFILE"
echo "[1/7] assembling boot stubs"
mkdir -p "$BUILD"
nasm -f elf64 -o "$BUILD/boot.o" "$ROOT/boot/boot.asm"
nasm -f elf64 -o "$BUILD/interrupts.o" "$ROOT/boot/interrupts.asm"
nasm -f bin -o "$BUILD/stage1.bin" "$ROOT/boot2/stage1.asm"
nasm -f bin -o "$BUILD/stage2.bin" "$ROOT/boot2/stage2.asm"
nasm -f bin -o "$BUILD/ap_trampoline.bin" "$ROOT/boot2/ap_trampoline.asm"
nasm -f bin -o "$BUILD/BOOTX64.EFI" "$ROOT/boot2/uefi.asm"

echo "[2/7] building user programs ($PROFILE)"
mkdir -p "$BUILD/cargo-user"
(
    cd "$ROOT/user"
    CARGO_TARGET_DIR="$BUILD/cargo-user" cargo build "${CARGO_FLAGS[@]}" --bins
)
USERBIN="$BUILD/cargo-user/x86_64-unknown-linux-gnu/$OUTDIR"

# The debug sections dominate the ELF size and would exhaust the kernel heap
# when every binary is loaded into ramfs. The loader only reads PT_LOAD
# segments, so a stripped copy is fully usable in both profiles; the cargo
# artifacts keep their debug information.
STRIPBIN="$BUILD/userbin"
mkdir -p "$STRIPBIN"
for bin in $USER_BINS; do
    if [ -f "$USERBIN/$bin" ]; then
        strip -o "$STRIPBIN/$bin" "$USERBIN/$bin"
    fi
done

echo "[2b/7] building C libc and libc-test"
CC="${CC:-cc}"
LIBC="$ROOT/libc"
"$CC" -c -ffreestanding -fno-pic -mcmodel=large -fno-stack-protector -fno-builtin -nostdlib \
    -o "$BUILD/crt0.o" "$LIBC/crt0.S"
"$CC" -c -ffreestanding -fno-pic -mcmodel=large -fno-stack-protector -fno-builtin -nostdlib \
    -I"$LIBC/include" -o "$BUILD/libc.o" "$LIBC/libc.c"
ar rcs "$BUILD/libsnf.a" "$BUILD/libc.o"
"$CC" -nostdlib -static -no-pie -fno-pic -mcmodel=large -fno-stack-protector -fno-builtin \
    -ffreestanding -I"$LIBC/include" \
    -Wl,-T,"$ROOT/user/linker.ld" -Wl,--build-id=none -Wl,-z,norelro \
    -o "$STRIPBIN/libc-test" "$LIBC/test/libc-test.c" \
    "$BUILD/crt0.o" "$BUILD/libsnf.a" -lgcc
strip "$STRIPBIN/libc-test"

USERBIN="$STRIPBIN"

echo "[3/7] compiling kernel ($PROFILE)"
(
    cd "$KERNEL"
    cargo rustc "${CARGO_FLAGS[@]}" -- \
        -C link-arg="$BUILD/boot.o" \
        -C link-arg="$BUILD/interrupts.o" \
        -C link-arg="-Wl,-T,$KERNEL/linker.ld" \
        -C link-arg="-Wl,--build-id=none" \
        -C link-arg="-Wl,-z,norelro" \
        -C link-arg="-nostartfiles" \
        -C link-arg="-nostdlib" \
        -C link-arg="-static" \
        -C link-arg="-no-pie"
)

KELF="$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/$OUTDIR/icepeak"

echo "[4/7] building boot-image packer"
rustc -O "$ROOT/tools/mkboot.rs" -o "$BUILD/mkboot"

echo "[5/7] packing boot image"
: > "$BUILD/modules.list"
for bin in $USER_BINS; do
    if [ -f "$USERBIN/$bin" ]; then
        echo "bin/$bin=$USERBIN/$bin" >> "$BUILD/modules.list"
    fi
done
"$BUILD/mkboot" \
    --stage1 "$BUILD/stage1.bin" \
    --stage2 "$BUILD/stage2.bin" \
    --kernel "$KELF" \
    --manifest "$BUILD/modules.list" \
    --out "$BUILD/boot.bin" \
    --archive-out "$BUILD/archive.bin" \
    --shift 11

echo "[6/7] building El Torito ISO"
rm -rf "$BUILD/iso"
mkdir -p "$BUILD/iso"
cp "$BUILD/boot.bin" "$BUILD/iso/boot.bin"

# EFI system partition image: the UEFI loader plus the boot archive.
EFI_IMG="$BUILD/efi.img"
rm -f "$EFI_IMG"
dd if=/dev/zero of="$EFI_IMG" bs=1M count=16 status=none
mformat -i "$EFI_IMG" ::
mmd -i "$EFI_IMG" ::/EFI ::/EFI/BOOT
mcopy -i "$EFI_IMG" "$BUILD/BOOTX64.EFI" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$EFI_IMG" "$BUILD/archive.bin" ::/sn0wflake.snf
cp "$EFI_IMG" "$BUILD/iso/efi.img"

xorriso -as mkisofs \
    -o "$ROOT/sn0wflake.iso" \
    -V SNFOS \
    -b boot.bin -no-emul-boot -boot-load-size 4 -boot-info-table \
    -eltorito-alt-boot -e efi.img -no-emul-boot \
    -R -J "$BUILD/iso" >/dev/null

echo "[7/7] done: $ROOT/sn0wflake.iso ($PROFILE)"
