use x86_64::{
    structures::paging::{mapper::MapToError, FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

//use linked_list_allocator::LockedHeap;

//pub mod bump;
//use bump::BumpAllocator;

//pub mod linked_list;
//use linked_list::LinkedListAllocator;   // Use the linked_list_allocator crate instead

pub mod fixed_size_block;
use fixed_size_block::FixedSizeBlockAllocator;

#[global_allocator]
//static ALLOCATOR: LockedHeap = LockedHeap::empty(); // uses a spinlock, so do not allocate in interrupt handlers
//static ALLOCATOR: Locked<BumpAllocator> = Locked::new(BumpAllocator::new());
//static ALLOCATOR: Locked<LinkedListAllocator> = Locked::new(LinkedListAllocator::new());
static ALLOCATOR: Locked<FixedSizeBlockAllocator> = Locked::new(FixedSizeBlockAllocator::new());

use crate::memory;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 64 * 1024 * 1024; // Heap has total size of 64MiB
pub const PAGE_TOTAL: usize = HEAP_SIZE / 4096;

pub fn init_heap(
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut memory::BootInfoFrameAllocator,
) -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    let mut frames = frame_allocator.allocate_n_frames(PAGE_TOTAL);

    for page in page_range {
        let frame = frames
            .next()
            .ok_or(MapToError::FrameAllocationFailed)?; // ? unwraps valid values or returns erroneous values
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper.map_to(page, frame, flags, frame_allocator as &mut dyn FrameAllocator<Size4KiB>)?.flush() // ? unwraps valid values or returns erroneous values
        };
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    Ok(())
}

/// A wrapper around spin::Mutex to permit trait implementation.
pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }

    pub fn lock(&self) -> spin::MutexGuard<A> {
        self.inner.lock()
    }
}

/// Align the given address `addr` upwards to alignment `align`.
///
/// Requires that `align` is a power of two.
#[allow(dead_code)]
fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
