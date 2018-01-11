use core::ptr::Unique;
use alloc::allocator::{AllocErr, Layout};

pub struct Slab {
    block_size: usize,
    big_slab_blocks: usize,
    free_block_list: FreeBlockList,
}

impl Slab {
    pub unsafe fn new(start_addr: usize, slab_size: usize, block_size: usize) -> Slab {
        let num_of_blocks = slab_size / block_size;
        Slab {
            block_size: block_size,
            big_slab_blocks: 0,
            free_block_list: FreeBlockList::new(start_addr, block_size, num_of_blocks),
        }
    }

    pub unsafe fn new_big(start_addr: usize, slab_size: usize) -> Slab {
        let mut slab = Slab {
            block_size: slab_size,
            big_slab_blocks: 1,
            free_block_list: FreeBlockList::new_empty(),
        };
        let mut big_block = Unique::new_unchecked(start_addr as *mut FreeBlock);
        big_block.as_mut().size = slab_size;
        slab.free_block_list.push_back(big_block);
        slab
    }

    pub unsafe fn grow(&mut self, start_addr: usize, slab_size: usize) {
        let num_of_blocks = slab_size / self.block_size;
        let mut block_list = FreeBlockList::new(start_addr, self.block_size, num_of_blocks);
        while let Some(block) = block_list.pop_front() {
            self.free_block_list.push_back(block);
        }
    }

    pub unsafe fn grow_big(&mut self, start_addr: usize, slab_size: usize) {
        let mut big_block = Unique::new_unchecked(start_addr as *mut FreeBlock);
        big_block.as_mut().size = slab_size;
        self.big_slab_blocks += 1;
        self.free_block_list.push_back(big_block);
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        match self.free_block_list.pop_front() {
            Some(block) => Ok(block.as_ptr() as *mut u8),
            None => Err(AllocErr::Exhausted { request: layout }),
        }
    }

    pub fn allocate_big(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        for _ in 0..self.big_slab_blocks {
            let mut block = self.free_block_list.pop_front().unwrap();
            let block_size = unsafe {block.as_ref().size};
            if block_size >= layout.size() {
                self.split_block_until_size(unsafe {block.as_mut()}, layout.size());
                self.big_slab_blocks -= 1;
                return Ok(block.as_ptr() as *mut u8)
            }
            self.free_block_list.push_back(block);
        }
        Err(AllocErr::Exhausted { request: layout })
    }

    fn split_block_until_size(&mut self, block: &mut FreeBlock, size: usize) {
        let power_of_two_size = upper_power_of_two(size);
        let addr = block.addr();
        while block.size > power_of_two_size {
            block.size /= 2;
            let mut new_block = unsafe {Unique::new_unchecked((addr + block.size) as *mut FreeBlock)};
            unsafe {new_block.as_mut().size = block.size};
            self.free_block_list.push_back(new_block);
            self.big_slab_blocks += 1;
        }
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        self.free_block_list.push_back(unsafe { Unique::new_unchecked(ptr as *mut FreeBlock) });
    }

    pub fn deallocate_big(&mut self, ptr: *mut u8, layout: Layout) {
        let mut block = unsafe { Unique::new_unchecked(ptr as *mut FreeBlock) };
        unsafe {block.as_mut().size = upper_power_of_two(layout.size())};
        self.free_block_list.push_back(block);
        self.big_slab_blocks += 1;
    }

}

struct FreeBlockList {
    head: Option<Unique<FreeBlock>>,
    tail: Option<Unique<FreeBlock>>,
}

impl FreeBlockList {
    unsafe fn new(start_addr: usize, block_size: usize, num_of_blocks: usize) -> FreeBlockList {
        let mut new_list = FreeBlockList::new_empty();
        for i in 0..num_of_blocks {
            new_list.push_back(Unique::new_unchecked(
                (start_addr + i * block_size) as *mut FreeBlock,
            ));
        }
        new_list
    }

    fn new_empty() -> FreeBlockList {
        FreeBlockList {
            head: None,
            tail: None,
        }
    }

    #[inline]
    fn pop_front(&mut self) -> Option<Unique<FreeBlock>> {
        self.head.map(|mut node| unsafe {
            self.head = node.as_mut().next;

            if let None = self.head {
                self.tail = None;
            }
            node
        })
    }
 
    #[inline]
    fn push_back(&mut self, mut free_block: Unique<FreeBlock>) {
        unsafe {
            free_block.as_mut().next = None;
            let node = Some(free_block);

            match self.tail {
                None => self.head = node,
                Some(mut tail) => tail.as_mut().next = node,
            }

            self.tail = node;
        }
    }

    fn is_empty(&self) -> bool {
        self.head.is_none()
    }

}

impl Drop for FreeBlockList {
    fn drop(&mut self) {
        while let Some(_) = self.pop_front() {}
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

fn upper_power_of_two(x: usize) -> usize {
    let mut power: usize = 1;
    while power < x {
        power *= 2;
    }
    power
}