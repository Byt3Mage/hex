use crate::heap::GCHeader;
use std::{alloc::Layout, ptr::NonNull};

const NUM_CLASSES: usize = 36;
const MAX_SMALL_SIZE: usize = 1024;

static SIZE_CLASSES: [SizeClass; MAX_SMALL_SIZE + 1] = {
    let mut table = [SizeClass(0); MAX_SMALL_SIZE + 1];
    let mut size = 1usize;

    while size <= MAX_SMALL_SIZE {
        let class = match size {
            1..=128 => (size - 1) / 8,
            129..=256 => 16 + (size - 129) / 16,
            257..=512 => 24 + (size - 257) / 32,
            513..=MAX_SMALL_SIZE => 32 + (size - 513) / 64,
            _ => panic!("size not allowed for size class"),
        };

        table[size] = SizeClass(class as u8);
        size += 1;
    }

    table
};

static BLOCK_SIZES: [usize; NUM_CLASSES] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288,
    320, 352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 832, 896, 960, 1024,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
struct SizeClass(u8);

impl SizeClass {
    const LARGE: SizeClass = SizeClass(0xFF);

    #[inline]
    pub const fn class_idx(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn block_size(self) -> usize {
        BLOCK_SIZES[self.class_idx()]
    }

    #[inline]
    pub const fn is_large(self) -> bool {
        self.0 == Self::LARGE.0
    }

    #[inline]
    pub const fn from_size(size: usize) -> Option<Self> {
        match size {
            0 => None,
            1..=MAX_SMALL_SIZE => Some(SIZE_CLASSES[size]),
            _ => Some(Self::LARGE),
        }
    }
}

pub trait HeapAllocator {
    fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>>;
    fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout);
    fn reset(&mut self);
}

pub const PAGE_ALIGN: usize = 16;
pub type PagePtr = NonNull<Page>;

/// A page of memory containing multiple blocks of the same size.
/// Pages are linked in two intrusive linked lists:
/// 1. **Freelist:** Pages with at least one free block (for fast allocation)
/// 2. **All-pages list:** All pages (for GC sweeping)
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
    padding: [u8; 3],

    // Allocation state
    block_free_list: Option<NonNull<GCHeader>>,
    block_free_next: i32,
    busy_blocks: u32,

    data: [u8; 0],
}

impl Page {
    const _ASSERT: () = assert!(std::mem::size_of::<Self>() % PAGE_ALIGN == 0);
    const HEADER_SIZE: usize = std::mem::size_of::<Self>();
    const SIZE_THRESHOLD: usize = 512;
    const SMALL_PAGE_SIZE: usize = 16 * 1024;
    const LARGE_PAGE_SIZE: usize = 32 * 1024;

    fn layout(page_size: usize) -> Layout {
        Layout::from_size_align(page_size, PAGE_ALIGN).unwrap()
    }

    fn new<A: HeapAllocator>(allocator: &mut A, size_class: SizeClass) -> Option<PagePtr> {
        debug_assert!(!size_class.is_large());

        let block_size = size_class.block_size();
        let page_size = if block_size > Self::SIZE_THRESHOLD {
            Self::LARGE_PAGE_SIZE
        } else {
            Self::SMALL_PAGE_SIZE
        };

        Self::new_with_size(allocator, page_size, block_size, size_class)
    }

    fn new_large<A: HeapAllocator>(allocator: &mut A, block_size: usize) -> Option<PagePtr> {
        let page_size = (Self::HEADER_SIZE + block_size + (PAGE_ALIGN - 1)) & !(PAGE_ALIGN - 1);
        Self::new_with_size(allocator, page_size, block_size, SizeClass::LARGE)
    }

    fn new_with_size<A: HeapAllocator>(
        allocator: &mut A,
        page_size: usize,
        block_size: usize,
        size_class: SizeClass,
    ) -> Option<PagePtr> {
        let ptr = allocator.alloc(Self::layout(page_size))?;
        let data_size = page_size - Self::HEADER_SIZE;
        let block_count = data_size / block_size;
        let last_block_offset = (block_count - 1) * block_size;

        unsafe {
            let page_ptr = ptr.cast();

            page_ptr.write(Page {
                freelist_prev: None,
                freelist_next: None,
                pagelist_prev: None,
                pagelist_next: None,
                page_size: page_size as u32,
                block_size: block_size as u32,
                size_class,
                padding: [0; 3],
                block_free_list: None,
                block_free_next: last_block_offset as i32,
                busy_blocks: 0,
                data: [],
            });

            Some(page_ptr)
        }
    }

    pub unsafe fn destroy<A: HeapAllocator>(page_ptr: PagePtr, allocator: &mut A) {
        unsafe {
            let page = page_ptr.as_ref();

            debug_assert!(page.busy_blocks == 0);
            debug_assert!(page.freelist_prev.is_none());
            debug_assert!(page.freelist_next.is_none());

            allocator.dealloc(page_ptr.cast(), Self::layout(page.page_size as usize));
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
    fn size_class(&self) -> SizeClass {
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

    unsafe fn alloc_block(&mut self) -> Option<NonNull<GCHeader>> {
        unsafe {
            if let Some(block) = self.block_free_list {
                self.block_free_list = block.as_ref().get_freelist_next();
                self.busy_blocks += 1;
                return Some(block);
            }

            if self.block_free_next >= 0 {
                let ptr = self.data_ptr().add(self.block_free_next as usize);
                self.block_free_next -= self.block_size as i32;
                self.busy_blocks += 1;
                return Some(NonNull::new_unchecked(ptr.cast()));
            }

            None
        }
    }

    unsafe fn free_block(&mut self, mut block_ptr: NonNull<GCHeader>) {
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

    #[cfg(debug_assertions)]
    pub fn contains(&self, ptr: *const u8) -> bool {
        let start = self.data_ptr().cast_const();
        let end = unsafe { start.add(self.page_size() - Self::HEADER_SIZE) };
        ptr >= start && ptr < end
    }

    /// Iterate over all potentially-allocated blocks in this page.
    ///
    /// The iterator yields pointers to GCHeader for each bump-allocated block slot.
    /// Callers MUST check if the block is freed and skip those blocks.
    pub fn blocks(&self) -> BlockIter {
        let start = (self.block_free_next + self.block_size as i32) as usize;
        let end = self.page_size() - Self::HEADER_SIZE;
        let data_ptr = self.data_ptr();

        unsafe {
            BlockIter {
                current: data_ptr.add(start),
                end: data_ptr.add(end),
                block_size: self.block_size(),
            }
        }
    }
}

/// Iterator over block slots in a page.
///
/// IMPORTANT: This iterates ALL slots that have been bump-allocated, including
/// freed blocks. Callers MUST check `GCHeader::is_free()` and skip freed blocks.
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

        // SAFETY: ptr is within the page's data area and properly aligned
        unsafe {
            self.current = self.current.add(self.block_size);
            Some(NonNull::new_unchecked(ptr.cast()))
        }
    }
}

/// Manages lists of pages for allocation
pub struct PageManager<A: HeapAllocator> {
    allocator: A,

    /// Freelist per size class (pages with free blocks)
    freelist: [Option<PagePtr>; NUM_CLASSES],

    /// All gcobject pages
    pagelist: Option<PagePtr>,

    /// Total number of pages currently allocated
    total_pages: usize,

    /// Total amount of bytes allocated by pages
    total_bytes: usize,
}

impl<A: HeapAllocator> PageManager<A> {
    pub const fn new(allocator: A) -> Self {
        Self {
            allocator,
            freelist: [None; NUM_CLASSES],
            pagelist: None,
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
        self.pagelist
    }

    pub fn alloc(&mut self, size: usize) -> Option<NonNull<GCHeader>> {
        let size_class = SizeClass::from_size(size)?;
        if size_class.is_large() {
            self.alloc_large(size)
        } else {
            self.alloc_small(size_class)
        }
    }

    fn alloc_small(&mut self, size_class: SizeClass) -> Option<NonNull<GCHeader>> {
        unsafe {
            // find a page with free blocks in the freelist for this size class
            if let Some(mut page_ptr) = self.freelist[size_class.class_idx()] {
                let page = page_ptr.as_mut();
                let block = page.alloc_block();

                if page.is_full() {
                    self.freelist_remove(page_ptr, size_class);
                }

                return block;
            }

            // allocate fresh page and allocate from it
            let mut page_ptr = Page::new(&mut self.allocator, size_class)?;
            self.pagelist_insert(page_ptr);

            let page = page_ptr.as_mut();
            self.total_pages += 1;
            self.total_bytes += page.page_size();

            let block = page.alloc_block();

            // should have free blocks, but just in case
            if page.has_free() {
                self.freelist_insert(page_ptr, size_class);
            }

            block
        }
    }

    fn alloc_large(&mut self, size: usize) -> Option<NonNull<GCHeader>> {
        let mut page_ptr = Page::new_large(&mut self.allocator, size)?;
        self.pagelist_insert(page_ptr);

        unsafe {
            let page = page_ptr.as_mut();
            self.total_pages += 1;
            self.total_bytes += page.page_size();

            page.alloc_block()
        }
    }

    fn pagelist_insert(&mut self, mut page_ptr: PagePtr) {
        unsafe {
            let page = page_ptr.as_mut();

            debug_assert!(page.pagelist_prev.is_none());
            debug_assert!(page.pagelist_next.is_none());

            page.pagelist_next = self.pagelist;

            if let Some(mut old_head) = self.pagelist {
                old_head.as_mut().pagelist_prev = Some(page_ptr);
            }

            self.pagelist = Some(page_ptr);
        }
    }

    fn pagelist_remove(&mut self, mut page_ptr: PagePtr) {
        unsafe {
            let page = page_ptr.as_mut();

            match page.pagelist_prev {
                None => self.pagelist = page.pagelist_next,
                Some(mut prev) => prev.as_mut().pagelist_next = page.pagelist_next,
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

            let head = self.freelist[size_class.class_idx()];

            page.freelist_next = head;

            if let Some(mut old_head) = head {
                old_head.as_mut().freelist_prev = Some(page_ptr);
            }

            self.freelist[size_class.class_idx()] = Some(page_ptr);
        }
    }

    /// Remove page from freelist (linked list removal)
    fn freelist_remove(&mut self, mut page_ptr: PagePtr, size_class: SizeClass) {
        unsafe {
            let page = page_ptr.as_mut();

            match page.freelist_prev {
                None => self.freelist[size_class.class_idx()] = page.freelist_next,
                Some(mut prev) => prev.as_mut().freelist_next = page.freelist_next,
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
    /// - Block must belong to the page
    /// - Block must not already be freed
    pub unsafe fn free(&mut self, block: NonNull<GCHeader>, mut page_ptr: PagePtr) {
        unsafe {
            let page = page_ptr.as_mut();
            let was_full = page.is_full();
            let size_class = page.size_class();

            page.free_block(block);

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
            self.pagelist_remove(page_ptr);

            if !size_class.is_large() {
                self.freelist_remove(page_ptr, size_class);
            }

            let page = page_ptr.as_mut();
            self.total_pages -= 1;
            self.total_bytes -= page.page_size();

            Page::destroy(page_ptr, &mut self.allocator);
        }
    }

    /// Reset the page manager, deallocating all pages.
    pub fn reset(&mut self) {
        // Free all GCO pages
        let mut page_opt = self.pagelist;
        while let Some(page_ptr) = page_opt {
            unsafe {
                let next = page_ptr.as_ref().pagelist_next();
                Page::destroy(page_ptr, &mut self.allocator);
                page_opt = next;
            };
        }

        // Reset state
        self.freelist = [None; NUM_CLASSES];
        self.pagelist = None;
        self.total_pages = 0;
        self.total_bytes = 0;
        self.allocator.reset();
    }
}

impl<A: HeapAllocator> Drop for PageManager<A> {
    fn drop(&mut self) {
        let mut page_opt = self.pagelist;
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

impl HeapAllocator for BumpAllocator {
    fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>> {
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

    fn dealloc(&mut self, _: NonNull<u8>, _: Layout) {}

    fn reset(&mut self) {
        self.reset();
    }
}

pub struct SystemAllocator;

impl HeapAllocator for SystemAllocator {
    #[inline]
    fn alloc(&mut self, layout: Layout) -> Option<NonNull<u8>> {
        NonNull::new(unsafe { std::alloc::alloc(layout) })
    }

    fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        unsafe { std::alloc::dealloc(ptr.as_ptr(), layout) };
    }

    fn reset(&mut self) {}
}
