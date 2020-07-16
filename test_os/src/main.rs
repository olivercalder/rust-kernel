#![no_std]
#![no_main]

mod vga_buffer;

// Panic handler
use core::panic::PanicInfo;
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);

    loop {}
}

#[no_mangle] // Ensure that this is really named _start, which is default entry point for most systems
// Provide an entry point, since the linker looks for _start by default
pub extern "C" fn _start() -> ! {
    println!("Hello Galaxy, the answer is {}", 42);
    panic!("We panic on purpose");

    loop {}
}

