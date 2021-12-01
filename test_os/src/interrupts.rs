#![allow(deprecated)]
#![allow(unused_imports)]

use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use crate::{gdt, print, println, hlt_loop, vga_buffer};
use lazy_static::lazy_static;
use pic8259::ChainedPics;
use spin;
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
const PLTE_ARR: [u8; 4] = [0x50, 0x4c, 0x54, 0x45];

const GREYSCALE: u8 = 0;
const TRUECOLOR: u8 = 2;
const INDEXED_COLOR: u8 = 3;
const GREYSCALE_WITH_ALPHA: u8 = 4;
const TRUECOLOR_WITH_ALPHA: u8 = 6;

const DEFAULT_COMPRESSION_LEVEL: u8 = 3;

struct Chunk {
    length: u32,
    type_arr: [u8; 4],
    data: Vec<u8>,
    crc: [u8; 4],
}

struct PNGInfo {
    width: usize,
    height: usize,
    bit_depth: u8,
    color_type: u8,
    compression_method: u8,
    filter_method: u8,
    interlace_method: u8,
}

fn channel_count(color_type: u8) -> usize {
    match color_type {
        GREYSCALE => 1,
        TRUECOLOR => 3,
        INDEXED_COLOR => 3,
        GREYSCALE_WITH_ALPHA => 2,
        TRUECOLOR_WITH_ALPHA => 4,
        _ => panic!("Invalid color type: {:?}", color_type),
    }
}

fn check_color_type_valid(color_type: u8) -> bool {
    match color_type {
        GREYSCALE => true,
        TRUECOLOR => true,
        INDEXED_COLOR => true,
        GREYSCALE_WITH_ALPHA => true,
        TRUECOLOR_WITH_ALPHA => true,
        _ => false,
    }
}

fn check_bit_depth_valid(depth: u8, color_type: u8) -> bool {
    let depth_options: Vec<u8> = match color_type {
        GREYSCALE => Vec::from([1, 2, 4, 8, 16]),
        TRUECOLOR => Vec::from([8, 16]),
        INDEXED_COLOR => Vec::from([1, 2, 4, 8]),
        GREYSCALE_WITH_ALPHA => Vec::from([8, 16]),
        TRUECOLOR_WITH_ALPHA => Vec::from([8, 16]),
        _ => panic!("Invalid color type: {:?}", color_type),
    };
    depth_options.contains(&depth)
}

fn check_interlace_method_valid(method: u8) -> bool {
    return (method & !1) == 0
}

fn check_png_info_valid(info: &PNGInfo) -> bool {
    (check_color_type_valid(info.color_type) == true)
    && (check_bit_depth_valid(info.bit_depth, info.color_type) == true)
    && (info.compression_method == 0)   // png only supports 0
    && (info.filter_method == 0)        // png only supports 0
    && (check_interlace_method_valid(info.interlace_method) == true)

    && (info.color_type & 1 == 0)   // For now, do not allow indexed-color
    && (info.bit_depth == 8)        // For now, only accept bit depth of 8
}

fn compute_bytes_per_pixel(info: &PNGInfo) -> usize {
    let channels = channel_count(info.color_type);
    let bits_per_pixel = info.bit_depth as usize * channels;
    bits_per_pixel / 8
}

fn compute_total_data_bytes(info: &PNGInfo) -> usize {
    info.height * (1 + compute_bytes_per_pixel(&info) * info.width)
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

fn parse_ihdr_data(data: Vec<u8>) -> PNGInfo {
    // TODO Implement this properly
    // assert!(data.len() == 13);
    return PNGInfo {
        width: ((data[0] as usize) << 24) | ((data[1] as usize) << 16) |
            ((data[2] as usize) << 8) | (data[3] as usize),
        height: ((data[4] as usize) << 24) | ((data[5] as usize) << 16) |
            ((data[6] as usize) << 8) | (data[7] as usize),
        bit_depth: data[8],
        color_type: data[9],
        compression_method: data[10],
        filter_method: data[11],
        interlace_method: data[12],
    };
}

fn decompress_data(data: Vec<u8>) -> Vec<u8> {
    return miniz_oxide::inflate::decompress_to_vec_zlib(data.as_slice()).expect("Failed to decompress!");
}

fn compress_data(data: Vec<u8>) -> Vec<u8> {
    return miniz_oxide::deflate::compress_to_vec_zlib(data.as_slice(), DEFAULT_COMPRESSION_LEVEL);
}

fn unfilter_data(info: &PNGInfo, data: Vec<u8>) -> Vec<u8> {
    // Unfilters and deserializes data, thus removing filter type byte from the
    // beginning of each scanline
    let mut unfiltered: Vec<u8> = Vec::with_capacity(data.len() - info.height);
    let bytes_per_pixel: usize = compute_bytes_per_pixel(&info);
    let stride: usize = info.width * bytes_per_pixel;
    for row in 0..info.height {
        let filter_type = data[row * (stride + 1)];
        match filter_type {
            0 => {  // no change
                for col in 0..stride {
                    unfiltered.push(data[row * (stride + 1) + 1 + col]);
                }
            },
            1 => {  // sub
                for col in 0..bytes_per_pixel {
                    unfiltered.push(data[row * (stride + 1) + 1 + col]);
                }
                for col in bytes_per_pixel..stride {
                    let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                    unfiltered.push((orig + unfiltered[row * stride + col - bytes_per_pixel] as u32) as u8);
                }
            },
            2 => {  // up
                if row == 0 {
                    for col in 0..stride {
                        unfiltered.push(data[row * (stride + 1) + 1 + col]);
                    }
                } else {
                    for col in 0..stride {
                        let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                        unfiltered.push((orig + unfiltered[(row - 1) * stride + col] as u32) as u8);
                    }
                }
            },
            3 => {  // average
                for col in 0..stride {
                    let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                    let mut sum: u32 = 0;
                    if col >= bytes_per_pixel {
                        sum += unfiltered[row * stride + col - bytes_per_pixel] as u32;
                    }
                    if row > 0 {
                        sum += unfiltered[(row - 1) * stride + col] as u32;
                    }
                    unfiltered.push((orig + (sum >> 1)) as u8);
                }
            },
            4 => {  // Paeth predictor
                for col in 0..stride {
                    let orig: u32 = data[row * (stride + 1) + 1 + col] as u32;
                    let a: i32 = if col >= bytes_per_pixel {
                        unfiltered[row * stride + col - bytes_per_pixel] as i32 } else { 0 };
                    let b: i32 = if row > 0 {
                        unfiltered[(row - 1) * stride + col] as i32 } else { 0 };
                    let c: i32 = if row > 0 && col >= bytes_per_pixel {
                        unfiltered[(row - 1) * stride + col - bytes_per_pixel] as i32 } else { 0 };
                    let p: i32 = a + b - c;
                    let result: u32;
                    let mut pa: i32 = p - a;
                    pa *= (pa >> 31) | 1;   // take absolute value of pa
                    let mut pb: i32 = p - b;
                    pb *= (pb >> 31) | 1;   // take absolute value of pb
                    let mut pc: i32 = p - c;
                    pc *= (pc >> 31) | 1;   // take absolute value of pc
                    if pa <= pb && pa <= pc {
                        result = a as u32;
                    } else if pb <= pc {
                        result = b as u32;
                    } else {
                        result = c as u32;
                    }
                    unfiltered.push((orig + result) as u8);
                }
            },
            _ => panic!("Invalid filter type {:?} for row {:?}", filter_type, row),
        }
    }
    return unfiltered;
}

fn filter_data(info: &PNGInfo, data: Vec<u8>) -> Vec<u8> {
    // Filters data and inserts filter type byte for each scanline
    let mut filtered: Vec<u8> = Vec::with_capacity(data.len() + info.height);
    for row in 0..info.height {
        // For now, always use filter type 0 -- no-op
        filtered.push(0);
        for col in 0..info.width {
            filtered.push(data[row * info.width + col]);
        }
    }
    return filtered;
}

extern "x86-interrupt" fn serial_interrupt_handler(_stack_frame: InterruptStackFrame) {
    println!("Serial interrupt");

    // All png files must begin with bytes: [0x89, 'P', 'N', 'G', '\r', '\n', 0x1a, '\n'];
    let correct_signature: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

    // Verify that first 8 bytes match the png signature
    for i in 0..8 {
        let serial_byte = unsafe { SERIAL_PORT.lock().receive() };
        if correct_signature[i] != serial_byte {
            // Invalid png, so print what it was and then return
            println!("First byte invalid: {:02x?}", serial_byte);
            unsafe { PICS.lock().notify_end_of_interrupt(InterruptIndex::Serial1.as_u8()); };
            return;
        }
    }

    println!("Valid PNG signature");

    // If here, then signature matches.
    // Don't signal end of interrupt until png has been fully read and processed.

    let mut ihdr_data: Vec<u8> = Vec::with_capacity(13);
    let mut idat_data: Vec<u8> = Vec::new();

    loop {
        let mut new_chunk = read_chunk();
        println!("\nNew chunk read");
        println!("length: {:?}", new_chunk.length);
        println!("type:   {:?}", new_chunk.type_arr);
        /*
        println!("data:");
        for byte in &new_chunk.data {
            print!("{:02x?} ", byte);
        }
        print!("\n");
        */
        if chunk_types_equal(new_chunk.type_arr, IEND_ARR) {
            println!("Read IEND chunk, break from loop");
            // chunks.push(new_chunk);
            break;
        } else if chunk_types_equal(new_chunk.type_arr, IHDR_ARR) {
            println!("Read IHDR chunk");
            //ihdr.data.append(&mut new_chunk.data);
            ihdr_data.append(&mut new_chunk.data);
        } else if chunk_types_equal(new_chunk.type_arr, IDAT_ARR) {
            print!("Read IDAT chunk... ");
            // TODO: take advantage of fact that IDAT chunks must be consecutive
            idat_data.append(&mut new_chunk.data);
        } else {
            println!("Read chunk with unexpected type: {:?}", new_chunk.type_arr);
        }
    }
    println!("Escaped the loop");

    let png_info = parse_ihdr_data(ihdr_data);

    assert!(check_png_info_valid(&png_info));

    let decompressed_data = decompress_data(idat_data);
    println!("Decompressed data from IDAT blocks:");
    for byte in &decompressed_data {
        print!("{:02x?} ", byte);
    }
    print!("\n");

    let expected_size = compute_total_data_bytes(&png_info);
    println!("IDAT decompressed data size equals expected size? {:?}", expected_size == decompressed_data.len());

    let unfiltered_data = unfilter_data(&png_info, decompressed_data);
    println!("Unfiltered the data:");
    for byte in unfiltered_data {
        print!("{:02x?} ", byte);
    }
    print!("\n");

    /*
    let raw_data = match png_info.interlace_method {
        0 => deserialized_data,
        1 => deinterlace_data(&png_info, deserialized_data),
        _ => panic!("Invalid interlace method"),
    };

    let thumbnail_data = generate_thumbnail(&png_info, raw_data);
    // don't bother interlacing output

    */


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
