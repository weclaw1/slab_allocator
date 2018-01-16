use core::ptr::Unique;
use alloc::allocator::{AllocErr, Layout};

pub struct Slab {
    block_size: usize,
    free_block_list: FreeBlockList,
}

impl Slab {
    pub unsafe fn new(start_addr: usize, slab_size: usize, block_size: usize) -> Slab {
        let num_of_blocks = slab_size / block_size;
        Slab {
            block_size: block_size,
            free_block_list: FreeBlockList::new(start_addr, block_size, num_of_blocks),
        }
    }

    pub fn used_blocks(&self) -> usize {
        self.free_block_list.len()
    }

    pub unsafe fn grow(&mut self, start_addr: usize, slab_size: usize) {
        let num_of_blocks = slab_size / self.block_size;
        let mut block_list = FreeBlockList::new(start_addr, self.block_size, num_of_blocks);
        while let Some(block) = block_list.pop() {
            self.free_block_list.push(block);
        }
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        match self.free_block_list.pop() {
            Some(block) => Ok(block.as_ptr() as *mut u8),
            None => Err(AllocErr::Exhausted { request: layout }),
        }
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        self.free_block_list.push(unsafe { Unique::new_unchecked(ptr as *mut FreeBlock) });
    }

}

struct FreeBlockList {
    len: usize,
    head: Option<Unique<FreeBlock>>,
}

impl FreeBlockList {
    unsafe fn new(start_addr: usize, block_size: usize, num_of_blocks: usize) -> FreeBlockList {
        let mut new_list = FreeBlockList::new_empty();
        for i in (0..num_of_blocks).rev() {
            new_list.push(Unique::new_unchecked(
                (start_addr + i * block_size) as *mut FreeBlock,
            ));
        }
        new_list
    }

    fn new_empty() -> FreeBlockList {
        FreeBlockList {
            len: 0,
            head: None,
        }
    }

    fn len(&self) -> usize {
        self.len
    }

    fn pop(&mut self) -> Option<Unique<FreeBlock>> {
        self.head.take().map(|mut node| unsafe {
            self.head = node.as_mut().next;
            self.len -= 1;
            node
        })
    }
 
    fn push(&mut self, mut free_block: Unique<FreeBlock>) {
        unsafe {
            free_block.as_mut().next = self.head.take();
            self.len += 1;
            self.head = Some(free_block);
        }
    }

    fn is_empty(&self) -> bool {
        self.head.is_none()
    }

}

impl Drop for FreeBlockList {
    fn drop(&mut self) {
        while let Some(_) = self.pop() {}
    }
}

struct FreeBlock {
    next: Option<Unique<FreeBlock>>,
    size: usize,
}

impl FreeBlock {
    fn addr(&self) -> usize {
        self as *const _ as usize
    }
}