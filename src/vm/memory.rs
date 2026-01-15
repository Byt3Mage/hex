use std::{alloc::Layout, ptr::NonNull};

use simple_ternary::tnr;

use crate::vm::heap::GCHeader;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SizeClass(u8);

impl SizeClass {
    const S8: Self = Self(0);
    const S16: Self = Self(1);
    const S24: Self = Self(2);
    const S32: Self = Self(3);
    const S40: Self = Self(4);
    const S48: Self = Self(5);
    const S56: Self = Self(6);
    const S64: Self = Self(7);
    const S80: Self = Self(8);
    const S96: Self = Self(9);
    const S112: Self = Self(10);
    const S128: Self = Self(11);
    const S144: Self = Self(12);
    const S160: Self = Self(13);
    const S176: Self = Self(14);
    const S192: Self = Self(15);
    const S208: Self = Self(16);
    const S224: Self = Self(17);
    const S240: Self = Self(18);
    const S256: Self = Self(19);
    const S288: Self = Self(20);
    const S320: Self = Self(21);
    const S352: Self = Self(22);
    const S384: Self = Self(23);
    const S416: Self = Self(24);
    const S448: Self = Self(25);
    const S480: Self = Self(26);
    const S512: Self = Self(27);
    const S576: Self = Self(28);
    const S640: Self = Self(29);
    const S704: Self = Self(30);
    const S768: Self = Self(31);
    const S832: Self = Self(32);
    const S896: Self = Self(33);
    const S960: Self = Self(34);
    pub const S1024: Self = Self(35);
    pub const LARGE: Self = Self(0xFF);

    pub const COUNT: usize = Self::S1024.0 as usize + 1;
    pub const MAX_SMALL_SIZE: usize = 1024;

    const BLOCK_SIZES: [usize; Self::COUNT] = [
        8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256,
        288, 320, 352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 832, 896, 960, 1024,
    ];

    const LOOKUP: [Self; Self::MAX_SMALL_SIZE + 1] = {
        let mut table = [Self(0); Self::MAX_SMALL_SIZE + 1];
        let mut size = 1usize;

        while size <= Self::MAX_SMALL_SIZE {
            let class = if size <= 8 {
                0
            } else if size <= 16 {
                1
            } else if size <= 24 {
                2
            } else if size <= 32 {
                3
            } else if size <= 40 {
                4
            } else if size <= 48 {
                5
            } else if size <= 56 {
                6
            } else if size <= 64 {
                7
            } else if size <= 80 {
                8
            } else if size <= 96 {
                9
            } else if size <= 112 {
                10
            } else if size <= 128 {
                11
            } else if size <= 144 {
                12
            } else if size <= 160 {
                13
            } else if size <= 176 {
                14
            } else if size <= 192 {
                15
            } else if size <= 208 {
                16
            } else if size <= 224 {
                17
            } else if size <= 240 {
                18
            } else if size <= 256 {
                19
            } else if size <= 288 {
                20
            } else if size <= 320 {
                21
            } else if size <= 352 {
                22
            } else if size <= 384 {
                23
            } else if size <= 416 {
                24
            } else if size <= 448 {
                25
            } else if size <= 480 {
                26
            } else if size <= 512 {
                27
            } else if size <= 576 {
                28
            } else if size <= 640 {
                29
            } else if size <= 704 {
                30
            } else if size <= 768 {
                31
            } else if size <= 832 {
                32
            } else if size <= 896 {
                33
            } else if size <= 960 {
                34
            } else {
                35
            };

            table[size] = Self(class);
            size += 1;
        }

        table
    };

    #[inline]
    pub const fn as_index(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn block_size(self) -> Option<usize> {
        let idx = self.0 as usize;
        let len = Self::BLOCK_SIZES.len();
        tnr! {idx < len => Some(Self::BLOCK_SIZES[idx]) : None}
    }

    #[inline]
    pub const fn is_large(self) -> bool {
        self.0 == 0xFF
    }

    #[inline]
    pub const fn from_size(size: usize) -> Option<Self> {
        tnr! {
            size == 0 => None :
            size > Self::MAX_SMALL_SIZE => Some(SizeClass::LARGE) :  Some(Self::LOOKUP[size])
        }
    }
}

const PAGE_ALIGN: usize = 16;

pub trait PageAllocator {
    fn alloc_page(&mut self, layout: Layout) -> Option<NonNull<u8>>;
    fn dealloc_page(&mut self, ptr: NonNull<u8>, layout: Layout);
    fn reset(&mut self);
}

pub type PagePtr = NonNull<Page>;

/// A page of memory containing multiple blocks of the same size.
/// Pages are linked in two intrusive linked lists:
/// 1. Freelist - pages with at least one free block (for fast allocation)
/// 2. All-pages list - all pages (for GC sweeping)
#[repr(C)]
pub struct Page {
    // Freelist linkage (pages with free blocks)
    freelist_prev: Option<PagePtr>,
    freelist_next: Option<PagePtr>,

    // All-pages list linkage (for sweeping)
    pagelist_prev: Option<PagePtr>,
    pagelist_next: Option<PagePtr>,

    // Page config
    page_size: u32,
    block_size: u32,
    size_class: SizeClass,
    _padding: [u8; 3],

    // Allocation state
    block_free_list: Option<NonNull<GCHeader>>,
    block_free_next: i32,
    busy_blocks: u32,

    data: [u8; 0],
}

impl Page {
    const _ASSERT: () = assert!(std::mem::size_of::<Self>() % PAGE_ALIGN == 0);
    const HEADER_SIZE: usize = std::mem::size_of::<Self>();
    const SMALL_PAGE_SIZE: usize = 16 * 1024;
    const LARGE_PAGE_SIZE: usize = 32 * 1024;
    const SIZE_THRESHOLD: usize = 512;

    fn page_layout(page_size: usize) -> Layout {
        Layout::from_size_align(page_size, PAGE_ALIGN).unwrap()
    }

    pub fn new<A: PageAllocator>(allocator: &mut A, size_class: SizeClass) -> Option<PagePtr> {
        debug_assert!(!size_class.is_large());

        let block_size = size_class.block_size()?;
        let page_size = if block_size > Self::SIZE_THRESHOLD {
            Self::LARGE_PAGE_SIZE
        } else {
            Self::SMALL_PAGE_SIZE
        };

        Self::new_with_size(allocator, page_size, block_size, size_class)
    }

    pub fn new_large<A: PageAllocator>(allocator: &mut A, block_size: usize) -> Option<PagePtr> {
        let page_size = (Self::HEADER_SIZE + block_size + (PAGE_ALIGN - 1)) & !(PAGE_ALIGN - 1);
        Self::new_with_size(allocator, page_size, block_size, SizeClass::LARGE)
    }

    fn new_with_size<A: PageAllocator>(
        allocator: &mut A,
        page_size: usize,
        block_size: usize,
        size_class: SizeClass,
    ) -> Option<PagePtr> {
        let ptr = allocator.alloc_page(Self::page_layout(page_size))?;
        let data_size = page_size - Self::HEADER_SIZE;
        let block_count = data_size / block_size;
        let last_block_offset = (block_count - 1) * block_size;

        unsafe {
            let page = ptr.cast();
            page.write(Page {
                freelist_prev: None,
                freelist_next: None,
                pagelist_prev: None,
                pagelist_next: None,
                page_size: page_size as u32,
                block_size: block_size as u32,
                size_class,
                _padding: [0; 3],
                block_free_list: None,
                block_free_next: last_block_offset as i32,
                busy_blocks: 0,
                data: [],
            });

            Some(page)
        }
    }

    pub unsafe fn destroy<A: PageAllocator>(page: PagePtr, allocator: &mut A) {
        unsafe {
            let page_ref = page.as_ref();

            debug_assert!(page_ref.busy_blocks == 0);
            debug_assert!(page_ref.freelist_prev.is_none());
            debug_assert!(page_ref.freelist_next.is_none());

            let layout = Self::page_layout(page_ref.page_size as usize);
            allocator.dealloc_page(page.cast(), layout);
        }
    }

    #[inline]
    pub fn pagelist_next(&self) -> Option<PagePtr> {
        self.pagelist_next
    }

    #[inline]
    fn data_ptr(&self) -> *mut u8 {
        self.data.as_ptr().cast_mut()
    }

    #[inline]
    fn page_size(&self) -> usize {
        self.page_size as usize
    }

    #[inline]
    pub fn block_size(&self) -> usize {
        self.block_size as usize
    }

    #[inline]
    pub fn size_class(&self) -> SizeClass {
        self.size_class
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.block_free_list.is_none() && self.block_free_next < 0
    }

    #[inline]
    pub fn has_free(&self) -> bool {
        !self.is_full()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.busy_blocks == 0
    }

    pub unsafe fn alloc(&mut self) -> Option<NonNull<GCHeader>> {
        unsafe {
            if let Some(block) = self.block_free_list {
                self.block_free_list = block.as_ref().freelist_next();
                self.busy_blocks += 1;
                return Some(block);
            }

            if self.block_free_next >= 0 {
                let ptr = self.data_ptr().add(self.block_free_next as usize);
                self.block_free_next -= self.block_size() as i32;
                self.busy_blocks += 1;
                return Some(NonNull::new_unchecked(ptr.cast()));
            }

            None
        }
    }

    pub unsafe fn free(&mut self, mut block_ptr: NonNull<GCHeader>) {
        debug_assert!(self.contains(block_ptr.as_ptr().cast()));
        debug_assert!(self.busy_blocks > 0);

        unsafe {
            let block = block_ptr.as_mut();
            block.mark_free();
            block.set_freelist_next(self.block_free_list);
            self.block_free_list = Some(block_ptr);
            self.busy_blocks -= 1;
        }
    }

    pub fn contains(&self, ptr: *const u8) -> bool {
        let start = self.data_ptr().cast_const();
        let end = unsafe { start.add(self.page_size() - Self::HEADER_SIZE) };
        ptr >= start && ptr < end
    }

    /// Get the walk range for iterating allocated blocks.
    ///
    /// Returns (start, end, busy_blocks) where:
    /// - start: first potentially allocated block (after bump allocator frontier)
    /// - end: one past the last block in data area
    /// - busy_blocks: number of allocated blocks
    ///
    /// The range contains all blocks that have ever been bump-allocated.
    /// Freed blocks within this range have their type set to Free
    /// and should be skipped during iteration.
    pub fn walk_info(&self) -> PageWalkInfo {
        // block_free_next points to the next block to bump-allocate (or negative if exhausted)
        // Blocks from (block_free_next + block_size) to end have been allocated at some point
        let start = (self.block_free_next + self.block_size as i32) as usize;
        let end = self.page_size() - Self::HEADER_SIZE;
        let data_ptr = self.data_ptr();

        unsafe {
            PageWalkInfo {
                start: data_ptr.add(start),
                end: data_ptr.add(end),
                block_size: self.block_size(),
                busy_blocks: self.busy_blocks as usize,
            }
        }
    }

    /// Iterate over all potentially-allocated blocks in this page.
    ///
    /// The iterator yields pointers to GCHeader for each bump-allocated block slot.
    /// Callers MUST check if the block is freed and skip those blocks.
    pub fn blocks(&self) -> BlockIter {
        let info = self.walk_info();
        BlockIter {
            current: info.start,
            end: info.end,
            block_size: info.block_size,
        }
    }
}

/// Information needed to walk a page's blocks
#[derive(Debug, Clone, Copy)]
pub struct PageWalkInfo {
    pub start: *mut u8,
    pub end: *mut u8,
    pub block_size: usize,
    pub busy_blocks: usize,
}

/// Iterator over block slots in a page.
///
/// IMPORTANT: This iterates ALL slots that have been bump-allocated, including
/// freed blocks. Callers must check `GCHeader::is_free()` and skip freed blocks.
pub struct BlockIter {
    current: *mut u8,
    end: *mut u8,
    block_size: usize,
}

impl Iterator for BlockIter {
    type Item = NonNull<GCHeader>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.end {
            return None;
        }

        let ptr = self.current;

        // Safety: ptr is within the page's data area and properly aligned
        unsafe {
            self.current = self.current.add(self.block_size);
            Some(NonNull::new_unchecked(ptr.cast()))
        }
    }
}

/// Manages lists of pages for allocation
pub struct PageManager<A: PageAllocator> {
    allocator: A,

    /// Freelist per size class (pages with free blocks)
    freelist: [Option<PagePtr>; SizeClass::COUNT],

    /// All gcobject pages
    gco_pagelist: Option<PagePtr>,

    /// Total number of pages currently allocated
    total_pages: usize,

    /// Total amount of bytes allocated by pages
    total_bytes: usize,
}

impl<A: PageAllocator> PageManager<A> {
    pub const fn new(allocator: A) -> Self {
        Self {
            allocator,
            freelist: [None; SizeClass::COUNT],
            gco_pagelist: None,
            total_pages: 0,
            total_bytes: 0,
        }
    }

    pub fn total_pages(&self) -> usize {
        self.total_pages
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn gco_pages(&self) -> Option<PagePtr> {
        self.gco_pagelist
    }

    pub fn alloc(&mut self, size: usize) -> Option<NonNull<GCHeader>> {
        let size_class = SizeClass::from_size(size)?;
        tnr! {size_class.is_large() => self.alloc_large(size) : self.alloc_small(size_class)}
    }

    fn alloc_small(&mut self, size_class: SizeClass) -> Option<NonNull<GCHeader>> {
        let index = size_class.as_index();

        unsafe {
            if let Some(mut page_ptr) = self.freelist[index] {
                let page = page_ptr.as_mut();
                let block = page.alloc();

                if page.is_full() {
                    self.freelist_remove(page_ptr, size_class);
                }

                return block;
            }

            let mut page_ptr = Page::new(&mut self.allocator, size_class)?;
            let page = page_ptr.as_mut();

            self.gco_pagelist_insert(page_ptr);
            self.total_pages += 1;
            self.total_bytes += page.page_size();

            let block = page.alloc();

            if page.has_free() {
                self.freelist_insert(page_ptr, size_class);
            }

            block
        }
    }

    fn alloc_large(&mut self, size: usize) -> Option<NonNull<GCHeader>> {
        let mut page_ptr = Page::new_large(&mut self.allocator, size)?;

        unsafe {
            let page = page_ptr.as_mut();

            self.gco_pagelist_insert(page_ptr);
            self.total_pages += 1;
            self.total_bytes += page.page_size();

            page.alloc()
        }
    }

    fn gco_pagelist_insert(&mut self, mut page_ptr: PagePtr) {
        unsafe {
            let page = page_ptr.as_mut();

            debug_assert!(page.pagelist_prev.is_none());
            debug_assert!(page.pagelist_next.is_none());

            page.pagelist_next = self.gco_pagelist;

            if let Some(mut old_head) = self.gco_pagelist {
                old_head.as_mut().pagelist_prev = Some(page_ptr);
            }

            self.gco_pagelist = Some(page_ptr);
        }
    }

    fn gco_pagelist_remove(&mut self, mut page_ptr: PagePtr) {
        unsafe {
            let page = page_ptr.as_mut();

            if let Some(mut prev) = page.pagelist_prev {
                prev.as_mut().pagelist_next = page.pagelist_next;
            } else {
                self.gco_pagelist = page.pagelist_next;
            }

            if let Some(mut next) = page.pagelist_next {
                next.as_mut().pagelist_prev = page.pagelist_prev;
            }

            page.pagelist_prev = None;
            page.pagelist_next = None;
        }
    }

    fn freelist_insert(&mut self, mut page_ptr: PagePtr, size_class: SizeClass) {
        unsafe {
            let page = page_ptr.as_mut();

            debug_assert!(page.freelist_prev.is_none());
            debug_assert!(page.freelist_next.is_none());

            let head = &mut self.freelist[size_class.as_index()];

            page.freelist_next = *head;

            if let Some(mut old_head) = *head {
                old_head.as_mut().freelist_prev = Some(page_ptr);
            }

            *head = Some(page_ptr);
        }
    }

    fn freelist_remove(&mut self, mut page_ptr: PagePtr, size_class: SizeClass) {
        unsafe {
            let page = page_ptr.as_mut();

            if let Some(mut prev) = page.freelist_prev {
                prev.as_mut().freelist_next = page.freelist_next;
            } else {
                // Page is head, replace with next
                self.freelist[size_class.as_index()] = page.freelist_next;
            }

            if let Some(mut next) = page.freelist_next {
                next.as_mut().freelist_prev = page.freelist_prev;
            }

            page.freelist_prev = None;
            page.freelist_next = None;
        }
    }

    /// Free a block back to its page.
    ///
    /// # Safety
    /// - Block must have been allocated by this manager
    /// - Block must not already be freed
    pub unsafe fn free(&mut self, block: NonNull<GCHeader>, mut page_ptr: PagePtr) {
        unsafe {
            let page = page_ptr.as_mut();
            let was_full = page.is_full();
            let size_class = page.size_class();

            page.free(block);

            if was_full && !size_class.is_large() {
                self.freelist_insert(page_ptr, size_class);
            }

            if page.is_empty() {
                self.release_page(page_ptr, size_class);
            }
        }
    }

    fn release_page(&mut self, mut page_ptr: PagePtr, size_class: SizeClass) {
        unsafe {
            let page = page_ptr.as_mut();

            if !size_class.is_large() {
                self.freelist_remove(page_ptr, size_class);
            }

            self.gco_pagelist_remove(page_ptr);
            self.total_pages -= 1;
            self.total_bytes -= page.page_size();

            Page::destroy(page_ptr, &mut self.allocator);
        }
    }

    /// Reset the page manager, deallocating all pages.
    pub fn reset(&mut self) {
        // Free all GCO pages
        let mut page_opt = self.gco_pagelist;
        while let Some(page_ptr) = page_opt {
            unsafe {
                let next = page_ptr.as_ref().pagelist_next();
                Page::destroy(page_ptr, &mut self.allocator);
                page_opt = next;
            };
        }

        // Reset state
        self.freelist = [None; SizeClass::COUNT];
        self.gco_pagelist = None;
        self.total_pages = 0;
        self.total_bytes = 0;
        self.allocator.reset();
    }
}

impl<A: PageAllocator> Drop for PageManager<A> {
    fn drop(&mut self) {
        let mut page_opt = self.gco_pagelist;
        while let Some(page_ptr) = page_opt {
            let next = unsafe { page_ptr.as_ref().pagelist_next() };
            unsafe { Page::destroy(page_ptr, &mut self.allocator) };
            page_opt = next;
        }
    }
}

pub struct BumpAllocator {
    current_chunk: Option<NonNull<u8>>,
    offset: usize,
    end: usize,
    chunks: Vec<Chunk>,
}

struct Chunk {
    ptr: NonNull<u8>,
    layout: Layout,
}

impl BumpAllocator {
    pub fn new() -> Self {
        Self {
            current_chunk: None,
            offset: 0,
            end: 0,
            chunks: vec![],
        }
    }

    #[cold]
    fn alloc_slow(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        const MIN_CHUNK_SIZE: usize = 64 * 1024;
        const MIN_CHUNK_ALIGN: usize = 16;

        let chunk_size = layout.size().max(MIN_CHUNK_SIZE).next_power_of_two();
        let chunk_align = layout.align().max(MIN_CHUNK_ALIGN);
        let chunk_layout = Layout::from_size_align(chunk_size, chunk_align)
            .unwrap()
            .pad_to_align();

        let ptr = NonNull::new(unsafe { std::alloc::alloc(chunk_layout) })?;

        self.current_chunk = Some(ptr);
        self.offset = layout.size();
        self.end = chunk_size;
        self.chunks.push(Chunk {
            ptr,
            layout: chunk_layout,
        });

        Some(ptr)
    }

    #[inline]
    fn reset(&mut self) {
        for chunk in self.chunks.drain(..) {
            unsafe {
                std::alloc::dealloc(chunk.ptr.as_ptr(), chunk.layout);
            }
        }

        self.current_chunk = None;
        self.offset = 0;
        self.end = 0;
    }
}

impl Drop for BumpAllocator {
    fn drop(&mut self) {
        self.reset();
    }
}

#[inline(always)]
const fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

impl PageAllocator for BumpAllocator {
    fn alloc_page(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        if let Some(base) = self.current_chunk {
            let aligned_offset = align_up(self.offset, layout.align());
            let needed = aligned_offset + layout.size();
            if needed <= self.end && needed >= aligned_offset {
                self.offset = needed;
                return unsafe { Some(base.add(aligned_offset)) };
            }
        }

        self.alloc_slow(layout)
    }

    fn dealloc_page(&mut self, _: NonNull<u8>, _: Layout) {}

    fn reset(&mut self) {
        self.reset();
    }
}

pub struct SystemAllocator;

impl PageAllocator for SystemAllocator {
    #[inline]
    fn alloc_page(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        NonNull::new(unsafe { std::alloc::alloc(layout) })
    }

    fn dealloc_page(&mut self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) };
    }

    fn reset(&mut self) {}
}
