; icepeak legacy BIOS boot entry.
; GRUB loads us through Multiboot 1 in 32-bit protected mode, paging off.
; We build our own page tables, enable long mode, and jump to Rust.

BITS 32

MB_MAGIC  equ 0x1BADB002
MB_FLAGS  equ 0x00000003          ; page-align modules + provide memory map
MB_CHECK  equ -(MB_MAGIC + MB_FLAGS)

section .multiboot
align 4
    dd MB_MAGIC
    dd MB_FLAGS
    dd MB_CHECK

section .bss
align 4096
pml4:       resb 4096
pdpt:       resb 4096
pd:         resb 4096 * 4
stack_bottom: resb 65536
stack_top:

mb_magic:   resd 1
mb_info:    resd 1

section .text
global _start
extern kmain

_start:
    cli
    cld

    ; Preserve Multiboot registers.
    mov [mb_magic], eax
    mov [mb_info], ebx

    ; ---- identity map the first 4 GiB with 2 MiB pages ----
    mov edi, pd
    xor ecx, ecx
.map_pd:
    mov eax, ecx
    shl eax, 21                  ; ecx * 2 MiB
    or  eax, 0x83                ; present | writable | huge
    mov [edi + ecx*8], eax
    mov dword [edi + ecx*8 + 4], 0
    inc ecx
    cmp ecx, 2048
    jne .map_pd

    mov edi, pdpt
    mov eax, pd
    or  eax, 0x03
    mov [edi], eax
    mov dword [edi+4], 0
    add eax, 4096
    mov [edi+8], eax
    mov dword [edi+12], 0
    add eax, 4096
    mov [edi+16], eax
    mov dword [edi+20], 0
    add eax, 4096
    mov [edi+24], eax
    mov dword [edi+28], 0

    mov edi, pml4
    mov eax, pdpt
    or  eax, 0x03
    mov [edi], eax
    mov dword [edi+4], 0

    ; ---- enable PAE + long mode ----
    mov eax, cr4
    or  eax, (1 << 5) | (1 << 9) | (1 << 10)   ; PAE | OSFXSR | OSXMMEXCPT
    mov cr4, eax

    mov eax, pml4
    mov cr3, eax

    mov ecx, 0xC0000080
    rdmsr
    or  eax, (1 << 8) | (1 << 11)              ; LME | NXE
    wrmsr

    mov eax, cr0
    and eax, ~(1 << 2)                          ; clear EM
    or  eax, (1 << 1) | (1 << 16) | (1 << 31)   ; MP | WP | PG
    mov cr0, eax

    lgdt [gdt64_ptr]

    jmp 0x08:long_mode

BITS 64
long_mode:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    mov rsp, stack_top
    xor rbp, rbp

    mov edi, [abs mb_magic]        ; zero-extended into rdi
    mov esi, [abs mb_info]
    call kmain

.halt:
    cli
    hlt
    jmp .halt

align 8
gdt64:
    dq 0x0000000000000000      ; 0x00 null
    dq 0x00AF9A000000FFFF      ; 0x08 kernel code, long
    dq 0x00CF92000000FFFF      ; 0x10 kernel data
    dq 0x00CFF2000000FFFF      ; 0x18 user data
    dq 0x00AFFA000000FFFF      ; 0x20 user code
gdt64_end:
gdt64_ptr:
    dw gdt64_end - gdt64 - 1
    dq gdt64
