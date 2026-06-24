use hex_vm::{Heap, HeapBuf, Trap};

use crate::{
    allocator::{Allocator, Handle},
    gc::shape::GcShapeId,
};

pub const NUM_CLASSES: usize = 36;
pub const MAX_SMALL_SIZE: usize = 1024;

static BLOCK_SIZE: [usize; NUM_CLASSES] = [
    8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288, 320, 352, 384, 416,
    448, 480, 512, 576, 640, 704, 768, 832, 896, 960, 1024,
];

static SIZE_CLASSES: [SizeClass; MAX_SMALL_SIZE + 1] = {
    let mut table = [SizeClass(0); MAX_SMALL_SIZE + 1];
    let mut size = 1usize;
    while size <= MAX_SMALL_SIZE {
        let mut class = 0usize;
        while BLOCK_SIZE[class] < size {
            class += 1;
        }
        table[size] = SizeClass(class as u8);
        size += 1;
    }
    table
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SizeClass(u8);

impl SizeClass {
    pub const LARGE: SizeClass = SizeClass(0xFF);

    #[inline]
    pub const fn class_idx(self) -> usize {
        self.0 as usize
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

pub const PAGE_SIZE: usize = 16 * 1024;
const MIN_BLOCK: usize = 8;
const SLOT_NONE: u16 = u16::MAX;
const HEADER_SIZE: usize = core::mem::size_of::<Page>();

/// Per-class page layout. Each slot costs (block_size + 1): one block
/// plus one state byte. The state array follows the header; block
/// storage follows the state array, 8-aligned.
#[derive(Clone, Copy)]
struct ClassLayout {
    capacity: u16,
    data_off: u16,
}

const fn compute_layout(block_size: usize) -> ClassLayout {
    let mut capacity = (PAGE_SIZE - HEADER_SIZE) / (block_size + 1);
    loop {
        let raw_data = HEADER_SIZE + capacity;
        let data_off = (raw_data + MIN_BLOCK - 1) & !(MIN_BLOCK - 1);
        if data_off + capacity * block_size <= PAGE_SIZE {
            return ClassLayout {
                capacity: capacity as u16,
                data_off: data_off as u16,
            };
        }
        capacity -= 1;
    }
}

const CLASS_LAYOUT: [ClassLayout; NUM_CLASSES] = {
    let mut table = [ClassLayout { capacity: 0, data_off: 0 }; NUM_CLASSES];

    let mut c = 0;
    while c < NUM_CLASSES {
        table[c] = compute_layout(BLOCK_SIZE[c]);
        c += 1;
    }
    table
};

#[inline]
pub const fn block_size(sc: SizeClass) -> usize {
    BLOCK_SIZE[sc.class_idx()]
}

#[inline]
const fn layout(sc: SizeClass) -> ClassLayout {
    CLASS_LAYOUT[sc.class_idx()]
}

#[inline]
const fn block_handle(page_handle: Handle, data_offset: u16, slot: u16, block_size: usize) -> u32 {
    page_handle + data_offset as u32 + (slot as u32 * block_size as u32)
}

/// One byte per slot. Distinguishes the three
/// slot marks for sweep and carries the mark color.
pub mod mark {
    use hex_vm::{Heap, HeapBuf, Trap};

    use crate::{allocator::Handle, gc::page::HEADER_SIZE};

    /// On the free list, holds no object.
    pub const FREED: u8 = 0;
    /// Holds an object, not yet marked this cycle.
    pub const WHITE: u8 = 1;
    /// Holds an object, marked reachable this cycle.
    pub const BLACK: u8 = 2;

    #[inline]
    pub const fn offset(page_handle: Handle, slot: u16) -> u32 {
        page_handle + HEADER_SIZE as u32 + slot as u32
    }

    #[inline]
    pub fn set<B: HeapBuf>(mem: &mut Heap<B>, page: Handle, slot: u16, val: u8) -> Result<(), Trap> {
        mem.write_u8(offset(page, slot), val)
    }

    #[inline]
    pub fn try_mark<B: HeapBuf>(mem: &mut Heap<B>, page: Handle, slot: u16) -> Result<bool, Trap> {
        let mark = mem.get_mut(offset(page, slot), 1)?;
        if mark[0] == WHITE {
            mark[0] = BLACK;
            return Ok(true);
        }
        debug_assert!(mark[0] != FREED);
        Ok(false)
    }
}

#[repr(C)]
pub struct Page {
    freelist_prev: Option<Handle>,
    freelist_next: Option<Handle>,
    pagelist_prev: Option<Handle>,
    pagelist_next: Option<Handle>,

    free_list: u16,
    high_water: u16,
    free_count: u16,
    capacity: u16,
    size_class: SizeClass,
    shape_id: GcShapeId,
}

#[inline(always)]
pub fn page<B: HeapBuf>(mem: &Heap<B>, handle: Handle) -> Result<&Page, Trap> {
    let data = mem.get(handle, PAGE_SIZE as u32)?;
    Ok(unsafe { &*(data.as_ptr().cast()) })
}

#[inline(always)]
pub fn page_mut<B: HeapBuf>(mem: &mut Heap<B>, handle: Handle) -> Result<&mut Page, Trap> {
    let data = mem.get_mut(handle, PAGE_SIZE as u32)?;
    Ok(unsafe { &mut *(data.as_mut_ptr().cast()) })
}

/// Base of the page owning `block`. Relies on PAGE_SIZE alignment.
#[inline]
pub const fn base(block: Handle) -> Handle {
    block & !(PAGE_SIZE as u32 - 1)
}

impl Page {
    #[inline]
    pub fn shape_id(&self) -> GcShapeId {
        self.shape_id
    }

    #[inline]
    pub fn is_full(&self) -> bool {
        self.free_list == SLOT_NONE && self.high_water >= self.capacity
    }

    /// Slot index of `block` within this page.
    #[inline]
    pub fn slot_of(&self, page_base: Handle, block: Handle) -> u16 {
        let l = layout(self.size_class);
        let rel = block - page_base - l.data_off as u32;
        (rel / block_size(self.size_class) as u32) as u16
    }
}

pub struct PageManager {
    freelist: [Option<Handle>; NUM_CLASSES],
    pagelist: Option<Handle>,
}

impl PageManager {
    pub const fn new() -> Self {
        Self { freelist: [None; NUM_CLASSES], pagelist: None }
    }

    pub fn alloc<B: HeapBuf>(
        &mut self,
        mem: &mut Heap<B>,
        allocator: &mut Allocator,
        size_class: SizeClass,
        shape: GcShapeId,
    ) -> Result<Option<Handle>, Trap> {
        debug_assert!(!size_class.is_large());
        self.alloc_small(mem, allocator, size_class, shape)
    }

    /// Allocate one block of `size_class` belonging to `shape`.
    fn alloc_small<B: HeapBuf>(
        &mut self,
        mem: &mut Heap<B>,
        allocator: &mut Allocator,
        size_class: SizeClass,
        shape: GcShapeId,
    ) -> Result<Option<Handle>, Trap> {
        let mut next = self.freelist[size_class.class_idx()];
        while let Some(ph) = next {
            let page = page(mem, ph)?;

            if page.shape_id == shape {
                return Ok(Some(self.alloc_from_page(mem, ph)?));
            }

            next = page.freelist_next;
        }

        let Some(ph) = self.new_page(mem, allocator, size_class, shape)? else {
            return Ok(None);
        };

        Ok(Some(self.alloc_from_page(mem, ph)?))
    }

    /// Pop one block from a page; unlink from class freelist if it fills.
    fn alloc_from_page<B: HeapBuf>(&mut self, mem: &mut Heap<B>, ph: Handle) -> Result<Handle, Trap> {
        let (mut free_list, mut high_water, mut free_count, capacity, sc) = {
            let p = page(mem, ph)?;
            (p.free_list, p.high_water, p.free_count, p.capacity, p.size_class)
        };
        debug_assert!(free_count > 0);

        let block_size = block_size(sc);
        let data_off = layout(sc).data_off;

        let slot;
        let bh;

        if free_list != SLOT_NONE {
            // freed slot's first 2 bytes hold the next free index
            slot = free_list;
            bh = block_handle(ph, data_off, slot, block_size);

            let link = mem.get(bh, 2)?;
            free_list = u16::from_le_bytes([link[0], link[1]]);
        } else {
            debug_assert!(high_water < capacity);
            slot = high_water;
            high_water += 1;
            bh = block_handle(ph, data_off, slot, block_size);
        }

        free_count -= 1;

        mark::set(mem, ph, slot, mark::WHITE)?;

        let page = page_mut(mem, ph)?;
        page.free_list = free_list;
        page.high_water = high_water;
        page.free_count = free_count;

        if free_list == SLOT_NONE && high_water >= capacity {
            self.freelist_remove(mem, ph, sc)?;
        }

        Ok(bh)
    }

    fn new_page<B: HeapBuf>(
        &mut self,
        mem: &mut Heap<B>,
        allocator: &mut Allocator,
        size_class: SizeClass,
        shape: GcShapeId,
    ) -> Result<Option<Handle>, Trap> {
        let Some(ph) = allocator.alloc(mem, PAGE_SIZE, PAGE_SIZE) else {
            return Ok(None);
        };

        let cap = layout(size_class).capacity;

        *page_mut(mem, ph)? = Page {
            freelist_prev: None,
            freelist_next: None,
            pagelist_prev: None,
            pagelist_next: None,
            free_list: SLOT_NONE,
            high_water: 0,
            free_count: cap,
            capacity: cap,
            size_class,
            shape_id: shape,
        };

        // Mark array: fresh slots are FREE until handed out. high_water
        // gates allocation, so explicit init isn't required for correctness,
        // but zero it so sweep can scan [0, high_water) uniformly.
        let mark_off = ph + HEADER_SIZE as u32;
        mem.get_mut(mark_off, cap as u32)?.fill(mark::FREED);

        let page = page_mut(mem, ph)?;
        page.pagelist_prev = None;
        page.pagelist_next = self.pagelist;

        if let Some(head) = self.pagelist {
            page_mut(mem, head)?.pagelist_prev = Some(ph);
        }

        self.pagelist = Some(ph);
        self.freelist_insert(mem, ph, size_class)?;

        Ok(Some(ph))
    }

    fn freelist_insert<B: HeapBuf>(&mut self, mem: &mut Heap<B>, ph: Handle, sc: SizeClass) -> Result<(), Trap> {
        let head = self.freelist[sc.class_idx()];
        let page = page_mut(mem, ph)?;

        debug_assert!(page.freelist_prev.is_none() && page.freelist_next.is_none());

        page.freelist_prev = None;
        page.freelist_next = head;

        if let Some(h) = head {
            page_mut(mem, h)?.freelist_prev = Some(ph);
        }
        self.freelist[sc.class_idx()] = Some(ph);

        Ok(())
    }

    fn freelist_remove<B: HeapBuf>(&mut self, mem: &mut Heap<B>, ph: Handle, sc: SizeClass) -> Result<(), Trap> {
        let page = page_mut(mem, ph)?;
        let prev = page.freelist_prev.take();
        let next = page.freelist_next.take();

        match prev {
            None => self.freelist[sc.class_idx()] = next,
            Some(prev) => page_mut(mem, prev)?.freelist_next = next,
        }

        if let Some(next) = next {
            page_mut(mem, next)?.freelist_prev = prev;
        }

        Ok(())
    }

    pub fn sweep<B: HeapBuf>(&mut self, mem: &mut Heap<B>) -> Result<(), Trap> {
        let mut curr = self.pagelist;
        while let Some(ph) = curr {
            let next = page_mut(mem, ph)?.pagelist_next;
            self.sweep_page(mem, ph)?;
            curr = next;
        }
        Ok(())
    }

    fn sweep_page<B: HeapBuf>(&mut self, mem: &mut Heap<B>, ph: Handle) -> Result<(), Trap> {
        let page = page(mem, ph)?;

        // Cache page state.
        let mut free_list = page.free_list;
        let mut free_count = page.free_count;
        let was_full = page.is_full();

        // Cache page walk data.
        let sc = page.size_class;
        let block_size = block_size(sc);
        let data_off = layout(sc).data_off;

        for slot in (0..page.high_water).rev() {
            let b = mem.get_mut(mark::offset(ph, slot), 1)?;

            match b[0] {
                mark::FREED => continue,
                mark::WHITE => b[0] = mark::FREED, // newly dead
                mark::BLACK => {
                    b[0] = mark::WHITE;
                    continue;
                }
                s => unreachable!("invalid state: {s}"),
            }

            // Add this slot to the free list: write link, push head.
            let bh = block_handle(ph, data_off, slot, block_size);
            let link = mem.get_mut(bh, 2)?;
            link.copy_from_slice(&free_list.to_le_bytes());
            free_list = slot;
            free_count += 1;
        }

        // Update page state.
        let page = page_mut(mem, ph)?;
        page.free_list = free_list;
        page.free_count = free_count;

        // Add to freelist if page wasn't on freelist already.
        if was_full && !page.is_full() {
            self.freelist_insert(mem, ph, sc)?;
        }

        Ok(())
    }
}
