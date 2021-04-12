/* Fixed Size Block Allocator
 * As described by Phil Opperman in https://os.phil-opp.com/allocator-designs/
 *
 * The fixed size block allocator provides good performance for a range of uses,
 * at the cost of greater internal fragmentation than other allocation schemes.
 * This implementation relies on the linked_list_allocator crate to provide a
 * fallback allocator. Several improvements could be made:
 *  1.  replace this linked list fallback allocator with an allocator based on
 *      paging, which maps a continuous block of virtual memory to non-
 *      continuous physical frames
 *  2.  pre-populate the list heads array with available blocks, ideally in a
 *      way which leverages doing so in bulk and which is specialized to the
 *      application running on the kernel
 *  3.  optimize the block sizes according to the most frequently allocated
 *      data structures or types
 */

use alloc::alloc::Layout;
use core::{mem, ptr::{self, NonNull}};
use super::Locked;
use alloc::alloc::GlobalAlloc;

/// The block sizes to use.
///
/// The sizes must each be a power of 2 because they are also used as the block
/// alignment (alignments must always be powers of 2). If wishing to use blocks
/// that are not powers of 2, define a second BLOCK_ALIGNMENT array.
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 1024, 2048];

struct ListNode {
    next: Option<&'static mut ListNode>,
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: linked_list_allocator::Heap,
}

impl FixedSizeBlockAllocator {
    /// Creates an empty FixedSizeBlockAllocator.
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        FixedSizeBlockAllocator {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: linked_list_allocator::Heap::empty(),
        }
    }

    /// Initialize the allocator with the given heap bounds.
    ///
    /// This function is unsafe because the caller must guarantee that the given
    /// heap bounds are valid and that the heap is unused. This method must be
    /// called only once.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size);
    }

    /// Allocates using the fallback allocator.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.allocate_first_fit(layout) {
            // allocate_first_fit() returns a Result<NonNull<u8>, AllocErr>
            Ok(ptr) => ptr.as_ptr(),    // returns ptr.pointer as *mut u8
            Err(_) => ptr::null_mut(),
        }
    }
}

unsafe impl GlobalAlloc for Locked<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                match allocator.list_heads[index].take() {
                    Some(node) => {
                        allocator.list_heads[index] = node.next.take(); // take() sets pointer to null and returns previous value
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        // no block exists in list => allocate new block
                        let block_size = BLOCK_SIZES[index];
                        // only works if all block sizes are a power of 2
                        let block_align = block_size;
                        let layout = Layout::from_size_align(block_size, block_align).unwrap();
                        allocator.fallback_alloc(layout)
                    }
                }
            }
            None => allocator.fallback_alloc(layout),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let mut allocator = self.lock();
        match list_index(&layout) {
            Some(index) => {
                let new_node = ListNode {
                    next: allocator.list_heads[index].take()
                };
                // verify that block has size and alignment required for storing node
                assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                let new_node_ptr = ptr as *mut ListNode;
                new_node_ptr.write(new_node);
                allocator.list_heads[index] = Some(&mut *new_node_ptr);
            }
            None => {
                let ptr = NonNull::new(ptr).unwrap();
                allocator.fallback_allocator.deallocate(ptr, layout);
            }
        }
    }
}

/// Choose an appropriate block size for the given layout.
///
/// Returns an index into the `BLOCK_SIZES` array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}
