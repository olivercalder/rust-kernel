// Compile to assembly using gcc -static [-Og] -S fibonacci.c
// Compile to binary using gcc -static [-Og] -o fibonacci fibonacci.c

#include <stdlib.h>

void main() {
    int a, b, c, d;
    a = 0;
    b = 1;
    c = 10;

    c -= 1;
    while (c > 0) {
        (d = c & 1) ? (a += b) : (b += a);
        c -= 1;
    }
    if (!d) { a = b; }
    exit(a);
}
