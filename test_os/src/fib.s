    # Compile with gcc -nostdlib -static -o fib fib.s

    .global _start
    .text
_start:
    mov $0, %rax
    mov $1, %rbx
    mov $10, %rdi
loop:
    sub $1, %rdi
    cmp $0, %rdi
    je end
    mov $1, %rsi
    and %rdi, %rsi
    cmp $0, %rsi
    je set_rbx
    add %rbx, %rax
    jne loop
set_rbx:
    add %rax, %rbx
    jne loop
end:
    cmp $0, %rsi
    jne return
    mov %rbx, %rax
return:
    mov %rax, %rdi  # rdi holds exit status
    mov $60, %rax   # rax indicates which syscall
    syscall
