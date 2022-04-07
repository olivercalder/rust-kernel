#![allow(deprecated)]
#![allow(unused_imports)]

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::{gdt, print, println, serial_println, hlt_loop, vga_buffer, serial::SERIAL1, png, QemuExitCode, exit_qemu};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use miniz_oxide;
use alloc::vec::Vec;
use uart_16550::SerialPort;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> = spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });  // unsafe due to unchecked offsets

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    LegacyTimer = PIC_1_OFFSET,
    Keyboard,
    Secondary,
    Serial2,
    Serial1,
}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
    pub fn as_pic_enable_mask(self) -> u8 {
        let diff = self.as_usize() - InterruptIndex::LegacyTimer.as_usize();
        let mask = 0xff & !(1 << diff);
        mask as u8
    }
}

lazy_static! {  // IDT will be initialized when it is referenced the first time
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
                // assumes that the IST index is valid and not already used for another exception
        }
        idt[InterruptIndex::LegacyTimer.as_usize()].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard.as_usize()].set_handler_fn(keyboard_interrupt_handler);
        idt[InterruptIndex::Serial1.as_usize()].set_handler_fn(serial_interrupt_handler);
        idt[InterruptIndex::Serial2.as_usize()].set_handler_fn(serial_interrupt_handler_two);
        idt[0x80].set_handler_fn(syscall_interrupt_handler);
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

pub unsafe fn init_pics() {
    PICS.lock().initialize();
    let keyboard_enable = InterruptIndex::Keyboard.as_pic_enable_mask();
    let serial_enable = InterruptIndex::Serial1.as_pic_enable_mask()
        & InterruptIndex::Serial2.as_pic_enable_mask();
    SERIAL1.lock().init();
    PICS.lock().write_masks(keyboard_enable & serial_enable, 0xff);
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(stack_frame: InterruptStackFrame, error_code: PageFaultErrorCode) {
    use x86_64::registers::control::Cr2;    // CR2 register is set by CPU on page fault

    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", Cr2::read());
    println!("Error Code: {:?}", error_code);
    println!("{:#?}", stack_frame);
    hlt_loop();
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error_code: u64) -> ! {
    println!("DOUBLE FAULT");
    // error code is always 0
    // x86_64 does not permit returning from double fault, hence -> !
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    // print!(".");
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::LegacyTimer.as_u8()); }  // using the wrong interrupt index is dangerous
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60); // PS/2 data port is I/O port 0x60
    let scancode: u8 = unsafe { port.read() };  // must read scancode from the port before another keyboard interrupt can be handled

    crate::task::keyboard::add_scancode(scancode);

    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    // using the wrong interrupt index is dangerous
}

fn read_serial_png() -> Option<Vec<u8>> {
    let mut raw_data: Vec<u8> = Vec::new();
    // Verify that first 8 bytes match the png signature
    for i in 0..8 {
        let serial_byte = SERIAL1.lock().receive();
        raw_data.push(serial_byte);
        if serial_byte != png::PNG_SIGNATURE[i] {
            // Invalid png, so print what it was and then return
            println!("Invalid byte {:02x?}", serial_byte);
            return None;
        }
    }
    println!("Valid PNG signature");
    loop {
        let mut length: u32 = 0;
        let mut type_arr: [u8; 4] = [0; 4];
        for _ in 0..4 {
            let new_byte: u8 = SERIAL1.lock().receive();
            raw_data.push(new_byte);
            length <<= 8;
            length += new_byte as u32;
        }
        for i in 0..4 {
            let new_byte: u8 = SERIAL1.lock().receive();
            raw_data.push(new_byte);
            type_arr[i] = new_byte;
        }
        for _ in 0..length+4 {  // include the four crc bytes
            raw_data.push(SERIAL1.lock().receive());
        }
        if &type_arr == "IEND".as_bytes() {
            println!("Read IEND chunk, break from loop");
            break;
        } else if &type_arr == "IHDR".as_bytes() {
            println!("Read IHDR chunk");
        } else if &type_arr == "IDAT".as_bytes() {
            print!("Read IDAT chunk... ");
        } else {
            println!("Read chunk with unexpected type: {:?}", type_arr);
        }
    }
    return Some(raw_data);
}

extern "x86-interrupt" fn serial_interrupt_handler(_stack_frame: InterruptStackFrame) {
    println!("Serial interrupt");
    let raw_data: Vec<u8>;
    match read_serial_png() {
        Some(data) => raw_data = data,
        None => {
            unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial1.as_u8()); };
            return;
        },
    }
    let max_width: usize = 150;
    let max_height: usize = 150;
    let zoom_to_fill: bool = true;
    let new_png: Vec<u8> = match png::generate_thumbnail(raw_data, max_width, max_height, zoom_to_fill) {
        Ok(data) => data,
        Err(e) => {
            serial_println!("Error when generating thumbnail: {:?}", e);
            exit_qemu(QemuExitCode::Failed);
            Vec::new()
        }
    };
    for byte in new_png {
        SERIAL1.lock().send_raw(byte);
    }
    exit_qemu(QemuExitCode::Success);
    unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial1.as_u8()); }
    // using the wrong interrupt index is dangerous
}

extern "x86-interrupt" fn serial_interrupt_handler_two(_stack_frame: InterruptStackFrame) {
    println!("Serial Interrupt Two!");

    // use x86_64::instructions::port::Port;
    //
    // let mut port = Port::new(0x3f8); // PS/2 data port is I/O port 0x60
    //
    // println!("Serial Interrupt Two!");
    // let scancode: u8 = unsafe { port.read() };  // must read scancode from the port before another keyboard interrupt can be handled
    // println!("{:?}", scancode);
    //
    // //
    // unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Keyboard.as_u8()); }
    // using the wrong interrupt index is dangerous
}



extern "x86-interrupt" fn syscall_interrupt_handler(_stack_frame: InterruptStackFrame,) {
    unsafe {
        println!("{:?} {:?}", _stack_frame.stack_pointer, (*_stack_frame.stack_pointer.as_ptr::<*const i32>()));
    }
    println!("TRIGGERED SYSCALL");
    hlt_loop();
}

#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
}
