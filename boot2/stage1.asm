; Stage 1: El Torito / raw boot stub.
; Loaded at 0x0000:0x7C00 with DL = boot drive. Reads the stage-2 image from
; the same boot image and jumps to it.
;
; Bytes 8..63 are reserved for the ISO boot-info table patched by xorriso
; (PVD LBA, boot-file LBA, length, checksum). The descriptor lives at 0x300
; and is filled in by tools/mkboot.rs.

BITS 16
org 0x7C00

    jmp short s1_start
    times (8 - ($ - $$)) db 0

boot_info:
    times 56 db 0

s1_start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00
    cld
    mov [drive], dl

    ; COM1 at 115200 8N1 for early diagnostics.
    mov dx, 0x3F9
    xor al, al
    out dx, al
    mov dx, 0x3FB
    mov al, 0x80
    out dx, al
    mov dx, 0x3F8
    mov al, 1
    out dx, al
    mov dx, 0x3F9
    xor al, al
    out dx, al
    mov dx, 0x3FB
    mov al, 3
    out dx, al
    mov dx, 0x3FA
    mov al, 0xC7
    out dx, al
    mov dx, 0x3F9
    xor al, al
    out dx, al

    cmp dword [0x7F00], 0x534E4631
    jne fail
    mov eax, [0x7F04]
    mov [shift], eax
    mov ecx, eax
    mov eax, 1
    shl eax, cl
    mov [ssize], eax

    xor ah, ah
    mov dl, [drive]
    int 0x13

    ; First sector of the boot image, plus the stage-2 offset.
    mov eax, [0x7C0C]
    add eax, [0x7F08]
    mov [lba], eax
    mov eax, [0x7F0C]
    mov [remain], eax
    mov ax, 0x1000
    mov es, ax
    xor bx, bx

.read:
    cmp dword [remain], 0
    je .go
    mov eax, 0x8000
    xor edx, edx
    div dword [ssize]
    cmp eax, [remain]
    jbe .chunk
    mov eax, [remain]
.chunk:
    movzx ecx, ax
    mov [dap_count], cx
    mov [dap_off], bx
    mov [dap_seg], es
    mov eax, [lba]
    mov [dap_lba], eax
    mov dword [dap_lba + 4], 0
    mov si, dap
    mov dl, [drive]
    mov ah, 0x42
    push ds
    push es
    int 0x13
    pop es
    pop ds
    jc fail

    movzx ecx, word [dap_count]
    add [lba], ecx
    sub [remain], ecx
    mov eax, ecx
    mul dword [ssize]
    add bx, ax
    jnc .read
    mov dx, es
    add dx, 0x1000
    mov es, dx
    jmp .read

.go:
    mov dl, [drive]
    jmp 0x1000:0

fail:
    mov al, 'F'
    call putc
    cli
    hlt
    jmp fail

putc:
    push ax
    push dx
    mov dx, 0x3FD
.wait:
    in al, dx
    test al, 0x20
    jz .wait
    pop dx
    pop ax
    mov dx, 0x3F8
    out dx, al
    ret

dap:
    db 0x10, 0
dap_count: dw 0
dap_off:   dw 0
dap_seg:   dw 0
dap_lba:   dq 0
drive:     db 0
shift:     dd 0
ssize:     dd 0
lba:       dd 0
remain:    dd 0

    times (0x300 - ($ - $$)) db 0
desc:
    dd 0x534E4631          ; magic
    dd 0                   ; sector shift
    dd 0                   ; stage-2 offset (sectors)
    dd 0                   ; stage-2 count (sectors)
    dd 0                   ; archive offset (sectors)
    dd 0                   ; archive size (bytes)

    times (0x800 - ($ - $$)) db 0
