/* Minimal C runtime entry point for Sn0wflakeOS user programs.
 *
 * icepeak enters `_start` with the System V function convention: argc in
 * %rdi and argv in %rsi (not the Linux process-stack layout). There is no
 * dynamic loader and no libc startup, so this file calls `main` directly and
 * hands the return value to `exit`.
 */

    .text
    .globl _start
    .type _start, @function
_start:
    xor     %rbp, %rbp
    and     $-16, %rsp
    call    main
    mov     %eax, %edi
    call    exit
1:
    hlt
    jmp     1b
    .size _start, .-_start

    .section .note.GNU-stack, "", @progbits
