; Stage 2: loads the kernel ELF and the user modules from the boot archive,
; builds a Multiboot 1 information block (mmap + modules) and enters the
; kernel's 32-bit Multiboot entry point in protected mode.
;
; Loaded at 0x1000:0000, so with DS=0x1000 label values are 16-bit offsets.
; Scratch layout (linear addresses, all below 1 MiB):
;   0x1C000  Multiboot info       0x1C400  module table
;   0x1C800  module name strings  0x1D000  raw E820 entries
;   0x1D800  Multiboot mmap       0x1F000  ELF header / phdr buffer
;   0x20000  32 KiB staging buffer (filled by BIOS, copied in protected mode)

BITS 16
org 0

start:
    cli
    cld
    mov ax, 0x1000
    mov ds, ax
    mov es, ax
    xor ax, ax
    mov ss, ax
    mov sp, 0x7C00

    mov [drive], dl

    call vbe_init

    ; Pull the descriptor fields out of the loaded stage-1 image.
    xor ax, ax
    mov es, ax
    mov eax, [es:0x7F04]
    mov [shift], eax
    mov ecx, eax
    mov eax, 1
    shl eax, cl
    mov [ssize], eax
    mov eax, [es:0x7F10]
    mov [arc_off], eax
    mov eax, [es:0x7F14]
    mov [arc_size], eax
    mov eax, [es:0x7C0C]
    mov [base_lba], eax
    mov ax, 0x1000
    mov es, ax

    mov dword [mod_next], 0x04000000
    mov dword [mod_count], 0
    mov dword [entry_point], 0

    call serial_init

    ; Read the first 4 KiB of the archive: magic, count and entry table.
    mov ebx, 4096
    mov ecx, [shift]
    shr ebx, cl
    mov eax, [arc_off]
    mov ecx, ebx
    mov bx, 0xE000
    call read_abs
    jc fail

    cmp dword [0xE000], 0x534E4631
    jne fail
    mov eax, [0xE008]
    cmp eax, 64
    jbe .ok_count
    mov eax, 64
.ok_count:
    mov [ent_count], eax
    mov dword [ent_idx], 0

.entry_loop:
    mov eax, [ent_idx]
    cmp eax, [ent_count]
    jae .entries_done
    imul eax, 44
    add eax, 0xE00C
    mov [ep], eax
    mov esi, [ep]

    cmp dword [esi + 32], 0
    jne .module

    mov eax, [esi + 36]          ; archive-relative byte offset
    mov ecx, [shift]
    shr eax, cl
    add eax, [arc_off]
    mov ebx, [esi + 40]
    call load_elf
    jc fail
    jmp .next

.module:
    mov eax, [esi + 36]
    mov ecx, [shift]
    shr eax, cl
    add eax, [arc_off]
    mov [tmp_abs], eax
    mov ebx, [esi + 40]
    mov [tmp_size], ebx
    mov edi, [mod_next]
    xor edx, edx
    call load_file_range
    jc .mod_fail

    mov ecx, [mod_count]
    cmp ecx, 64
    jae .skip_record
    imul ecx, 16
    add ecx, 0xC400
    mov eax, [mod_next]
    mov [ecx], eax
    add eax, [tmp_size]
    mov [ecx + 4], eax
    mov edx, [mod_count]
    imul edx, 32
    add edx, 0xC800
    lea eax, [edx + 0x10000]
    mov [ecx + 8], eax

    push ds
    push es
    mov ax, 0x1000
    mov es, ax
    mov si, [ep]
    mov di, dx
    mov cx, 32
    rep movsb
    pop es
    pop ds
    mov byte [edx + 31], 0
    inc dword [mod_count]

.skip_record:
    mov eax, [tmp_size]
    add eax, 0xFFF
    and eax, 0xFFFFF000
    add [mod_next], eax
    jmp .next

.mod_fail:
    mov al, 'M'
    call putc
    jmp fail

.next:
    inc dword [ent_idx]
    jmp .entry_loop

.entries_done:
    call build_mb
    jmp enter_kernel

; ---------------------------------------------------------------- ELF loader
; EAX = first sector (boot-image relative), EBX = file size.
load_elf:
    pushad
    mov [elf_sector], eax

    ; Read the ELF header.
    mov edx, 0
    mov edi, 0x1F000
    mov ebx, 4096
    mov eax, [elf_sector]
    call load_file_range
    jc .bad

    cmp dword [0xF000], 0x464C457F
    jne .bad
    cmp byte [0xF004], 2
    jne .bad
    cmp word [0xF012], 62
    jne .bad

    mov eax, [0xF018]
    mov [entry_point], eax
    mov eax, [0xF020]
    mov [ph_off], eax
    movzx eax, word [0xF036]
    mov [ph_ent], eax
    movzx eax, word [0xF038]
    mov [ph_num], eax

    ; Read the program headers on top of the header buffer.
    mov eax, [ph_ent]
    imul eax, [ph_num]
    mov ebx, eax
    mov edx, [ph_off]
    mov edi, 0x1F000
    mov eax, [elf_sector]
    call load_file_range
    jc .bad

    mov dword [ph_idx], 0
.ph_loop:
    mov eax, [ph_idx]
    cmp eax, [ph_num]
    jae .done
    imul eax, [ph_ent]
    add eax, 0xF000
    mov esi, eax

    cmp dword [esi], 1           ; PT_LOAD
    jne .ph_next

    mov eax, [esi + 8]
    mov [seg_off], eax
    mov eax, [esi + 24]
    mov [seg_dest], eax
    mov eax, [esi + 32]
    mov [seg_filesz], eax
    mov eax, [esi + 40]
    mov [seg_memsz], eax

    cmp dword [seg_filesz], 0
    je .ph_bss
    mov eax, [elf_sector]
    mov edx, [seg_off]
    mov edi, [seg_dest]
    mov ebx, [seg_filesz]
    call load_file_range
    jc .bad

.ph_bss:
    mov eax, [seg_memsz]
    sub eax, [seg_filesz]
    jbe .ph_next
    mov ecx, eax
    mov edi, [seg_dest]
    add edi, [seg_filesz]
    call pm_memset

.ph_next:
    inc dword [ph_idx]
    jmp .ph_loop

.done:
    popad
    clc
    ret
.bad:
    popad
    stc
    ret

; --------------------------------------------------------- byte-range loader
; EAX = first sector (boot-image relative), EDX = offset within file,
; EDI = physical destination, EBX = length.
load_file_range:
    pushad
    mov ecx, [shift]
    shl eax, cl
    add eax, edx                 ; absolute byte position in boot image
    mov edx, eax
    shr eax, cl                  ; sector index
    mov [cur_sector], eax
    mov eax, [ssize]
    dec eax
    and edx, eax
    mov [cur_skip], edx
    mov [remaining], ebx
    mov [dest_ptr], edi

.loop:
    cmp dword [remaining], 0
    je .done

    mov ecx, [shift]
    mov eax, 0x8000
    shr eax, cl
    mov [chunk_sectors], eax
    shl eax, cl
    mov [chunk_bytes], eax

    mov eax, [cur_sector]
    mov ecx, [chunk_sectors]
    xor bx, bx
    mov dx, 0x2000
    mov es, dx
    call read_abs
    jc .bad

    mov eax, [chunk_bytes]
    sub eax, [cur_skip]
    cmp eax, [remaining]
    jbe .have
    mov eax, [remaining]
.have:
    mov [copied], eax
    mov esi, [cur_skip]
    add esi, 0x20000
    mov edi, [dest_ptr]
    mov ecx, eax
    call pm_copy

    mov eax, [copied]
    add [dest_ptr], eax
    sub [remaining], eax
    mov dword [cur_skip], 0
    mov eax, [chunk_sectors]
    add [cur_sector], eax
    jmp .loop

.done:
    popad
    clc
    ret
.bad:
    popad
    stc
    ret

; ---------------------------------------------------------------- BIOS reads
; EAX = LBA relative to the boot image, ECX = sector count, ES:BX = buffer.
read_abs:
    pushad
    push ds
    push es
    mov [dap_count], cx
    mov [dap_off], bx
    mov [dap_seg], es
    mov edx, [base_lba]
    add eax, edx
    mov [dap_lba], eax
    mov dword [dap_lba + 4], 0
    mov dl, [drive]
    mov si, dap
    mov ah, 0x42
    int 0x13
    pop es
    pop ds
    jc .bad
    popad
    clc
    ret
.bad:
    popad
    stc
    ret

; --------------------------------------------------------------- diagnostics
serial_init:
    push ax
    push dx
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
    pop dx
    pop ax
    ret

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

fail:
    mov al, '!'
    call putc
    cli
.halt:
    hlt
    jmp .halt

; ------------------------------------------------------------- VBE mode set
; Ask the BIOS for a linear-framebuffer mode and record it at 0x9000 for the
; kernel. Runs before the protected-mode switch, while int 10h is available.
; The controller-info and mode-info blocks live at 0x18000 and 0x19000, both
; below the Multiboot area at 0x1C000.
vbe_init:
    pusha
    push ds
    push es
    push fs

    mov ax, 0x1800
    mov es, ax
    xor di, di
    mov dword [es:di], 0x32454256       ; "VBE2"
    mov ax, 0x4F00
    int 0x10
    cmp ax, 0x004F
    jne .done
    cmp dword [es:di], 0x41534556       ; "VESA"
    jne .done

    mov si, [es:di + 0x0E]              ; mode list offset
    mov ax, [es:di + 0x10]              ; mode list segment
    mov fs, ax
    mov dword [best_mode], 0xFFFF
    mov dword [best_score], 0

.mode_loop:
    mov cx, [fs:si]
    cmp cx, 0xFFFF
    je .modes_done
    add si, 2

    push si
    push fs
    mov ax, 0x1900
    mov es, ax
    xor di, di
    mov ax, 0x4F01
    int 0x10
    pop fs
    pop si
    cmp ax, 0x004F
    jne .mode_loop

    mov ax, [es:di]                     ; attributes
    test ax, 0x0001                     ; supported
    jz .mode_loop
    test ax, 0x0080                     ; linear framebuffer
    jz .mode_loop
    cmp byte [es:di + 0x1B], 6          ; direct colour
    jne .mode_loop
    mov al, [es:di + 0x19]              ; bits per pixel
    cmp al, 32
    je .bpp_ok
    cmp al, 24
    jne .mode_loop
.bpp_ok:
    mov dx, [es:di + 0x12]
    mov bx, [es:di + 0x14]
    xor eax, eax
    cmp dx, 1024
    jne .try800
    cmp bx, 768
    jne .try800
    mov eax, 1000
    jmp .score
.try800:
    cmp dx, 800
    jne .try640
    cmp bx, 600
    jne .try640
    mov eax, 800
    jmp .score
.try640:
    cmp dx, 640
    jne .mode_loop
    cmp bx, 480
    jne .mode_loop
    mov eax, 600
.score:
    cmp byte [es:di + 0x19], 32
    jne .no_bonus
    add eax, 50
.no_bonus:
    cmp eax, [best_score]
    jbe .mode_loop
    mov [best_score], eax
    mov [best_mode], cx
    jmp .mode_loop

.modes_done:
    mov cx, [best_mode]
    cmp cx, 0xFFFF
    je .done
    mov bx, cx
    or bx, 0x4000                       ; use the linear framebuffer
    mov ax, 0x4F02
    int 0x10
    cmp ax, 0x004F
    jne .done

    ; Re-query: some BIOSes report the real pitch only after SetMode.
    mov ax, 0x1900
    mov es, ax
    xor di, di
    mov ax, 0x4F01
    int 0x10
    cmp ax, 0x004F
    jne .done

    mov ax, 0x1900
    mov fs, ax
    xor ax, ax
    mov es, ax
    mov di, 0x9000

    mov dword [es:di + 0], 0x534E4656   ; "SNFV"
    movzx eax, word [fs:0x12]
    mov [es:di + 4], eax
    movzx eax, word [fs:0x14]
    mov [es:di + 8], eax
    movzx eax, word [fs:0x10]
    mov [es:di + 12], eax
    movzx eax, byte [fs:0x19]
    mov [es:di + 16], eax
    mov dword [es:di + 20], 0
    mov al, [fs:0x20]
    mov [es:di + 24], al
    mov al, [fs:0x1F]
    mov [es:di + 25], al
    mov al, [fs:0x22]
    mov [es:di + 26], al
    mov al, [fs:0x21]
    mov [es:di + 27], al
    mov al, [fs:0x24]
    mov [es:di + 28], al
    mov al, [fs:0x23]
    mov [es:di + 29], al
    mov byte [es:di + 30], 0
    mov byte [es:di + 31], 0
    mov eax, [fs:0x28]                  ; PhysBasePtr
    mov [es:di + 32], eax
    mov dword [es:di + 36], 0
    movzx eax, word [fs:0x10]
    movzx ecx, word [fs:0x14]
    imul eax, ecx
    mov [es:di + 40], eax
    mov dword [es:di + 44], 0

.done:
    pop fs
    pop es
    pop ds
    popa
    ret

; ---------------------------------------------------------------- variables
dap:
    db 0x10, 0
dap_count: dw 0
dap_off:   dw 0
dap_seg:   dw 0
dap_lba:   dq 0

drive:       db 0
shift:       dd 0
ssize:       dd 0
base_lba:    dd 0
arc_off:     dd 0
arc_size:    dd 0
ent_count:   dd 0
ent_idx:     dd 0
ep:          dd 0
elf_sector:  dd 0
ph_off:      dd 0
ph_ent:      dd 0
ph_num:      dd 0
ph_idx:      dd 0
seg_off:     dd 0
seg_dest:    dd 0
seg_filesz:  dd 0
seg_memsz:   dd 0
entry_point: dd 0
tmp_abs:     dd 0
tmp_size:    dd 0
mod_next:    dd 0
best_mode:   dd 0
best_score:  dd 0
mod_count:   dd 0
cur_sector:  dd 0
cur_skip:    dd 0
remaining:   dd 0
dest_ptr:    dd 0
chunk_sectors: dd 0
chunk_bytes: dd 0
copied:      dd 0
e820_count:  dd 0
e820_ebx:    dd 0
max_end:     dd 0

; ------------------------------------------------- Multiboot information
build_mb:
    pushad

    ; Collect the firmware E820 map into 0x1D000.
    mov dword [e820_count], 0
    mov dword [e820_ebx], 0
.e820:
    cmp dword [e820_count], 60
    jae .e820_done
    mov eax, [e820_count]
    imul eax, 24
    add eax, 0xD000
    mov di, ax
    push ds
    push es
    mov ax, 0x1000
    mov es, ax
    mov eax, 0xE820
    mov edx, 0x534D4150
    mov ecx, 24
    mov ebx, [e820_ebx]
    int 0x15
    pop es
    pop ds
    jc .e820_done
    cmp eax, 0x534D4150
    jne .e820_done
    mov [e820_ebx], ebx
    inc dword [e820_count]
    test ebx, ebx
    jnz .e820
.e820_done:

    ; Convert E820 records to Multiboot mmap records at 0x1D800.
    mov ecx, [e820_count]
    mov di, 0xD000
    mov si, 0xD800
.mmap:
    test ecx, ecx
    jz .mmap_done
    mov dword [si], 20
    mov eax, [di]
    mov [si + 4], eax
    mov eax, [di + 4]
    mov [si + 8], eax
    mov eax, [di + 8]
    mov [si + 12], eax
    mov eax, [di + 12]
    mov [si + 16], eax
    mov eax, [di + 16]
    mov [si + 20], eax
    add di, 24
    add si, 24
    dec ecx
    jmp .mmap
.mmap_done:

    ; Highest usable address above 1 MiB, for mem_upper.
    mov dword [max_end], 0
    mov ecx, [e820_count]
    mov di, 0xD000
.max:
    test ecx, ecx
    jz .max_done
    cmp dword [di + 16], 1
    jne .max_next
    cmp dword [di + 4], 0
    jne .max_next
    mov eax, [di]
    cmp eax, 0x100000
    jb .max_next
    add eax, [di + 8]
    cmp eax, [max_end]
    jbe .max_next
    mov [max_end], eax
.max_next:
    add di, 24
    dec ecx
    jmp .max
.max_done:

    mov dword [0xC000], 0x49         ; mem | modules | mmap
    mov dword [0xC004], 639
    mov eax, [max_end]
    sub eax, 0x100000
    shr eax, 10
    mov [0xC008], eax
    mov eax, [mod_count]
    mov [0xC014], eax
    mov dword [0xC018], 0x1C400
    mov eax, [e820_count]
    imul eax, 24
    mov [0xC02C], eax
    mov dword [0xC030], 0x1D800

    popad
    ret

; --------------------------------------------- protected-mode bulk copy/zero
pm_memset:
    mov edx, 1
    jmp pm_common

pm_copy:
    xor edx, edx

pm_common:
    pushad
    lgdt [gdtr]
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
    cld
    test edx, edx
    jnz .zero
    rep movsb
    jmp .out
.zero:
    xor eax, eax
    rep stosb
.out:
    jmp dword 0x18:rm16

BITS 16
rm16:
    mov eax, cr0
    and al, 0xFE
    mov cr0, eax
    jmp word 0x1000:rm16_done
rm16_done:
    mov ax, 0x1000
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    xor ax, ax
    mov ss, ax        ; base 0; ESP keeps pointing at the saved frame
    popad
    ret

; -------------------------------------------------------- kernel handoff
enter_kernel:
    cli
    mov ebp, [entry_point]
    lgdt [gdtr]
    mov ecx, cr0
    or cl, 1
    mov cr0, ecx
    jmp dword 0x08:kernel32

BITS 32
kernel32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov esp, 0x7C00

    mov eax, 0x2BADB002
    mov ebx, 0x1C000
    sub ebp, 0x10000          ; CS base is 0x10000 here; make EBP flat
    call ebp
.hang:
    cli
    hlt
    jmp .hang

BITS 16
align 8
gdt:
    dq 0
    dq 0x00CF9A010000FFFF        ; 0x08 32-bit code, base 0x10000
    dq 0x00CF92000000FFFF        ; 0x10 32-bit data, flat
    dq 0x00009A010000FFFF        ; 0x18 16-bit code, base 0x10000
    dq 0x000092010000FFFF        ; 0x20 16-bit data
gdt_end:
gdtr:
    dw gdt_end - gdt - 1
    dd gdt + 0x10000
