; 64-bit interrupt stubs and common dispatch.

BITS 64

section .text

%macro ISR_NOERR 1
global isr%1
isr%1:
    push qword 0
    push qword %1
    jmp isr_common
%endmacro

%macro ISR_ERR 1
global isr%1
isr%1:
    push qword %1
    jmp isr_common
%endmacro

%assign v 0
%rep 256
    %if v == 8 || v == 10 || v == 11 || v == 12 || v == 13 || v == 14 || v == 17 || v == 21 || v == 29 || v == 30
        ISR_ERR v
    %else
        ISR_NOERR v
    %endif
    %assign v v+1
%endrep

extern isr_dispatch
extern fpu_areas

isr_common:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    cld

    ; Save the interrupted context's x87/SSE state before any Rust code (which
    ; may use SSE) runs. The current FXSAVE area is per CPU, indexed by the
    ; logical CPU number kept at gs:0. Skip everything while GS is not set up
    ; yet (early boot faults still have to be reportable).
    mov ecx, [gs:0]
    cmp ecx, 8
    jae .no_fpu_save
    lea rdx, [rel fpu_areas]
    mov rax, [rdx + rcx*8]
    test rax, rax
    jz .no_fpu_save
    fxsave [rax]
.no_fpu_save:

    mov rdi, rsp
    call isr_dispatch
    test rax, rax
    jz .keep_stack
    mov rsp, rax
.keep_stack:

    ; Restore the state of whichever task schedule() selected, right before
    ; returning to it.
    mov ecx, [gs:0]
    cmp ecx, 8
    jae .no_fpu_restore
    lea rdx, [rel fpu_areas]
    mov rax, [rdx + rcx*8]
    test rax, rax
    jz .no_fpu_restore
    fxrstor [rax]
.no_fpu_restore:

    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    iretq

section .rodata
global isr_stub_table
isr_stub_table:
%assign v 0
%rep 256
    dq isr%+v
    %assign v v+1
%endrep
