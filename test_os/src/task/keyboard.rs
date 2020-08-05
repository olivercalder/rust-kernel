use conquer_once::spin::OnceCell;
use crossbeam_queue::ArrayQueue;
use crate::{print, println};
use core::{pin::Pin, task::{Poll, Context}};
use futures_util::{stream::{Stream, StreamExt}, task::AtomicWaker};
use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};

static SCANCODE_QUEUE: OnceCell<ArrayQueue<u8>> = OnceCell::uninit();
// using OnceCell instead of lazy_static ensures that initialization does not
// happen in the interrupt handler, thus preventing the interrupt handler from
// performing a heap allocation, which could cause deadlock

static WAKER: AtomicWaker = AtomicWaker::new();

/// Called by the keyboard interrupt handler
///
/// Must not block or allocate.
pub(crate) fn add_scancode(scancode: u8) {  // pub(crate) makes available to lib.rs
    if let Ok(queue) = SCANCODE_QUEUE.try_get() {   // gets reference to queue
        if let Err(_) = queue.push(scancode) {  // performs synchronization and pushes
            println!("WARNING: scancode queue full; dropping keyboard input");
        } else {
            WAKER.wake();   // if a waker is registered, notify the executor; else, no-op
            // this occurs after the scancode has been pushed, so we don't wake with an empty queue
        }
    } else {
        println!("WARNING: scancode queue uninitialized");
    }
}

pub struct ScancodeStream {
    _private: (),   // prevents construction of the struct from outside the module
}

impl ScancodeStream {
    pub fn new() -> Self {
        SCANCODE_QUEUE.try_init_once(|| ArrayQueue::new(128))
            .expect("ScancodeStream::new should only be called once");
        ScancodeStream { _private: () }
    }
}

impl Stream for ScancodeStream {
    type Item = u8;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<u8>> {
        let queue = SCANCODE_QUEUE.try_get().expect("scancode queue not initialized");

        if let Ok(scancode) = queue.pop() {
            return Poll::Ready(Some(scancode));
            // avoid performance overhead of registering a waker when queue is not empty
        }

        WAKER.register(&cx.waker());    // register the waker contained in the Context
        match queue.pop() {
            Ok(scancode) => {
                WAKER.take();   // remove the registered waker
                Poll::Ready(Some(scancode))
            }
            Err(crossbeam_queue::PopError) => Poll::Pending,    // queue remains empty
        }
    }
}

pub async fn print_keypresses() {
    let mut scancodes = ScancodeStream::new();
    let mut keyboard = Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore);

    while let Some(scancode) = scancodes.next().await { // next() method from StreamExt trait
        if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
            if let Some(key) = keyboard.process_keyevent(key_event) {
                match key {
                    DecodedKey::Unicode(character) => print!("{}", character),
                    DecodedKey::RawKey(key) => print!("{:?}", key),
                }
            }
        }
        // since poll_next (called by scancodes.next()) never returns None, this is an endless loop
    }
}
