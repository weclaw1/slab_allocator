#![feature(unique)]
#![feature(alloc, allocator_api)]
#![feature(const_fn)]
#![feature(attr_literals)]
#![feature(repr_align)]
#![no_std]

extern crate alloc;

extern crate spin;

mod slab;

use core::ops::Deref;

use slab::Slab;
use alloc::allocator::{Alloc, AllocErr, Layout};

use spin::Mutex;

#[cfg(test)]
mod test;

pub const NUM_OF_SLABS: usize = 8;
pub const MIN_SLAB_SIZE: usize = 4096;
pub const MIN_HEAP_SIZE: usize = NUM_OF_SLABS * MIN_SLAB_SIZE;

/// A fixed size heap backed by multiple slabs with blocks of different sizes.
pub struct Heap {
    slab_64_bytes: Slab,
    slab_128_bytes: Slab,
    slab_256_bytes: Slab,
    slab_512_bytes: Slab,
    slab_1024_bytes: Slab,
    slab_2048_bytes: Slab,
    slab_4096_bytes: Slab,
    big_slab: Slab,
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
        Heap {
            slab_64_bytes: Slab::new(heap_start_addr, slab_size, 64),
            slab_128_bytes: Slab::new(heap_start_addr + slab_size, slab_size, 128),
            slab_256_bytes: Slab::new(heap_start_addr + 2 * slab_size, slab_size, 256),
            slab_512_bytes: Slab::new(heap_start_addr + 3 * slab_size, slab_size, 512),
            slab_1024_bytes: Slab::new(heap_start_addr + 4 * slab_size, slab_size, 1024),
            slab_2048_bytes: Slab::new(heap_start_addr + 5 * slab_size, slab_size, 2048),
            slab_4096_bytes: Slab::new(heap_start_addr + 6 * slab_size, slab_size, 4096),
            big_slab: Slab::new_big(heap_start_addr + 7 * slab_size, slab_size),
        }
    }

    /// Adds new memory to the heap with the given `new_mem_start_addr` and `new_mem_size`. 
    /// The start address must be valid and the memory in the `[new_mem_start_addr, new_mem_start_addr + new_mem_size)` 
    /// range must not be used for anything else. This function is unsafe because it can cause undefined behavior if the
    /// given address is invalid.
    pub unsafe fn grow(&mut self, new_mem_start_addr: usize, new_mem_size: usize) {
        assert!(
            new_mem_start_addr % 4096 == 0,
            "Start address should be page aligned"
        );
        assert!(
            new_mem_size >= MIN_HEAP_SIZE,
            "Memory size should be greater or equal to minimum heap size"
        );
        assert!(
            new_mem_size % MIN_HEAP_SIZE == 0,
            "Memory size should be a multiple of minimum heap size"
        );
        let slab_size = new_mem_size / NUM_OF_SLABS;
        self.slab_64_bytes.grow(new_mem_start_addr, slab_size);
        self.slab_128_bytes.grow(new_mem_start_addr + slab_size, slab_size);
        self.slab_256_bytes.grow(new_mem_start_addr + 2 * slab_size, slab_size);
        self.slab_512_bytes.grow(new_mem_start_addr + 3 * slab_size, slab_size);
        self.slab_1024_bytes.grow(new_mem_start_addr + 4 * slab_size, slab_size);
        self.slab_2048_bytes.grow(new_mem_start_addr + 5 * slab_size, slab_size);
        self.slab_4096_bytes.grow(new_mem_start_addr + 6 * slab_size, slab_size);
        self.big_slab.grow_big(new_mem_start_addr + 7 * slab_size, slab_size);
    }

    /// Allocates a chunk of the given size with the given alignment. Returns a pointer to the
    /// beginning of that chunk if it was successful. Else it returns `Err`.
    /// This function finds the slab of lowest size which can still accomodate the given chunk.
    /// The runtime is in `O(1)` for chunks of size <= 4096, and `O(n)` when chunk size is > 4096,
    pub fn allocate(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        if layout.size() <= 64 && layout.align() <= 64 {
            self.slab_64_bytes.allocate(layout)
        } else if layout.size() <= 128 && layout.align() <= 128 {
            self.slab_128_bytes.allocate(layout)
        } else if layout.size() <= 256 && layout.align() <= 256 {
            self.slab_256_bytes.allocate(layout)
        } else if layout.size() <= 512 && layout.align() <= 512 {
            self.slab_512_bytes.allocate(layout)
        } else if layout.size() <= 1024 && layout.align() <= 1024 {
            self.slab_1024_bytes.allocate(layout)
        } else if layout.size() <= 2048 && layout.align() <= 2048 {
            self.slab_2048_bytes.allocate(layout)
        } else if layout.size() <= 4096 && layout.align() <= 4096 {
            self.slab_4096_bytes.allocate(layout)
        } else {
            self.big_slab.allocate_big(layout)
        }
    }

    /// Frees the given allocation. `ptr` must be a pointer returned
    /// by a call to the `allocate` function with identical size and alignment. Undefined
    /// behavior may occur for invalid arguments, thus this function is unsafe.
    ///
    /// This function finds the slab which contains address of `ptr` and adds the blocks beginning
    /// with `ptr` address to the list of free blocks.
    /// This operation is in `O(1)` for blocks of all sizes.
    pub unsafe fn deallocate(&mut self, ptr: *mut u8, layout: Layout) {
        if layout.size() <= 64 && layout.align() <= 64 {
            self.slab_64_bytes.deallocate(ptr)
        } else if layout.size() <= 128 && layout.align() <= 128 {
            self.slab_128_bytes.deallocate(ptr)
        } else if layout.size() <= 256 && layout.align() <= 256 {
            self.slab_256_bytes.deallocate(ptr)
        } else if layout.size() <= 512 && layout.align() <= 512 {
            self.slab_512_bytes.deallocate(ptr)
        } else if layout.size() <= 1024 && layout.align() <= 1024 {
            self.slab_1024_bytes.deallocate(ptr)
        } else if layout.size() <= 2048 && layout.align() <= 2048 {
            self.slab_2048_bytes.deallocate(ptr)
        } else if layout.size() <= 4096 && layout.align() <= 4096 {
            self.slab_4096_bytes.deallocate(ptr)
        } else {
            self.big_slab.deallocate_big(ptr, layout)
        }
    }

    /// Returns bounds on the guaranteed usable size of a successful
    /// allocation created with the specified `layout`.
    pub fn usable_size(&self, layout: &Layout) -> (usize, usize) {
        if layout.size() <= 32 {
            (layout.size(), 32)
        } else if layout.size() <= 64 {
            (layout.size(), 64)
        } else if layout.size() <= 128 {
            (layout.size(), 128)
        } else if layout.size() <= 256 {
            (layout.size(), 256)
        } else if layout.size() <= 512 {
            (layout.size(), 512)
        } else if layout.size() <= 1024 {
            (layout.size(), 1024)
        } else if layout.size() <= 2048 {
            (layout.size(), 2048)
        } else if layout.size() <= 4096 {
            (layout.size(), 4096)
        } else {
            (layout.size(), layout.size())
        }
    }

}

unsafe impl Alloc for Heap {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        self.allocate(layout)
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        self.deallocate(ptr, layout)
    }

    fn oom(&mut self, _: AllocErr) -> ! {
        panic!("Out of memory");
    }

    fn usable_size(&self, layout: &Layout) -> (usize, usize) {
        self.usable_size(layout)
    }
}

pub struct LockedHeap(Mutex<Option<Heap>>);

impl LockedHeap {
    pub const fn empty() -> LockedHeap {
        LockedHeap(Mutex::new(None))
    }

    pub unsafe fn init(&mut self, heap_start_addr: usize, size: usize) {
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

unsafe impl<'a> Alloc for &'a LockedHeap {
    unsafe fn alloc(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        if let Some(ref mut heap) = *self.0.lock() {
            heap.allocate(layout)
        } else {
            panic!("allocate: heap not initialized");
        }
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8, layout: Layout) {
        if let Some(ref mut heap) = *self.0.lock() {
            heap.deallocate(ptr, layout)
        } else {
            panic!("deallocate: heap not initialized");
        }
    }

    fn usable_size(&self, layout: &Layout) -> (usize, usize) {
        if let Some(ref mut heap) = *self.0.lock() {
            heap.usable_size(layout)
        } else {
            panic!("usable_size: heap not initialized");
        }
    }

    fn oom(&mut self, _: AllocErr) -> ! {
        panic!("Out of memory");
    }
}
