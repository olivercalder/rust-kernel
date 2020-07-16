#![no_std]
#![no_main]

static HELLO: &[u8] = b"Hello World!";

// Panic handler
use core::panic::PanicInfo;
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

#[no_mangle] // Ensure that this is really named _start, which is default entry point for most systems
// Provide an entry point, since the linker looks for _start by default
pub extern "C" fn _start() -> ! {
    let vga_buffer = 0xb8000 as *mut u8;  // vga_buffer is essentially a char* which points to the
                                          // part of memory which is mapped to the display buffer

    for (i, &byte) in HELLO.iter().enumerate() {
        unsafe {
            *vga_buffer.offset(i as isize * 2) = byte;    // Write the byte to the buffer
            *vga_buffer.offset(i as isize * 2 + 1) = 0xb; // Make it cyan
        }
    }

    loop {}
}

