; Sn0wflakeOS UEFI boot loader.
;
; A hand-written PE32+ EFI application. It reads \sn0wflake.snf from the EFI
; system partition, loads the kernel ELF at its linked address, builds a
; Multiboot 1 information block from the firmware memory map, calls
; ExitBootServices, installs an identity map for the first 4 GiB and enters the
; kernel's 32-bit Multiboot entry point.
;
; No external libraries or crates are involved; everything below is BIOS/UEFI
; protocol calls and our own code.
;
; PE layout: labels are file offsets. The single section maps file offset 0x200
; to RVA 0x1000, so RVA = label + 0xE00. ImageBase = 0, one RWX section, plus a
; dummy .reloc block so firmware will relocate the image (the code itself is
; position independent: RIP-relative only).

BITS 64
default rel

; ---------------------------------------------------------------- PE headers
pe_image:
    dw 0x5A4D                       ; "MZ"
    times 0x3A db 0
    dd 0x40                         ; e_lfanew
    dd 0x00004550                   ; "PE\0\0"
    ; COFF header
    dw 0x8664                       ; machine: x86-64
    dw 1                            ; one section
    dd 0
    dd 0
    dd 0
    dw 0xF0                         ; SizeOfOptionalHeader
    dw 0x0022                       ; executable | large address aware
    ; PE32+ optional header
    dw 0x20B
    db 0
    db 0
    dd raw_end - 0x200             ; SizeOfCode
    dd 0
    dd 0
    dd efi_main + 0xE00             ; AddressOfEntryPoint (RVA)
    dd 0x1000                       ; BaseOfCode
    dq 0                            ; ImageBase
    dd 0x1000                       ; SectionAlignment
    dd 0x200                        ; FileAlignment
    dw 0
    dw 0
    dw 0
    dw 0
    dw 0
    dw 0
    dd 0
    dd 0x20000                      ; SizeOfImage (128 KiB, checked in build.sh)
    dd 0x200                        ; SizeOfHeaders
    dd 0
    dw 10                           ; Subsystem: EFI application
    dw 0
    dq 0x100000
    dq 0x1000
    dq 0
    dq 0
    dd 0
    dd 16
    times 5 dq 0                    ; directories 0..4
    dd reloc_start + 0xE00           ; directory 5: base relocations (RVA)
    dd 12
    times 10 dq 0                   ; directories 6..15
    ; section header
    db ".text", 0, 0, 0
    dd raw_end - 0x200              ; VirtualSize
    dd 0x1000                       ; VirtualAddress
    dd raw_end - 0x200              ; SizeOfRawData
    dd 0x200                        ; PointerToRawData
    dd 0                            ; PointerToRelocations
    dd 0                            ; PointerToLinenumbers
    dw 0                            ; NumberOfRelocations
    dw 0                            ; NumberOfLinenumbers
    dd 0xE0000020                   ; code | execute | read | write
    times 0x200 - ($ - pe_image) db 0

; ------------------------------------------------------------------ EFI entry
efi_main:
    push rbp
    mov rbp, rsp
    push rbx
    push r12
    push r13
    push r14
    push r15
    sub rsp, 0x28                   ; 32-byte shadow space + alignment

    mov [rel image_handle], rcx
    mov [rel system_table], rdx
    mov rax, [rdx + 96]             ; EFI_SYSTEM_TABLE.BootServices
    mov [rel boot_services], rax
    test rax, rax
    jz .err_bs

    call serial_init

    lea rsi, [rel msg_start]
    call serial_puts

    ; ---- select a Graphics Output Protocol mode ----
    ; The framebuffer description is left at 0x9000 for the kernel, the same
    ; block the BIOS loader writes.
    mov dword [0x9000], 0
    lea rcx, [rel guid_gop]
    xor edx, edx
    lea r8, [rel gop_iface]
    mov rax, [rel boot_services]
    call qword [rax + 0x140]        ; LocateProtocol
    test rax, rax
    js .no_gop
    mov rbx, [rel gop_iface]
    mov rax, [rbx + 24]             ; Mode
    mov [rel gop_mode], rax
    mov r13d, [rax + 0]             ; MaxMode
    mov qword [rel gop_best_mode], -1
    mov qword [rel gop_best_score], 0
    xor r12d, r12d
.gop_loop:
    cmp r12d, r13d
    jae .gop_done
    mov qword [rel gop_info_ptr], 0
    mov rcx, rbx
    mov edx, r12d
    lea r8, [rel gop_info_size]
    lea r9, [rel gop_info_ptr]
    call qword [rbx + 0]            ; QueryMode
    test rax, rax
    js .gop_next
    mov r14, [rel gop_info_ptr]
    test r14, r14
    jz .gop_next
    mov eax, [r14 + 12]             ; PixelFormat
    cmp eax, 2                      ; 3 = BltOnly, no linear buffer
    ja .gfree
    mov eax, [r14 + 4]
    mov ecx, [r14 + 8]
    xor edx, edx
    cmp eax, 1024
    jne .g800
    cmp ecx, 768
    jne .g800
    mov edx, 1000
    jmp .gscore
.g800:
    cmp eax, 800
    jne .g640
    cmp ecx, 600
    jne .g640
    mov edx, 800
    jmp .gscore
.g640:
    cmp eax, 640
    jne .gfree
    cmp ecx, 480
    jne .gfree
    mov edx, 600
.gscore:
    cmp edx, [rel gop_best_score]
    jbe .gfree
    mov [rel gop_best_score], edx
    mov [rel gop_best_mode], r12
.gfree:
    mov rcx, r14
    mov rax, [rel boot_services]
    call qword [rax + 0x48]         ; FreePool
.gop_next:
    inc r12d
    jmp .gop_loop
.gop_done:
    mov rax, [rel gop_best_mode]
    cmp rax, -1
    je .no_gop
    mov rbx, [rel gop_iface]
    mov rcx, rbx
    mov edx, eax
    call qword [rbx + 8]            ; SetMode
    test rax, rax
    js .no_gop

    mov rdi, 0x9000
    mov dword [rdi], 0x534E4656     ; "SNFV"
    mov rax, [rel gop_mode]
    mov rcx, [rax + 8]              ; Info
    mov eax, [rcx + 4]
    mov [rdi + 4], eax
    mov eax, [rcx + 8]
    mov [rdi + 8], eax
    mov eax, [rcx + 32]             ; PixelsPerScanLine
    shl eax, 2
    mov [rdi + 12], eax
    mov dword [rdi + 16], 32
    mov dword [rdi + 20], 0
    mov eax, [rcx + 12]             ; PixelFormat
    cmp eax, 0
    je .grgb
    cmp eax, 1
    je .gbgr
    mov eax, [rcx + 16]
    call mask_pos_size
    mov [rdi + 24], dl
    mov [rdi + 25], r8b
    mov eax, [rcx + 20]
    call mask_pos_size
    mov [rdi + 26], dl
    mov [rdi + 27], r8b
    mov eax, [rcx + 24]
    call mask_pos_size
    mov [rdi + 28], dl
    mov [rdi + 29], r8b
    jmp .gmasks
.grgb:
    mov byte [rdi + 24], 0
    mov byte [rdi + 25], 8
    mov byte [rdi + 26], 8
    mov byte [rdi + 27], 8
    mov byte [rdi + 28], 16
    mov byte [rdi + 29], 8
    jmp .gmasks
.gbgr:
    mov byte [rdi + 24], 16
    mov byte [rdi + 25], 8
    mov byte [rdi + 26], 8
    mov byte [rdi + 27], 8
    mov byte [rdi + 28], 0
    mov byte [rdi + 29], 8
.gmasks:
    mov byte [rdi + 30], 0
    mov byte [rdi + 31], 0
    mov rax, [rel gop_mode]
    mov rax, [rax + 24]             ; FrameBufferBase
    mov [rdi + 32], rax
    mov rax, [rel gop_mode]
    mov rax, [rax + 32]             ; FrameBufferSize
    mov [rdi + 40], rax
    lea rsi, [rel msg_video]
    call serial_puts
    jmp .video_done
.no_gop:
    lea rsi, [rel msg_novideo]
    call serial_puts
.video_done:

    ; LoadedImage protocol of this application.
    mov rcx, [rel image_handle]
    lea rdx, [rel guid_loaded_image]
    lea r8, [rel loaded_image]
    mov rax, [rel boot_services]
    call qword [rax + 152]          ; HandleProtocol
    test rax, rax
    js .err_li

    ; SimpleFileSystem on the device this image came from.
    mov rcx, [rel loaded_image]
    mov rcx, [rcx + 24]             ; LoadedImage.DeviceHandle
    lea rdx, [rel guid_simple_fs]
    lea r8, [rel file_system]
    mov rax, [rel boot_services]
    call qword [rax + 152]
    test rax, rax
    js .err_fs

    mov rcx, [rel file_system]
    lea rdx, [rel root_dir]
    call qword [rcx + 8]            ; OpenVolume
    test rax, rax
    js .err_vol

    ; Open \sn0wflake.snf for reading.
    mov rcx, [rel root_dir]
    lea rdx, [rel file_handle]
    lea r8, [rel archive_name]
    mov r9d, 1                      ; EFI_FILE_MODE_READ
    mov qword [rsp + 32], 0
    call qword [rcx + 8]            ; EFI_FILE_PROTOCOL.Open
    test rax, rax
    js .err_open

    ; Reserve the kernel's linked load region at 1 MiB *before* any other
    ; allocation, so the archive buffer cannot land there. The kernel is
    ; linked at 1 MiB; its image (including the static heap) is kept below the
    ; ACPI NVS block at 8 MiB, so a 7 MiB reservation exactly fits.
    mov qword [rel kernel_base], 0x100000
    mov qword [rel kernel_reserved], 1792
    mov rcx, 2                      ; AllocateAddress
    mov rdx, 2                      ; EfiLoaderData
    mov r8, 1792                    ; 7 MiB: exactly the 1..8 MiB region
    lea r9, [rel kernel_base]
    mov rax, [rel boot_services]
    call qword [rax + 40]
    test rax, rax
    jz .kalloc_ok
    push rax
    lea rsi, [rel msg_kstat]
    call serial_puts
    pop rax
    call serial_hex
    lea rsi, [rel msg_nl]
    call serial_puts
    jmp .err_kalloc
.kalloc_ok:

    ; 8 MiB archive buffer.
    xor ecx, ecx                    ; AllocateAnyPages
    mov edx, 2                      ; EfiLoaderData
    mov r8d, 2048
    lea r9, [rel archive_base]
    mov rax, [rel boot_services]
    call qword [rax + 40]
    test rax, rax
    js .err_alloc

    ; Page tables: PML4 + PDPT + four PDs = 6 pages, round to 8.
    xor ecx, ecx
    mov edx, 2
    mov r8d, 8
    lea r9, [rel pt_base]
    mov rax, [rel boot_services]
    call qword [rax + 40]
    test rax, rax
    js .err_alloc

    xor r12, r12
.read_loop:
    mov qword [rel read_size], 0x100000
    mov rcx, [rel file_handle]
    lea rdx, [rel read_size]
    mov r8, [rel archive_base]
    add r8, r12
    call qword [rcx + 32]           ; EFI_FILE_PROTOCOL.Read
    test rax, rax
    js .err_read
    mov rax, [rel read_size]
    test rax, rax
    jz .read_done
    add r12, rax
    cmp r12, 8 * 1024 * 1024
    jb .read_loop
    jmp .err_big
.read_done:
    mov [rel archive_size], r12
    mov rbx, [rel archive_base]
    cmp dword [rbx], 0x534E4631
    jne .err_magic
    mov r13d, [rbx + 8]
    cmp r13d, 64
    jbe .count_ok
    mov r13d, 64
.count_ok:

    ; One 16 KiB block: Multiboot info, module table, scratch.
    xor ecx, ecx
    mov edx, 2
    mov r8d, 4
    lea r9, [rel mb_area]
    mov rax, [rel boot_services]
    call qword [rax + 40]
    test rax, rax
    js .err_alloc
    mov rdi, [rel mb_area]
    mov rcx, 16384 / 8
    xor rax, rax
    rep stosq

    xor r14d, r14d                  ; entry index
    mov qword [rel mod_count], 0
    mov rdi, [rel mb_area]
    lea rsi, [rdi + 0x1000]         ; module table
.entry_loop:
    cmp r14d, r13d
    jae .entries_done
    mov eax, r14d
    imul eax, 44
    lea rdx, [rbx + 12]
    add rdx, rax
    cmp dword [rdx + 32], 0         ; kind 0 = kernel
    jne .module_entry
    mov [rel kernel_entry_ptr], rdx
    jmp .next_entry
.module_entry:
    cmp qword [rel mod_count], 64
    jae .next_entry
    mov eax, [rdx + 36]             ; archive-relative offset
    add rax, rbx
    mov [rsi], eax
    mov ecx, [rdx + 40]             ; size
    add eax, ecx
    mov [rsi + 4], eax
    mov [rsi + 8], edx              ; name lives in the entry itself
    add rsi, 16
    inc qword [rel mod_count]
.next_entry:
    inc r14d
    jmp .entry_loop
.entries_done:

    mov rdx, [rel kernel_entry_ptr]
    test rdx, rdx
    jz .err_nokernel
    mov eax, [rdx + 36]
    mov [rel kernel_off], rax
    mov eax, [rdx + 40]
    mov [rel kernel_size], rax

    ; ---- parse the kernel ELF ----
    mov rbx, [rel archive_base]
    add rbx, [rel kernel_off]
    cmp dword [rbx], 0x464C457F
    jne .err_elf
    cmp byte [rbx + 4], 2
    jne .err_elf
    cmp word [rbx + 0x12], 62
    jne .err_elf
    mov rax, [rbx + 0x18]
    mov [rel kernel_entry], rax
    mov r12, [rbx + 0x20]           ; e_phoff
    movzx r13d, word [rbx + 0x36]   ; e_phentsize
    movzx r14d, word [rbx + 0x38]   ; e_phnum

    mov r15, -1                     ; lowest page-aligned vaddr
    xor r8, r8                      ; highest vaddr+memsz
    xor ecx, ecx
.ph_scan:
    cmp ecx, r14d
    jae .ph_scan_done
    mov rax, rcx
    imul rax, r13
    add rax, r12
    lea rdx, [rbx + rax]
    cmp dword [rdx], 1              ; PT_LOAD
    jne .ph_scan_next
    mov rax, [rdx + 16]             ; p_vaddr
    mov r9, rax
    and r9, ~0xFFF
    cmp r9, r15
    jae .no_min
    mov r15, r9
.no_min:
    add rax, [rdx + 40]             ; p_memsz
    cmp rax, r8
    jbe .ph_scan_next
    mov r8, rax
.ph_scan_next:
    inc ecx
    jmp .ph_scan
.ph_scan_done:
    mov rax, r8
    sub rax, r15
    add rax, 0xFFF
    shr rax, 12
    mov [rel kernel_pages], rax
    ; The kernel is linked at 1 MiB and entered at the absolute address
    ; 0x100000, so its lowest page must match the reservation base.
    cmp r15, [rel kernel_base]
    jne .err_kbase
    cmp rax, [rel kernel_reserved]
    ja .err_ksize

    ; Zero the whole image (covers .bss) before copying file bytes.
    mov rdi, [rel kernel_base]
    mov rcx, [rel kernel_pages]
    shl rcx, 9
    xor rax, rax
    rep stosq

    xor r8d, r8d
.ph_copy:
    cmp r8d, r14d
    jae .ph_copy_done
    mov rax, r8
    imul rax, r13
    add rax, r12
    lea rdx, [rbx + rax]
    cmp dword [rdx], 1
    jne .ph_copy_next
    mov rcx, [rdx + 32]             ; p_filesz
    test rcx, rcx
    jz .ph_copy_next
    mov rdi, [rdx + 16]             ; p_vaddr
    sub rdi, r15
    add rdi, [rel kernel_base]
    mov rsi, rbx
    add rsi, [rdx + 8]              ; p_offset
    rep movsb
.ph_copy_next:
    inc r8d
    jmp .ph_copy
.ph_copy_done:

    ; ---- ACPI RSDP from the UEFI configuration table ----
    call find_rsdp
    mov [rel rsdp_addr], rax

    ; ---- Multiboot 1 info block ----
    mov rdi, [rel mb_area]
    mov dword [rdi], 0x49           ; mem | modules | mmap
    mov dword [rdi + 4], 639
    mov rax, [rel mod_count]
    mov [rdi + 20], eax
    mov rax, [rel mb_area]
    add rax, 0x1000
    mov [rdi + 24], eax
    ; Pass the RSDP through Multiboot's config_table field (bit 8).
    mov rax, [rel rsdp_addr]
    test rax, rax
    jz .no_rsdp
    mov [rdi + 60], eax
    or dword [rdi], 0x100
.no_rsdp:

    ; ---- memory map ----
    mov qword [rel mmap_cap], 0
    lea rcx, [rel mmap_cap]
    xor edx, edx
    lea r8, [rel map_key]
    lea r9, [rel desc_size]
    lea rax, [rel desc_ver]
    mov [rsp + 32], rax
    mov rax, [rel boot_services]
    call qword [rax + 56]           ; GetMemoryMap (size query)
    mov rax, [rel mmap_cap]
    add rax, 4096
    add rax, 0xFFF
    shr rax, 12
    mov [rel mmap_pages], rax
    xor ecx, ecx
    mov edx, 2
    mov r8, rax
    lea r9, [rel mmap_buf]
    mov rax, [rel boot_services]
    call qword [rax + 40]
    test rax, rax
    js .err_mmap
    mov rax, [rel mmap_pages]
    shl rax, 12
    mov [rel mmap_cap], rax

.retry_map:
    mov rax, [rel mmap_cap]
    mov [rel mmap_size], rax
    lea rcx, [rel mmap_size]
    mov rdx, [rel mmap_buf]
    lea r8, [rel map_key]
    lea r9, [rel desc_size]
    lea rax, [rel desc_ver]
    mov [rsp + 32], rax
    mov rax, [rel boot_services]
    call qword [rax + 56]
    test rax, rax
    js .err_map2

    ; Convert EFI descriptors to Multiboot mmap records in place (24 < 40).
    mov rsi, [rel mmap_buf]
    mov rdi, rsi
    mov r8, [rel mmap_size]
    mov r10, [rel desc_size]
    xor r9d, r9d
    xor r11, r11                    ; highest usable end
.conv_loop:
    cmp r8, r10
    jb .conv_done
    mov eax, [rsi]                  ; Type
    mov rdx, [rsi + 8]              ; PhysicalStart
    mov rcx, [rsi + 24]             ; NumberOfPages
    shl rcx, 12
    mov dword [rdi], 20
    mov [rdi + 4], rdx
    mov [rdi + 12], rcx
    cmp eax, 7                      ; EfiConventionalMemory
    jne .not_usable
    mov dword [rdi + 20], 1
    add rdx, rcx
    cmp rdx, r11
    jbe .wrote
    mov r11, rdx
    jmp .wrote
.not_usable:
    mov dword [rdi + 20], 2
.wrote:
    add rdi, 24
    add rsi, r10
    sub r8, r10
    inc r9d
    jmp .conv_loop
.conv_done:
    mov rax, r9
    imul rax, 24
    mov rdi, [rel mb_area]
    mov [rdi + 44], eax
    mov rax, [rel mmap_buf]
    mov [rdi + 48], eax
    ; mem_upper in KiB above 1 MiB, best effort.
    cmp r11, 0x100000
    jbe .no_upper
    mov rax, r11
    sub rax, 0x100000
    shr rax, 10
    mov [rdi + 8], eax
.no_upper:

    mov rcx, [rel image_handle]
    mov rdx, [rel map_key]
    mov rax, [rel boot_services]
    call qword [rax + 232]          ; ExitBootServices
    test rax, rax
    jz .boot_services_off
    mov r11, 0x8000000000000002     ; EFI_INVALID_PARAMETER: map moved
    cmp rax, r11
    je .retry_map
    jmp .err_exit
.boot_services_off:
    cli

    ; ---- our own identity map: first 4 GiB with 2 MiB pages ----
    mov rdi, [rel pt_base]
    mov rcx, 8 * 512
    xor rax, rax
    rep stosq

    mov rdi, [rel pt_base]
    add rdi, 0x2000                 ; page-directory area
    xor ecx, ecx
.map_pd:
    mov rax, rcx
    shl rax, 21
    or rax, 0x83
    mov [rdi + rcx * 8], rax
    inc ecx
    cmp ecx, 2048
    jb .map_pd

    mov rsi, [rel pt_base]
    add rsi, 0x1000                 ; PDPT
    mov rax, [rel pt_base]
    add rax, 0x2000
    or rax, 3
    mov [rsi], rax
    add rax, 0x1000
    mov [rsi + 8], rax
    add rax, 0x1000
    mov [rsi + 16], rax
    add rax, 0x1000
    mov [rsi + 24], rax

    mov rdi, [rel pt_base]
    mov rax, rsi
    or rax, 3
    mov [rdi], rax

    mov rax, cr4
    or rax, (1 << 5) | (1 << 9) | (1 << 10)
    mov cr4, rax
    mov rax, [rel pt_base]
    mov cr3, rax
    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8) | (1 << 11)
    wrmsr
    mov rax, cr0
    mov rcx, 0x80010003             ; paging | write protect | fpu | protected mode
    or rax, rcx
    mov cr0, rax

    lea rax, [rel gdt]
    mov [rel gdt_ptr + 2], rax
    lgdt [rel gdt_ptr]

    lea rsi, [rel msg_enter]
    call serial_puts

    mov ebx, [rel mb_area]          ; Multiboot info (below 4 GiB)
    mov ecx, [rel kernel_entry]     ; ELF e_entry (linear address)
    push qword 0x08                 ; 32-bit code selector
    lea rax, [rel compat32]
    push rax
    retfq

.done:
    add rsp, 0x28
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp
    ret

.err_bs:
    lea rsi, [rel msg_bs]
    jmp .fail
.err_li:
    lea rsi, [rel msg_li]
    jmp .fail
.err_fs:
    lea rsi, [rel msg_fs]
    jmp .fail
.err_vol:
    lea rsi, [rel msg_vol]
    jmp .fail
.err_open:
    lea rsi, [rel msg_open]
    jmp .fail
.err_read:
    lea rsi, [rel msg_read]
    jmp .fail
.err_big:
    lea rsi, [rel msg_big]
    jmp .fail
.err_magic:
    lea rsi, [rel msg_magic]
    jmp .fail
.err_nokernel:
    lea rsi, [rel msg_nokernel]
    jmp .fail
.err_elf:
    lea rsi, [rel msg_elf]
    jmp .fail
.err_alloc:
    lea rsi, [rel msg_alloc]
    jmp .fail
.err_kalloc:
    lea rsi, [rel msg_kalloc]
    jmp .fail
.err_kbase:
    lea rsi, [rel msg_kbase]
    jmp .fail
.err_ksize:
    lea rsi, [rel msg_ksize]
    jmp .fail
.err_mmap:
    lea rsi, [rel msg_mmap]
    jmp .fail
.err_map2:
    lea rsi, [rel msg_map2]
    jmp .fail
.err_exit:
    lea rsi, [rel msg_exit]
    jmp .fail
.fail:
    call serial_puts
    mov eax, 1
    jmp .done

; ---------------------------------------------------------------- 32-bit entry
BITS 32
compat32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov eax, 0x2BADB002
    jmp ecx

BITS 64

; ------------------------------------------------------------------ utilities
serial_init:
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
    mov dx, 0x3FC
    mov al, 0x0B
    out dx, al
    ret

serial_putc:
    mov ah, al
.wait:
    mov dx, 0x3FD
    in al, dx
    test al, 0x20
    jz .wait
    mov al, ah
    mov dx, 0x3F8
    out dx, al
    ret

; Walk EFI_SYSTEM_TABLE.ConfigurationTable for the ACPI RSDP. Returns 0 if the
; firmware exposes no ACPI configuration table.
find_rsdp:
    push rbx
    push r12
    push r13
    push r14
    push r15
    mov rax, [rel system_table]
    test rax, rax
    jz .none
    mov r12, [rax + 112]            ; ConfigurationTable
    mov r13, [rax + 104]            ; NumberOfTableEntries
    test r12, r12
    jz .none
    xor r14, r14
.loop:
    cmp r14, r13
    jae .none
    mov rax, r14
    imul rax, 24                    ; sizeof(EFI_CONFIGURATION_TABLE)
    add rax, r12
    mov rbx, rax
    lea rsi, [rel guid_acpi2]
    mov rdi, rbx
    mov ecx, 16
    repe cmpsb
    je .found
    lea rsi, [rel guid_acpi1]
    mov rdi, rbx
    mov ecx, 16
    repe cmpsb
    je .found
    inc r14
    jmp .loop
.found:
    mov rax, [rbx + 16]             ; VendorTable = RSDP address
    jmp .done
.none:
    xor eax, eax
.done:
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    ret

; eax = colour mask -> dl = lowest set bit, r8b = number of set bits
mask_pos_size:
    xor edx, edx
    xor r8d, r8d
    test eax, eax
    jz .done
.shift:
    test eax, 1
    jnz .count
    shr eax, 1
    inc edx
    jmp .shift
.count:
    test eax, 1
    jz .done
    inc r8d
    shr eax, 1
    jmp .count
.done:
    ret

serial_puts:
.loop:
    mov al, [rsi]
    test al, al
    jz .done
    inc rsi
    call serial_putc
    jmp .loop
.done:
    ret

serial_hex:
    push rbx
    mov rbx, rax
    mov ecx, 16
.hloop:
    rol rbx, 4
    mov al, bl
    and al, 0x0F
    cmp al, 10
    jb .digit
    add al, 'a' - 10
    jmp .emit
.digit:
    add al, '0'
.emit:
    call serial_putc
    dec ecx
    jnz .hloop
    pop rbx
    ret


; -------------------------------------------------------------------- data
align 8
image_handle:      dq 0
system_table:      dq 0
boot_services:     dq 0
loaded_image:      dq 0
file_system:       dq 0
root_dir:          dq 0
file_handle:       dq 0
archive_base:      dq 0
archive_size:      dq 0
read_size:         dq 0
mb_area:           dq 0
kernel_entry_ptr:  dq 0
kernel_off:        dq 0
kernel_size:       dq 0
kernel_entry:      dq 0
kernel_base:       dq 0
kernel_pages:      dq 0
kernel_reserved:   dq 0
mod_count:         dq 0
mmap_buf:          dq 0
mmap_cap:          dq 0
mmap_size:         dq 0
mmap_pages:        dq 0
map_key:           dq 0
desc_size:         dq 0
desc_ver:          dd 0
pt_base:           dq 0
rsdp_addr:         dq 0
gop_iface:         dq 0
gop_mode:          dq 0
gop_info_ptr:      dq 0
gop_info_size:     dq 0
gop_best_mode:     dq 0
gop_best_score:    dq 0

align 8
gdt:
    dq 0
    dq 0x00CF9A000000FFFF           ; 0x08 32-bit code, flat
    dq 0x00CF92000000FFFF           ; 0x10 data, flat
    dq 0x00AF9A000000FFFF           ; 0x18 64-bit code, flat
gdt_end:
gdt_ptr:
    dw gdt_end - gdt - 1
    dq 0

guid_loaded_image:
    db 0xA1, 0x31, 0x1B, 0x5B, 0x62, 0x95, 0xD2, 0x11
    db 0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B
guid_simple_fs:
    db 0x22, 0x5B, 0x4E, 0x96, 0x59, 0x64, 0xD2, 0x11
    db 0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B
guid_acpi2:
    db 0x71, 0xE8, 0x68, 0x88, 0xF1, 0xE4, 0xD3, 0x11
    db 0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81
guid_acpi1:
    db 0x30, 0x2D, 0x9D, 0xEB, 0x88, 0x2D, 0xD3, 0x11
    db 0x9A, 0x16, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D
guid_gop:
    db 0xDE, 0xA9, 0x42, 0x90, 0xDC, 0x23, 0x38, 0x4A
    db 0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A

archive_name:
    dw 0x5C, 's', 'n', '0', 'w', 'f', 'l', 'a', 'k', 'e', '.', 's', 'n', 'f', 0

msg_start:     db "UEFI: icepeak loader", 13, 10, 0
msg_video:     db "UEFI: graphics mode set", 13, 10, 0
msg_novideo:   db "UEFI: no GOP framebuffer, using VGA text", 13, 10, 0
msg_kstat:     db "UEFI: kernel reserve status 0x", 0
msg_nl:        db 13, 10, 0
msg_enter:     db "UEFI: entering kernel", 13, 10, 0
msg_bs:        db "UEFI: no boot services", 13, 10, 0
msg_li:        db "UEFI: LoadedImage protocol failed", 13, 10, 0
msg_fs:        db "UEFI: SimpleFileSystem protocol failed", 13, 10, 0
msg_vol:       db "UEFI: OpenVolume failed", 13, 10, 0
msg_open:      db "UEFI: cannot open \sn0wflake.snf", 13, 10, 0
msg_read:      db "UEFI: archive read failed", 13, 10, 0
msg_big:       db "UEFI: archive too large", 13, 10, 0
msg_magic:     db "UEFI: bad archive magic", 13, 10, 0
msg_nokernel:  db "UEFI: no kernel in archive", 13, 10, 0
msg_elf:       db "UEFI: bad kernel ELF", 13, 10, 0
msg_alloc:     db "UEFI: AllocatePages failed", 13, 10, 0
msg_kalloc:    db "UEFI: kernel allocation failed", 13, 10, 0
msg_kbase:     db "UEFI: kernel link base is not 1 MiB", 13, 10, 0
msg_ksize:     db "UEFI: kernel image exceeds reservation", 13, 10, 0
msg_mmap:      db "UEFI: memory map allocation failed", 13, 10, 0
msg_map2:      db "UEFI: GetMemoryMap failed", 13, 10, 0
msg_exit:      db "UEFI: ExitBootServices failed", 13, 10, 0

; ----------------------------------------------------------- relocations
align 0x200
reloc_start:
    dd 0x1000                       ; page RVA
    dd 12                           ; block size
    dw 0                            ; ABSOLUTE, ignored
    dw 0
reloc_size_marker:

align 0x200
raw_end:
