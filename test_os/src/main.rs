#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_os::test_runner)]
#![reexport_test_harness_main = "test_main"]  // By default, generates a main() function to test, but we have no_main

extern crate rlibc;
use core::panic::PanicInfo;
use test_os::println;

// Provide an entry point, since the linker looks for _start by default
#[no_mangle] // Ensure that this is really named _start, which is default entry point for most systems
pub extern "C" fn _start() -> ! {
    println!("Fear is the mind killer.");

    test_os::init();

    #[cfg(test)]  // Only call test_main in test contexts, since it is not generated on a normal run
    test_main();

    println!("Fear is the little-death that brings total obliteration.");
    test_os::hlt_loop();    // Halt instead of looping forever
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    test_os::hlt_loop();
}

// Panic handler in test mode
#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    test_os::test_panic_handler(info)
}

#[test_case]
fn test_main() {
    assert_eq!(1, 1);
}
