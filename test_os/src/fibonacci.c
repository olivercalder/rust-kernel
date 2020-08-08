// Compile to assembly using gcc -static [-Og] -S fibonacci.c
// Compile to binary using gcc -static [-Og] -o fibonacci fibonacci.c

// This is meant to match fib.s as closely as possible

#include <stdlib.h>

void fib10() {
    int a, b, c, d;
    a = 0;
    b = 1;
    c = 10; // The cth fibonacci number

    c -= 1;
    while (c > 0) {
        (d = c & 1) ? (a += b) : (b += a);
        c -= 1;
    }
    if (d == 0) { a = b; }
    exit(a);    // perform an exit syscall
}
