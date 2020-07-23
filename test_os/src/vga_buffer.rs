use volatile::Volatile;
use core::fmt;
use lazy_static::lazy_static;
use spin::Mutex;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]  // Enables copy semantics and make Colors printable and comparable
#[repr(u8)]  // Treat numbers as u8 instead of i32
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]    // Ensures same data layout as u8
struct ColorCode(u8);   // Essentially a typedef for ColorCode type

impl ColorCode{
    fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]  // Guarantees struct field ordering
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

#[repr(transparent)]  // Ensures same data layout as its field
struct Buffer {
    chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
    // Syntax for arrays is [type; count], in this case nested in 2D array.
    // Volatile ensures that this memory cannot be written to without acting
    // through the Volatile class's write() method. In particular, the compiler
    // cannot "optimize" away this write.
}

pub struct Writer {
    column_position: usize,     // Stores current position in row
    color_code: ColorCode,      // Stores current foreground and background color
    buffer: &'static mut Buffer,    // buffer is valid for the whole program run time
}

impl Writer {
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' => self.write_byte(byte),  // printable ASCII byte or newline
                _ => self.write_byte(0xfe),  // not part of printable ASCII range
            }
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;

                let color_code = self.color_code;
                self.buffer.chars[row][col].write(ScreenChar {
                    ascii_character: byte,
                    color_code
                });
                self.column_position += 1;
            }
        }
    }

    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {  // Shift all rows "up" except the current top row
            for col in 0..BUFFER_WIDTH {
                let character = self.buffer.chars[row][col].read();
                self.buffer.chars[row - 1][col].write(character);
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);  // Clear "bottom" row
        self.column_position = 0;
    }
    
    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            self.buffer.chars[row][col].write(blank);
        }
    }
}

// This implements the core::fmt::Write trait, which allows write! and writeln! macros
impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// We want a static variable to keep track of the writer for the VGA buffer,
// but we cannot use non-const functions in a static, and cannot dereference
// a raw pointer, and we cannot have a mutable static without using static mut,
// which is highly unsafe due to race conditions.
// lazy_static allows a lazily initialized static, which is computed when it is
// accessed for the first time (during runtime), rather than at compile time.
// This allows dereferencing raw pointers and using non-constant functions to
// calculate a static variable, which is exactly what we need.
// However, it is still immutable. Thus, we use the spinlock Mutex, which adds
// safe interior mutability to the static WRITER.
lazy_static! {
    pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::LightGray, Color::Black),
        buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },  // This is the one piece of unsafe code
    });
}
// The unsafe code is only run once during initialization, and from then on,
// all operations are safe, and managed by the Mutex.

#[macro_export]  // Macro will be available everywhere in the crate
macro_rules! print{
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
    // Prefixing print! with the $crate:: ensures that we don't need to import
    // the print! macro if we only want to use println!
}

#[doc(hidden)]  // Needs to be public to allow macros to work, but it's internal, so hide it from documentation
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    // write_fmt() is from the core::fmt::Write trait
    WRITER.lock().write_fmt(args).unwrap();
}

#[test_case]
fn test_println_simple() {
    println!("It's a simple spell, but quite unbreakable.");
}

#[test_case]
fn test_println_many() {
    for _ in 0..200 {
        println!("This is getting out of hand. Now there are [many] of them!");
    }
}

#[test_case]
fn test_println_output() {
    let s = "Fear is the little-death that brings total obliteration.";
    println!("{}", s);
    for (i, c) in s.chars().enumerate() {
        let screen_char = WRITER.lock().buffer.chars[BUFFER_HEIGHT - 2][i].read();
        assert_eq!(char::from(screen_char.ascii_character), c);
    }
}
