#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(test_os::test_runner)]
#![reexport_test_harness_main = "test_main"]  // By default, generates a main() function to test, but we have no_main

extern crate rlibc;
use core::panic::PanicInfo;
use test_os::println;
use bootloader::{BootInfo, entry_point};

entry_point!(kernel_main);  // defines any Rust function as _start() function after doing type checking

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    // BootInfo struct contains memory_map and physical_map_offset
    //  memory_map: amount of physical memory and which regions reserved for devices
    //  physical_memory_offset: start address of physical memory mapping

    use x86_64::VirtAddr;
    use test_os::memory;

    println!("Fear is the mind killer.");

    test_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };

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
