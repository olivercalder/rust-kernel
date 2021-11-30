#![allow(deprecated)]
#![allow(unused_imports)]

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::{gdt, print, println, hlt_loop, vga_buffer};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
use base64;
use miniz_oxide;
use alloc::vec::Vec;
use uart_16550::SerialPort;



pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: spin::Mutex<ChainedPics> = spin::Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });  // unsafe due to unchecked offsets

static mut SERIAL_PORT: spin::Mutex<SerialPort> = spin::Mutex::new(unsafe { SerialPort::new(0x3F8) });

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
    SERIAL_PORT.lock().init();
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

const IHDR_ARR: [u8; 4] = [0x49, 0x48, 0x44, 0x52];
const IDAT_ARR: [u8; 4] = [0x49, 0x44, 0x41, 0x54];
const IEND_ARR: [u8; 4] = [0x49, 0x45, 0x4e, 0x44];

struct Chunk {
    length: u32,
    type_arr: [u8; 4],
    data: Vec<u8>,
    crc: [u8; 4],
}

struct PNGInfo {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace_method: u8,
}

fn read_chunk() -> Chunk {
    let mut length: u32 = 0;
    let mut type_arr: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    let mut crc: [u8; 4] = [0x00, 0x00, 0x00, 0x00];
    for _ in 0..4 {
        length <<= 8;
        length += unsafe { SERIAL_PORT.lock().receive() } as u32;
    }
    for i in 0..4 {
        unsafe { type_arr[i] = SERIAL_PORT.lock().receive() };
    }
    let mut data = Vec::with_capacity(length as usize);
    for _ in 0..length {
        data.push(unsafe { SERIAL_PORT.lock().receive() });
    }
    for i in 0..4 {
        unsafe { crc[i] = SERIAL_PORT.lock().receive() };
    }
    let chunk: Chunk = Chunk {
        length,
        type_arr,
        data,
        crc,
    };
    chunk
}

fn chunk_types_equal(arr_1: [u8; 4], arr_2: [u8; 4]) -> bool {
    return (arr_1[0] == arr_2[0]) && (arr_1[1] == arr_2[1]) &&
        (arr_1[2] == arr_2[2]) && (arr_1[3] == arr_2[3]);
}

fn parse_ihdr_chunk(chunk: &Chunk) -> PNGInfo {
    // TODO Implement this properly
    // assert!(chunk.length == 13);
    // assert!(chunk_types_equal(chunk.type_arr == IHDR_ARR));
    return PNGInfo {
        width: ((chunk.data[0] as u32) << 24) | ((chunk.data[1] as u32) << 16) |
            ((chunk.data[2] as u32) << 8) | (chunk.data[3] as u32),
        height: ((chunk.data[4] as u32) << 24) | ((chunk.data[5] as u32) << 16) |
            ((chunk.data[6] as u32) << 8) | (chunk.data[7] as u32),
        bit_depth: chunk.data[8],
        color_type: chunk.data[9],
        compression_method: chunk.data[10],
        filter_method: chunk.data[11],
        interlace_method: chunk.data[12],
    };
}

fn parse_idat_chunk(chunk: &Chunk) -> Vec<u8> {
    // TODO Implement this properly
    // assert!(chunk_types_equal(chunk.type_arr, IDAT_ARR));
    println!("Started decompressing IDAT chunk...");
    let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(chunk.data.as_slice()).expect("Failed to decompress!");
    println!("Finished decompressing IDAT chunk");
    return decompressed
}

extern "x86-interrupt" fn serial_interrupt_handler(_stack_frame: InterruptStackFrame) {
    println!("Serial interrupt");

    // All png files must begin with bytes: [0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'];
    let correct_signature: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

    // Verify that first 8 bytes match the png signature
    for i in 0..8 {
        let serial_byte = unsafe { SERIAL_PORT.lock().receive() };
        if correct_signature[i] != serial_byte {
            // Invalid png, so just return
            unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial1.as_u8()); };
            return;
        }
    }

    println!("Valid PNG signature");

    // If here, then signature matches.
    // Don't signal end of interrupt until png has been fully read and processed.

    let mut chunks: Vec<Chunk> = Vec::new();
    let mut index: usize = 0;
    let mut first_ihdr: usize = 0;
    let mut first_idat: usize = usize::MAX;

    loop {
        let mut new_chunk = read_chunk();
        println!("\nNew chunk read");
        println!("length: {:?}", new_chunk.length);
        println!("type:   {:?}", new_chunk.type_arr);
        println!("data:");
        for byte in &new_chunk.data {
            print!("{:02x?} ", byte);
        }
        print!("\n");
        if chunk_types_equal(new_chunk.type_arr, IEND_ARR) {
            println!("Read IEND chunk, break from loop");
            chunks.push(new_chunk);
            break;
        } else if chunk_types_equal(new_chunk.type_arr, IDAT_ARR) {
            print!("Read IDAT chunk... ");
            // TODO: take advantage of fact that IDAT chunks must be consecutive
            if first_idat == usize::MAX {
                println!("the first one");
                first_idat = index;
                chunks.push(new_chunk);
            } else {
                println!("not the first one");
                chunks[first_idat].length += new_chunk.length;
                chunks[first_idat].data.append(&mut new_chunk.data);
            }
        } else if chunk_types_equal(new_chunk.type_arr, IHDR_ARR) {
            println!("Read IHDR chunk");
            first_ihdr = index;
            chunks.push(new_chunk);
        } else {
            println!("Read chunk with unexpected type");
            chunks.push(new_chunk);
        }
        index += 1;
    }
    println!("Escaped the loop");

    println!("First IHDR at chunk {}", first_ihdr);
    println!("First IDAT at chunk {}", first_idat);

    let decompressed_data = parse_idat_chunk(&(chunks[first_idat]));
    println!("Decompressed data from IDAT blocks");
    for byte in decompressed_data {
        print!("{:02x?} ", byte);
    }
    print!("\n");


    // let mut serial_data = Vec::new();

    /*
    loop {
        unsafe {
            let serial_byte = SERIAL_PORT.lock().receive();
            print!("{:02x?} ", serial_byte);
            if serial_byte == 10 {
                //break;
            }
            serial_data.push(serial_byte)
        }
    }
    */

    // let idat_data_compressed: [u8; 27] = [24, 87, 99, 180, 15, 220, 250, 127, 235, 167, 70, 6, 70, 159, 245, 1, 255, 55, 7, 172, 103, 0, 0, 79, 43, 8, 107];
    // let decompressed = miniz_oxide::inflate::decompress_to_vec_zlib(&idat_data_compressed).expect("Failed to decompress!");

    // for data in serial_data.iter() {
    //     vga_buffer::print_byte(*data);
    // }

    // for data in serial_data.iter() {
    //     vga_buffer::print_byte(*data);
    // }

    // let mut buffer = Vec::<u8>::new();
    // base64::decode_config_buf("aGVsbG8gd29ybGR+Cg==", base64::STANDARD, &mut buffer).unwrap();
    // println!("{:?}", buffer);
    // println!();

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
