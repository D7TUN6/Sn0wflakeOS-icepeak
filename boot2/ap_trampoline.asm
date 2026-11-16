; Application-processor startup trampoline.
;
; The BSP copies this blob to physical 0x8000 and starts each AP with a SIPI
; carrying vector 8. The AP begins in real mode at 0x8000:0000 and is handed:
;   0x8F00  u32  kernel CR3
;   0x8F08  u64  stack top
;   0x8F10  u64  logical CPU index
;   0x8F18  u64  address of ap_entry
; The trampoline switches to 32-bit and then long mode, and jumps to ap_entry.

BITS 16
org 0x8000

start:
    cli
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov sp, 0x7C00

    lgdt [gdt_ptr]
    mov eax, cr0
    or al, 1
    mov cr0, eax
    jmp dword 0x08:pm32

BITS 32
pm32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov eax, [0x8F00]
    mov cr3, eax

    ; PAE + SSE state for the Rust entry point.
    mov eax, cr4
    or eax, (1 << 5) | (1 << 9) | (1 << 10)
    mov cr4, eax

    mov eax, cr0
    and eax, ~((1 << 2) | (1 << 3))   ; clear EM, TS
    or eax, (1 << 1)                  ; MP
    mov cr0, eax

    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8)                  ; LME
    wrmsr

    mov eax, cr0
    or eax, (1 << 31) | (1 << 0)      ; PG | PE
    mov cr0, eax
    jmp dword 0x18:lm64

BITS 64
lm64:
    mov ax, 0x20
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    mov rsp, [abs 0x8F08]
    mov rdi, [abs 0x8F10]
    mov rax, [abs 0x8F18]
    and rsp, -16
    sub rsp, 8                 ; mimic a call frame so Rust sees rsp % 16 == 8
    jmp rax

align 8
gdt:
    dq 0
    dq 0x00CF9A000000FFFF        ; 0x08 32-bit code, flat
    dq 0x00CF92000000FFFF        ; 0x10 data, flat
    dq 0x00AF9A000000FFFF        ; 0x18 64-bit code, flat
    dq 0x00CF92000000FFFF        ; 0x20 data, flat
gdt_end:
gdt_ptr:
    dw gdt_end - gdt - 1
    dq gdt
