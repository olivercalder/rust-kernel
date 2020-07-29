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

    use test_os::memory;
    use x86_64::{structures::paging::{MapperAllSizes, Page}, VirtAddr};

    println!("Fear is the mind killer.");

    test_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { memory::BootInfoFrameAllocator::init(&boot_info.memory_map) };

    // map an unused page which already has a level 1 table already allocated
    let page = Page::containing_address(VirtAddr::new(0xdeadbeaf));
    memory::create_example_mapping(page, &mut mapper, &mut frame_allocator);

    // write the string "New!" to the screen through the new mapping
    let page_ptr: *mut u64 = page.start_address().as_mut_ptr();
    unsafe { page_ptr.offset(400).write_volatile(0x_f021_f077_f065_f04e) };
    // uses offset of 400 so that it isn't immediately shifted off screen

    let addresses = [
        //the identity-mapped vga buffer page
        0xb8000,
        // some code page
        0x201008,
        // some stack page
        0x0100_0020_1a10,
        // cirtual address mapped to physical address 0
        boot_info.physical_memory_offset,
    ];

    for &address in &addresses {
        let virt = VirtAddr::new(address);
        let phys = mapper.translate_addr(virt); // translate_addr() is provided by the MapperAllSize trait
        println!("{:?} -> {:?}", virt, phys);
    }

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
