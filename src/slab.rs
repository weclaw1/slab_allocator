use core::ptr::Unique;
use core::mem;
use core::marker::PhantomData;
use alloc::allocator::{AllocErr, Layout};

pub struct Slab {
    start_addr: usize,
    slab_size: usize,
    block_size: usize,
    free_block_list: FreeBlockList,
}

impl Slab {
    pub unsafe fn new(start_addr: usize, slab_size: usize, block_size: usize) -> Slab {
        let num_of_blocks = slab_size / block_size;
        Slab {
            start_addr: start_addr,
            slab_size: slab_size,
            block_size: block_size,
            free_block_list: FreeBlockList::new(start_addr, block_size, num_of_blocks),
        }
    }

    pub fn start_addr(&self) -> usize {
        self.start_addr
    }

    pub fn size(&self) -> usize {
        self.slab_size
    }

    pub fn num_of_blocks(&self) -> usize {
        self.slab_size / self.block_size
    }

    pub fn free_blocks(&self) -> usize {
        self.free_block_list.len()
    }

    pub fn used_blocks(&self) -> usize {
        self.num_of_blocks() - self.free_blocks()
    }

    pub fn contains_addr(&self, addr: usize) -> bool {
        if addr >= self.start_addr && addr < self.start_addr + self.slab_size {
            return true;
        }
        false
    }

    pub fn allocate(&mut self, layout: Layout) -> Result<*mut u8, AllocErr> {
        match self.free_block_list.pop_front() {
            Some(block) => Ok(block.as_ptr() as *mut u8),
            None => Err(AllocErr::Exhausted { request: layout }),
        }
    }

    pub fn allocate_multiple(
        &mut self,
        layout: Layout,
        num_of_blocks: usize,
    ) -> Result<*mut u8, AllocErr> {
        if num_of_blocks == 1 {
            return self.allocate(layout);
        }
        match self.free_block_list
            .find_adjacent(self.block_size, num_of_blocks)
        {
            Some(index) => {
                let removed = self.free_block_list.remove(index, num_of_blocks);
                Ok(removed.head.unwrap().as_ptr() as *mut u8)
            }
            None => Err(AllocErr::Exhausted { request: layout }),
        }
    }

    pub fn deallocate(&mut self, ptr: *mut u8) {
        self.free_block_list
            .push_front(unsafe { Unique::new_unchecked(ptr as *mut FreeBlock) });
    }

    pub fn deallocate_multiple(&mut self, ptr: *mut u8, num_of_blocks: usize) {
        let mut new_block_list = FreeBlockList::new_empty();
        for i in 0..num_of_blocks {
            new_block_list.push_back(unsafe {
                Unique::new_unchecked((ptr as usize + i * self.block_size) as *mut FreeBlock)
            });
        }
        self.free_block_list.insert_sorted(new_block_list);
    }
}

struct FreeBlockList {
    len: usize,
    head: Option<Unique<FreeBlock>>,
    tail: Option<Unique<FreeBlock>>,
}

struct Iter<'a> {
    head: Option<Unique<FreeBlock>>,
    tail: Option<Unique<FreeBlock>>,
    len: usize,
    marker: PhantomData<&'a FreeBlock>,
}

struct IterMut<'a> {
    head: Option<Unique<FreeBlock>>,
    tail: Option<Unique<FreeBlock>>,
    len: usize,
    marker: PhantomData<&'a mut FreeBlock>,
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
            len: 0,
            head: None,
            tail: None,
        }
    }

    #[inline]
    fn len(&self) -> usize {
        self.len
    }

    #[inline]
    fn pop_front(&mut self) -> Option<Unique<FreeBlock>> {
        self.head.map(|mut node| unsafe {
            self.head = node.as_mut().next;

            match self.head {
                None => self.tail = None,
                Some(mut head) => head.as_mut().prev = None,
            }

            self.len -= 1;
            node
        })
    }

    #[inline]
    fn pop_back(&mut self) -> Option<Unique<FreeBlock>> {
        self.tail.map(|mut node| unsafe {
            self.tail = node.as_mut().prev;

            match self.tail {
                None => self.head = None,
                Some(mut tail) => tail.as_mut().next = None,
            }

            self.len -= 1;
            node
        })
    }

    #[inline]
    fn push_front(&mut self, mut free_block: Unique<FreeBlock>) {
        unsafe {
            free_block.as_mut().next = self.head;
            free_block.as_mut().prev = None;
            let node = Some(free_block);

            match self.head {
                None => self.tail = node,
                Some(mut head) => head.as_mut().prev = node,
            }

            self.head = node;
            self.len += 1;
        }
    }

    #[inline]
    fn push_back(&mut self, mut free_block: Unique<FreeBlock>) {
        unsafe {
            free_block.as_mut().next = None;
            free_block.as_mut().prev = self.tail;
            let node = Some(free_block);

            match self.tail {
                None => self.head = node,
                Some(mut tail) => tail.as_mut().next = node,
            }

            self.tail = node;
            self.len += 1;
        }
    }

    #[inline]
    fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    #[inline]
    fn front(&self) -> Option<&FreeBlock> {
        unsafe { self.head.as_ref().map(|node| node.as_ref()) }
    }

    #[inline]
    fn back(&self) -> Option<&FreeBlock> {
        unsafe { self.tail.as_ref().map(|node| node.as_ref()) }
    }

    pub fn append(&mut self, other: &mut Self) {
        match self.tail {
            None => mem::swap(self, other),
            Some(mut tail) => {
                if let Some(mut other_head) = other.head.take() {
                    unsafe {
                        tail.as_mut().next = Some(other_head);
                        other_head.as_mut().prev = Some(tail);
                    }

                    self.tail = other.tail.take();
                    self.len += mem::replace(&mut other.len, 0);
                }
            }
        }
    }

    pub fn split_off(&mut self, at: usize) -> FreeBlockList {
        let len = self.len();
        assert!(at <= len, "Cannot split off at a nonexistent index");
        if at == 0 {
            return mem::replace(self, Self::new_empty());
        } else if at == len {
            return Self::new_empty();
        }

        // Below, we iterate towards the `i-1`th node, either from the start or the end,
        // depending on which would be faster.
        let split_node = if at - 1 <= len - 1 - (at - 1) {
            let mut iter = self.iter_mut();
            // instead of skipping using .skip() (which creates a new struct),
            // we skip manually so we can access the head field without
            // depending on implementation details of Skip
            for _ in 0..at - 1 {
                iter.next();
            }
            iter.head
        } else {
            // better off starting from the end
            let mut iter = self.iter_mut();
            for _ in 0..len - 1 - (at - 1) {
                iter.next_back();
            }
            iter.tail
        };

        // The split node is the new tail node of the first part and owns
        // the head of the second part.
        let second_part_head;

        unsafe {
            second_part_head = split_node.unwrap().as_mut().next.take();
            if let Some(mut head) = second_part_head {
                head.as_mut().prev = None;
            }
        }

        let second_part = FreeBlockList {
            head: second_part_head,
            tail: self.tail,
            len: len - at,
        };

        // Fix the tail ptr of the first part
        self.tail = split_node;
        self.len = at;

        second_part
    }

    pub fn iter(&self) -> Iter {
        Iter {
            head: self.head,
            tail: self.tail,
            len: self.len,
            marker: PhantomData,
        }
    }

    pub fn iter_mut(&mut self) -> IterMut {
        IterMut {
            head: self.head,
            tail: self.tail,
            len: self.len,
            marker: PhantomData,
        }
    }

    fn find_adjacent(&self, block_size: usize, num_of_blocks: usize) -> Option<usize> {
        if self.len >= num_of_blocks {
            let mut current_block_index: usize = 0;
            let mut first_block_index: usize = 0;
            let mut adjacent: usize = 1;
            if num_of_blocks == 1 {
                return Some(first_block_index);
            }
            for (current_block, next_block) in self.iter().zip(self.iter().skip(1)) {
                if current_block.addr() + block_size == next_block.addr() {
                    adjacent += 1;
                } else {
                    first_block_index = current_block_index + 1;
                    adjacent = 1;
                }
                if adjacent == num_of_blocks {
                    return Some(first_block_index);
                }
                current_block_index += 1;
            }
        }
        None
    }

    fn remove(&mut self, index: usize, num_of_blocks: usize) -> FreeBlockList {
        let mut split_list = self.split_off(index);
        let mut after_removed = split_list.split_off(num_of_blocks);
        self.append(&mut after_removed);
        split_list
    }

    fn insert_sorted(&mut self, mut free_blocks: FreeBlockList) {
        if free_blocks.len > 0 {
            let mut current_block_index: usize = 0;
            let mut split_index: Option<usize> = None;
            if free_blocks.front().unwrap().addr() > self.back().unwrap().addr() {
                self.append(&mut free_blocks);
                return;
            }
            if free_blocks.back().unwrap().addr() < self.front().unwrap().addr() {
                while let Some(block) = free_blocks.pop_back() {
                    self.push_front(block);
                }
                return;
            }
            for (current_block, next_block) in self.iter().zip(self.iter().skip(1)) {
                if free_blocks.front().unwrap().addr() > current_block.addr()
                    && free_blocks.back().unwrap().addr() < next_block.addr()
                {
                    split_index = Some(current_block_index + 1);
                    break;
                }

                current_block_index += 1;
            }
            if let Some(index) = split_index {
                let mut after_inserted = self.split_off(index);
                self.append(&mut free_blocks);
                self.append(&mut after_inserted);
            }
        }
    }
}

impl Drop for FreeBlockList {
    fn drop(&mut self) {
        while let Some(_) = self.pop_front() {}
    }
}

struct FreeBlock {
    next: Option<Unique<FreeBlock>>,
    prev: Option<Unique<FreeBlock>>,
}

impl FreeBlock {
    fn addr(&self) -> usize {
        self as *const _ as usize
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a FreeBlock;

    #[inline]
    fn next(&mut self) -> Option<&'a FreeBlock> {
        if self.len == 0 {
            None
        } else {
            self.head.map(|node| unsafe {
                // Need an unbound lifetime to get 'a
                let node = &*node.as_ptr();
                self.len -= 1;
                self.head = node.next;
                node
            })
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a> DoubleEndedIterator for Iter<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a FreeBlock> {
        if self.len == 0 {
            None
        } else {
            self.tail.map(|node| unsafe {
                // Need an unbound lifetime to get 'a
                let node = &*node.as_ptr();
                self.len -= 1;
                self.tail = node.prev;
                node
            })
        }
    }
}

impl<'a> Iterator for IterMut<'a> {
    type Item = &'a mut FreeBlock;

    #[inline]
    fn next(&mut self) -> Option<&'a mut FreeBlock> {
        if self.len == 0 {
            None
        } else {
            self.head.map(|node| unsafe {
                // Need an unbound lifetime to get 'a
                let node = &mut *node.as_ptr();
                self.len -= 1;
                self.head = node.next;
                node
            })
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a> DoubleEndedIterator for IterMut<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a mut FreeBlock> {
        if self.len == 0 {
            None
        } else {
            self.tail.map(|node| unsafe {
                // Need an unbound lifetime to get 'a
                let node = &mut *node.as_ptr();
                self.len -= 1;
                self.tail = node.prev;
                node
            })
        }
    }
}
