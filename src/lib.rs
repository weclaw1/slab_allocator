// #![feature(alloc, allocator_api)]
#![feature(allocator_api)]
// #![feature(const_fn)]
#![no_std]

extern crate alloc;

extern crate spin;

extern crate linked_list_allocator;

mod slab;

use core::ops::Deref;

use alloc::alloc::{Allocator, AllocError, Layout};
use core::alloc::GlobalAlloc;
use core::ptr::NonNull;
use core::convert::TryInto;
use slab::Slab;

use spin::Mutex;

#[cfg(test)]
mod test;

pub const NUM_OF_SLABS: usize = 8;
pub const MIN_SLAB_SIZE: usize = 4096;
pub const MIN_HEAP_SIZE: usize = NUM_OF_SLABS * MIN_SLAB_SIZE;

#[derive(Copy, Clone)]
pub enum HeapAllocator {
    Slab64Bytes,
    Slab128Bytes,
    Slab256Bytes,
    Slab512Bytes,
    Slab1024Bytes,
    Slab2048Bytes,
    Slab4096Bytes,
    LinkedListAllocator,
}

/// A fixed size heap backed by multiple slabs with blocks of different sizes.
/// Allocations over 4096 bytes are served by linked list allocator.
pub struct Heap {
    slab_64_bytes: Slab,
    slab_128_bytes: Slab,
    slab_256_bytes: Slab,
    slab_512_bytes: Slab,
    slab_1024_bytes: Slab,
    slab_2048_bytes: Slab,
    slab_4096_bytes: Slab,
    linked_list_allocator: linked_list_allocator::Heap,
}

impl Heap {
    /// Creates a new heap with the given `heap_start_addr` and `heap_size`. The start address must be valid
    /// and the memory in the `[heap_start_addr, heap_start_addr + heap_size)` range must not be used for
    /// anything else. This function is unsafe because it can cause undefined behavior if the
    /// given address is invalid.
    pub unsafe fn new(heap_start_addr: usize, heap_size: usize) -> Heap {
        assert!(
            heap_start_addr % 4096 == 0,
            "Start address should be page aligned"
        );
        assert!(
            heap_size >= MIN_HEAP_SIZE,
            "Heap size should be greater or equal to minimum heap size"
        );
        assert!(
            heap_size % MIN_HEAP_SIZE == 0,
            "Heap size should be a multiple of minimum heap size"
        );
        let slab_size = heap_size / NUM_OF_SLABS;
		let mut heap_bottom: u8 = (heap_start_addr + 7 * slab_size).try_into().unwrap();
		let heap_bottom_ptr: *mut u8 = &mut heap_bottom;
        Heap {
            slab_64_bytes: Slab::new(heap_start_addr, slab_size, 64),
            slab_128_bytes: Slab::new(heap_start_addr + slab_size, slab_size, 128),
            slab_256_bytes: Slab::new(heap_start_addr + 2 * slab_size, slab_size, 256),
            slab_512_bytes: Slab::new(heap_start_addr + 3 * slab_size, slab_size, 512),
            slab_1024_bytes: Slab::new(heap_start_addr + 4 * slab_size, slab_size, 1024),
            slab_2048_bytes: Slab::new(heap_start_addr + 5 * slab_size, slab_size, 2048),
            slab_4096_bytes: Slab::new(heap_start_addr + 6 * slab_size, slab_size, 4096),
            linked_list_allocator: linked_list_allocator::Heap::new(
                (heap_bottom_ptr).try_into().unwrap(),
                slab_size,
            ),
        }
    }

    /// Adds memory to the heap. The start address must be valid
    /// and the memory in the `[mem_start_addr, mem_start_addr + heap_size)` range must not be used for
    /// anything else.
    /// In case of linked list allocator the memory can only be extended.
    /// This function is unsafe because it can cause undefined behavior if the
    /// given address is invalid.
    pub unsafe fn grow(&mut self, mem_start_addr: usize, mem_size: usize, slab: HeapAllocator) {
        match slab {
            HeapAllocator::Slab64Bytes => self.slab_64_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::Slab128Bytes => self.slab_128_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::Slab256Bytes => self.slab_256_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::Slab512Bytes => self.slab_512_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::Slab1024Bytes => self.slab_1024_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::Slab2048Bytes => self.slab_2048_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::Slab4096Bytes => self.slab_4096_bytes.grow(mem_start_addr, mem_size),
            HeapAllocator::LinkedListAllocator => self.linked_list_allocator.extend(mem_size),
        }
    }

    /// Allocates a chunk of the given size with the given alignment. Returns a pointer to the
    /// beginning of that chunk if it was successful. Else it returns `Err`.
    /// This function finds the slab of lowest size which can still accomodate the given chunk.
    /// The runtime is in `O(1)` for chunks of size <= 4096, and `O(n)` when chunk size is > 4096,
    pub fn allocate(&mut self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        match Heap::layout_to_allocator(&layout) {
            HeapAllocator::Slab64Bytes => self.slab_64_bytes.allocate(layout),
            HeapAllocator::Slab128Bytes => self.slab_128_bytes.allocate(layout),
            HeapAllocator::Slab256Bytes => self.slab_256_bytes.allocate(layout),
            HeapAllocator::Slab512Bytes => self.slab_512_bytes.allocate(layout),
            HeapAllocator::Slab1024Bytes => self.slab_1024_bytes.allocate(layout),
            HeapAllocator::Slab2048Bytes => self.slab_2048_bytes.allocate(layout),
            HeapAllocator::Slab4096Bytes => self.slab_4096_bytes.allocate(layout),
			HeapAllocator::LinkedListAllocator => {
				let result =
				self.linked_list_allocator.allocate_first_fit(layout).map_err(|_|
				core::alloc::AllocError)?;
				Ok(NonNull::slice_from_raw_parts(result, layout.size()))
			}
        }
    }

    /// Frees the given allocation. `ptr` must be a pointer returned
    /// by a call to the `allocate` function with identical size and alignment. Undefined
    /// behavior may occur for invalid arguments, thus this function is unsafe.
    ///
    /// This function finds the slab which contains address of `ptr` and adds the blocks beginning
    /// with `ptr` address to the list of free blocks.
    /// This operation is in `O(1)` for blocks <= 4096 bytes and `O(n)` for blocks > 4096 bytes.
    pub unsafe fn deallocate(&mut self, ptr: NonNull<u8>, layout: Layout) {
        match Heap::layout_to_allocator(&layout) {
            HeapAllocator::Slab64Bytes => self.slab_64_bytes.deallocate(ptr),
            HeapAllocator::Slab128Bytes => self.slab_128_bytes.deallocate(ptr),
            HeapAllocator::Slab256Bytes => self.slab_256_bytes.deallocate(ptr),
            HeapAllocator::Slab512Bytes => self.slab_512_bytes.deallocate(ptr),
            HeapAllocator::Slab1024Bytes => self.slab_1024_bytes.deallocate(ptr),
            HeapAllocator::Slab2048Bytes => self.slab_2048_bytes.deallocate(ptr),
            HeapAllocator::Slab4096Bytes => self.slab_4096_bytes.deallocate(ptr),
            HeapAllocator::LinkedListAllocator => {
                self.linked_list_allocator.deallocate(ptr, layout)
            }
        }
    }

    /// Returns bounds on the guaranteed usable size of a successful
    /// allocation created with the specified `layout`.
    pub fn usable_size(&self, layout: &Layout) -> (usize, usize) {
        match Heap::layout_to_allocator(&layout) {
            HeapAllocator::Slab64Bytes => (layout.size(), 64),
            HeapAllocator::Slab128Bytes => (layout.size(), 128),
            HeapAllocator::Slab256Bytes => (layout.size(), 256),
            HeapAllocator::Slab512Bytes => (layout.size(), 512),
            HeapAllocator::Slab1024Bytes => (layout.size(), 1024),
            HeapAllocator::Slab2048Bytes => (layout.size(), 2048),
            HeapAllocator::Slab4096Bytes => (layout.size(), 4096),
            HeapAllocator::LinkedListAllocator => (layout.size(), layout.size()),
        }
    }

    ///Finds allocator to use based on layout size and alignment
    pub fn layout_to_allocator(layout: &Layout) -> HeapAllocator {
        if layout.size() > 4096 {
            HeapAllocator::LinkedListAllocator
        } else if layout.size() <= 64 && layout.align() <= 64 {
            HeapAllocator::Slab64Bytes
        } else if layout.size() <= 128 && layout.align() <= 128 {
            HeapAllocator::Slab128Bytes
        } else if layout.size() <= 256 && layout.align() <= 256 {
            HeapAllocator::Slab256Bytes
        } else if layout.size() <= 512 && layout.align() <= 512 {
            HeapAllocator::Slab512Bytes
        } else if layout.size() <= 1024 && layout.align() <= 1024 {
            HeapAllocator::Slab1024Bytes
        } else if layout.size() <= 2048 && layout.align() <= 2048 {
            HeapAllocator::Slab2048Bytes
        } else {
            HeapAllocator::Slab4096Bytes
        }
    }
}

pub struct LockedHeap(Mutex<Option<Heap>>);

impl LockedHeap {
    pub const fn empty() -> LockedHeap {
        LockedHeap(Mutex::new(None))
    }

    pub unsafe fn init(&self, heap_start_addr: usize, size: usize) {
        *self.0.lock() = Some(Heap::new(heap_start_addr, size));
    }

    /// Creates a new heap with the given `heap_start_addr` and `heap_size`. The start address must be valid
    /// and the memory in the `[heap_start_addr, heap_bottom + heap_size)` range must not be used for
    /// anything else. This function is unsafe because it can cause undefined behavior if the
    /// given address is invalid.
    pub unsafe fn new(heap_start_addr: usize, heap_size: usize) -> LockedHeap {
        LockedHeap(Mutex::new(Some(Heap::new(heap_start_addr, heap_size))))
    }
}

impl Deref for LockedHeap {
    type Target = Mutex<Option<Heap>>;

    fn deref(&self) -> &Mutex<Option<Heap>> {
        &self.0
    }
}

unsafe impl<'a> Allocator for &'a LockedHeap {
    fn allocate(&self, layout: Layout) -> Result<NonNull<[u8]>, AllocError> {
        if let Some(ref mut heap) = *self.0.lock() {
            Ok(heap.allocate(layout)?)
        } else {
            panic!("allocate: heap not initialized");
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        if let Some(ref mut heap) = *self.0.lock() {
            heap.deallocate(ptr, layout)
        } else {
            panic!("deallocate: heap not initialized");
        }
    }
}

impl<'a> LockedHeap {
    fn usable_size(&self, layout: &Layout) -> (usize, usize) {
        if let Some(ref mut heap) = *self.0.lock() {
            heap.usable_size(layout)
        } else {
            panic!("usable_size: heap not initialized");
        }
    }
}

unsafe impl GlobalAlloc for LockedHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if let Some(ref mut heap) = *self.0.lock() {
            if let Ok(ref mut nnptr) = heap.allocate(layout) {
                return nnptr.as_ptr() as *mut u8;
            } else {
                panic!("allocate: failed");
            }
        } else {
            panic!("allocate: heap not initialzied");
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(ref mut heap) = *self.0.lock() {
            if let Some(p) = NonNull::new(ptr) {
                heap.deallocate(p, layout)
            }
        } else {
            panic!("deallocate: heap not initialized");
        }
    }
}
