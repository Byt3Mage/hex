use std::{ptr::NonNull, usize};

use crate::{async_runtime::Task, memory::*, object::*};

const WHITE0_BIT: u8 = 1 << 0;
const WHITE1_BIT: u8 = 1 << 1;
const BLACK_BIT: u8 = 1 << 2;
const FIXED_BIT: u8 = 1 << 3;
const WHITE_BITS: u8 = WHITE0_BIT | WHITE1_BIT;
const MASK_MARKS: u8 = !(BLACK_BIT | WHITE_BITS);

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockType {
    /// Freed object block, skip during sweep
    Free = 0,
    /// Fixed-size buffer for arrays, structs, ...
    Buffer,
    /// Dynamically-sized buffer for vectors
    DynBuffer,
    /// String data
    String,
    /// Async task
    Task,
}

#[derive(Clone, Copy)]
pub struct GCHeader {
    pub mark: u8,
    pub tt: BlockType,
    pub memcat: u8,
    _pad: [u8; 5],
    payload: [u8; 0],
}

impl GCHeader {
    pub const MIN_BLOCK_SIZE: usize = size_of::<Self>() + size_of::<*mut u8>();

    fn new(mark: u8, tt: BlockType, memcat: u8) -> Self {
        GCHeader {
            mark,
            tt,
            memcat,
            _pad: [0; _],
            payload: [],
        }
    }

    #[inline]
    fn payload_ptr(&self) -> *mut u8 {
        self.payload.as_ptr().cast_mut()
    }

    /// Check if this block has been freed.
    /// Freed blocks have type set to Free and should be skipped during sweep.
    #[inline(always)]
    pub fn is_free(&self) -> bool {
        self.tt == BlockType::Free
    }

    /// Mark this block as freed.
    /// Called when returning a block to the page freelist.
    #[inline(always)]
    pub fn mark_free(&mut self) {
        self.tt = BlockType::Free;
    }

    #[inline]
    pub unsafe fn get_freelist_next(&self) -> Option<NonNull<GCHeader>> {
        unsafe { *self.payload_ptr().cast() }
    }

    #[inline(always)]
    pub unsafe fn set_freelist_next(&mut self, next: Option<NonNull<GCHeader>>) {
        unsafe { *self.payload_ptr().cast() = next };
    }

    #[inline(always)]
    fn object_ptr<T>(&self) -> *mut T {
        self.payload_ptr().cast()
    }

    #[inline]
    pub fn is_white(&self) -> bool {
        (self.mark & WHITE_BITS) != 0
    }

    #[inline]
    pub fn is_black(&self) -> bool {
        (self.mark & BLACK_BIT) != 0
    }

    #[inline]
    pub fn is_gray(&self) -> bool {
        (self.mark & (WHITE_BITS | BLACK_BIT)) == 0
    }

    #[inline]
    pub fn is_fixed(&self) -> bool {
        (self.mark & FIXED_BIT) != 0
    }

    #[inline]
    pub fn set_fixed(&mut self) {
        self.mark |= FIXED_BIT;
    }

    #[inline(always)]
    const fn reset_bit(&mut self, mask: u8) {
        self.mark &= !mask
    }

    #[inline(always)]
    const fn reset_2_bits(&mut self, b1: u8, b2: u8) {
        self.reset_bit(b1 | b2);
    }

    const fn white_to_gray(&mut self) {
        self.reset_bit(WHITE_BITS);
    }

    const fn black_to_gray(&mut self) {
        self.reset_bit(BLACK_BIT);
    }
}

const _: () = assert!(std::mem::size_of::<GCHeader>() == 8);
const _: () = assert!(GCHeader::MIN_BLOCK_SIZE == 16);

#[repr(transparent)]
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct GCPtr(NonNull<GCHeader>);

impl GCPtr {
    #[inline(always)]
    pub fn as_ptr(&self) -> NonNull<GCHeader> {
        self.0
    }

    #[inline(always)]
    fn obj_ptr<T>(&self) -> *mut T {
        self.hdr().payload_ptr().cast()
    }

    #[inline(always)]
    pub fn hdr(&self) -> &GCHeader {
        unsafe { self.0.as_ref() }
    }

    #[inline(always)]
    pub fn hdr_mut(&mut self) -> &mut GCHeader {
        unsafe { self.0.as_mut() }
    }

    #[inline(always)]
    pub(super) fn ty(&self) -> BlockType {
        self.hdr().tt
    }

    pub(super) fn as_ref<T>(&self) -> &T {
        unsafe { &*self.obj_ptr() }
    }

    pub(super) fn as_mut<T>(&mut self) -> &mut T {
        unsafe { &mut *self.obj_ptr() }
    }
}

const GC_GOAL: usize = 200; // 200% - allow heap to double
const GC_STEP_MUL: usize = 200; // GC runs at 2x allocation speed
const GC_STEP_SIZE: usize = 1024; // GC step size in bytes
const GC_SWEEP_PAGE_STEP_COST: usize = 16;
const GC_THRESHOLD_DEFAULT: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GCState {
    Pause,
    Propagate,
    PropagateAgain,
    Atomic,
    Sweep,
}

const MEM_CATEGORY_COUNT: usize = 256;

pub struct MemCategoryStats {
    pub bytes: usize,
    pub objects: usize,
}

pub struct Heap<A: PageAllocator> {
    pages: PageManager<A>,

    // Gray lists for marking
    gray: Option<GCPtr>,
    gray_again: Option<GCPtr>,

    gc_state: GCState,
    current_white: u8,
    sweep_page: Option<PagePtr>,

    gc_threshold: usize,

    memcat_stats: Box<[MemCategoryStats; MEM_CATEGORY_COUNT]>,
}

impl<A: PageAllocator> Heap<A> {
    pub fn new(allocator: A) -> Self {
        Self {
            pages: PageManager::new(allocator),
            gray: None,
            gray_again: None,
            gc_state: GCState::Pause,
            current_white: WHITE0_BIT,
            sweep_page: None,
            gc_threshold: GC_THRESHOLD_DEFAULT,
            memcat_stats: Box::new(
                [const {
                    MemCategoryStats {
                        bytes: 0,
                        objects: 0,
                    }
                }; MEM_CATEGORY_COUNT],
            ),
        }
    }

    #[inline]
    fn track_alloc(&mut self, memcat: u8, size: usize) {
        let stats = &mut self.memcat_stats[memcat as usize];
        stats.bytes += size;
        stats.objects += 1;
    }

    #[inline]
    fn track_free(&mut self, memcat: u8, size: usize) {
        let stats = &mut self.memcat_stats[memcat as usize];
        stats.bytes -= size;
        stats.objects -= 1;
    }

    pub fn memcat_stats(&self, memcat: u8) -> &MemCategoryStats {
        &self.memcat_stats[memcat as usize]
    }

    pub fn alloc_buff(&mut self, len: usize, memcat: u8) -> Option<GCPtr> {
        let size = GCBuffer::block_size(len);
        let block = self.pages.alloc(size)?;

        unsafe {
            block.write(GCHeader::new(self.current_white, BlockType::Buffer, memcat));
            let buff_ptr = block.as_ref().object_ptr::<GCBuffer>();
            buff_ptr.write(GCBuffer::new(len));
        }

        self.track_alloc(memcat, size);
        Some(GCPtr(block))
    }

    pub fn alloc_dyn_buff(&mut self, memcat: u8) -> Option<GCPtr> {
        let size = GCDynBuffer::block_size();
        let block = self.pages.alloc(size)?;

        unsafe {
            block.write(GCHeader::new(
                self.current_white,
                BlockType::DynBuffer,
                memcat,
            ));

            let dyn_buff_ptr = block.as_ref().object_ptr::<GCDynBuffer>();
            dyn_buff_ptr.write(GCDynBuffer::new());
        }

        self.track_alloc(memcat, size);
        Some(GCPtr(block))
    }

    pub fn alloc_str(&mut self, memcat: u8) -> Option<GCPtr> {
        let size = GCString::block_size();
        let block = self.pages.alloc(size)?;

        unsafe {
            block.write(GCHeader::new(self.current_white, BlockType::String, memcat));
            let str_ptr = block.as_ref().object_ptr::<GCString>();
            str_ptr.write(GCString::new());
        }

        self.track_alloc(memcat, size);
        Some(GCPtr(block))
    }

    pub fn alloc_task(&mut self, task: Task, memcat: u8) -> Option<GCPtr> {
        let size = GCTask::block_size();
        let block = self.pages.alloc(size)?;

        unsafe {
            block.write(GCHeader::new(self.current_white, BlockType::Task, memcat));
            let task_ptr = block.as_ref().object_ptr::<GCTask>();
            task_ptr.write(GCTask::new(task));
        }

        self.track_alloc(memcat, size);
        Some(GCPtr(block))
    }

    /// Returns the "other" white (dead white from previous cycle)
    #[inline(always)]
    const fn other_white(&self) -> u8 {
        self.current_white ^ WHITE_BITS
    }

    /// Check if an object is alive (should not be swept)
    #[inline]
    fn is_alive(&self, hdr: &GCHeader) -> bool {
        // Fixed objects are always alive
        // OR
        // Object is alive if it doesn't have the dead (other) white
        hdr.is_fixed() || (hdr.mark ^ WHITE_BITS) & self.other_white() != 0
    }

    fn mark_roots(&mut self, roots: &[Value]) {
        self.gray = None;
        self.gray_again = None;

        for obj in roots.iter().filter_map(try_get_ptr) {
            self.mark_object(obj);
        }

        self.gc_state = GCState::Propagate
    }

    fn mark_object(&mut self, mut obj: GCPtr) {
        let hdr = obj.hdr_mut();

        if !hdr.is_white() {
            return;
        }

        debug_assert!(self.is_alive(hdr), "attempted to mark dead object");

        hdr.white_to_gray();

        match hdr.tt {
            BlockType::String => hdr.mark |= BLACK_BIT,
            BlockType::Buffer => obj.as_mut::<GCBuffer>().gc_list = self.gray.replace(obj),
            BlockType::DynBuffer => obj.as_mut::<GCDynBuffer>().gc_list = self.gray.replace(obj),
            BlockType::Task => obj.as_mut::<GCTask>().gc_list = self.gray.replace(obj),
            BlockType::Free => unreachable!("attempted to mark freed object"),
        }
    }

    /// Propagate mark: pop one gray object, mark it black, and mark its children.
    /// Returns the approximate amount of work done (bytes traversed).
    /// Returns 0 if there's no work to do.
    fn propagate_mark(&mut self) -> usize {
        let Some(mut obj) = self.gray else { return 0 };
        let hdr = obj.hdr_mut();

        debug_assert!(hdr.is_gray());

        hdr.mark |= BLACK_BIT;

        match hdr.tt {
            BlockType::String | BlockType::Free => {
                unreachable!("Non-gray-listable object on gray list")
            }
            BlockType::Buffer => {
                let buff = obj.as_mut::<GCBuffer>();
                self.gray = buff.gc_list.take();
                self.mark_children(buff.as_slice());
                GCBuffer::block_size(buff.len())
            }
            BlockType::DynBuffer => {
                let buff = obj.as_mut::<GCDynBuffer>();
                self.gray = buff.gc_list.take();
                self.mark_children(buff.get());
                GCDynBuffer::block_size() + buff.get().len() * size_of::<Value>()
            }
            BlockType::Task => {
                let task = obj.as_mut::<GCTask>();
                self.gray = task.gc_list.take();
                self.mark_children(&task.get().registers);
                GCTask::block_size() + task.get().registers.len() * size_of::<Value>()
            }
        }
    }

    fn mark_children(&mut self, children: &[Value]) {
        for o in children.iter().filter_map(try_get_ptr) {
            self.mark_object(o)
        }
    }

    /// Run propagation until gray list is empty.
    /// Returns total work done.
    pub fn propagate_all(&mut self) -> usize {
        let mut work = 0;
        while self.gray.is_some() {
            work += self.propagate_mark();
        }
        work
    }

    /// Perform one incremental GC step.
    /// `limit` is the target amount of work to do in bytes.
    ///
    /// Returns the actual work done.
    pub fn gc_step(&mut self, roots: &[Value], limit: usize) -> usize {
        let mut cost = 0;

        match self.gc_state {
            GCState::Pause => self.mark_roots(roots),
            GCState::Propagate => {
                while self.gray.is_some() && cost < limit {
                    cost += self.propagate_mark();
                }

                if self.gray.is_none() {
                    self.gray = self.gray_again.take();
                    self.gc_state = GCState::PropagateAgain;
                }
            }
            GCState::PropagateAgain => {
                while self.gray.is_some() && cost < limit {
                    cost += self.propagate_mark();
                }

                if self.gray.is_none() {
                    self.gc_state = GCState::Atomic;
                }
            }
            GCState::Atomic => {
                cost = self.atomic(roots);
            }
            GCState::Sweep => {
                while let Some(page_ptr) = self.sweep_page
                    && cost < limit
                {
                    // Page sweep might destroy the page
                    let next = unsafe { page_ptr.as_ref().pagelist_next() };
                    let steps = self.sweep_gco_page(page_ptr);

                    self.sweep_page = next;
                    cost += steps * GC_SWEEP_PAGE_STEP_COST;
                }

                if self.sweep_page.is_none() {
                    self.gc_state = GCState::Pause
                }
            }
        }

        return cost;
    }

    fn atomic(&mut self, roots: &[Value]) -> usize {
        let mut work = 0;

        // Re-mark roots, they may have changed during incremental marking
        for obj in roots.iter().filter_map(try_get_ptr) {
            self.mark_object(obj);
        }

        // Remark occasional upvalues of possibly dead threads
        // TODO: work += self.remark_upvals();

        // Traverse any new gray objects from re-marking roots
        work += self.propagate_all();

        // Remark gray again (objects modified by write barriers)
        self.gray = self.gray_again.take();
        work += self.propagate_all();

        // Close orphaned live upvalues of dead threads and clear dead upvalues
        // TODO: work += self.clear_upvals()

        self.current_white = self.other_white();
        self.sweep_page = self.pages.gco_pages();
        self.gc_state = GCState::Sweep;

        work
    }

    fn sweep_gco_page(&mut self, page_ptr: PagePtr) -> usize {
        let page = unsafe { page_ptr.as_ref() };

        if page.is_empty() {
            return 0;
        }

        let mut count = 0;

        for mut block in page.blocks() {
            // SAFETY: Block was allocated in this page
            let hdr = unsafe { block.as_mut() };

            // Skip freed blocks
            if !hdr.is_free() {
                continue;
            }

            count += 1;

            // If alive, recolor to current white for next cycle, otherwise free
            match self.is_alive(hdr) {
                true => hdr.mark = (hdr.mark & MASK_MARKS) | self.current_white,
                false => self.free_object(block, page_ptr),
            }
        }

        count
    }

    /// Free a dead object, dropping any owned resources
    fn free_object(&mut self, block: NonNull<GCHeader>, page_ptr: PagePtr) {
        let hdr = unsafe { block.as_ref() };
        let memcat = hdr.memcat;

        let size = match hdr.tt {
            BlockType::Free => unreachable!("Attempting to free already freed block"),
            BlockType::Buffer => {
                let buff = unsafe { &*(hdr.object_ptr::<GCBuffer>()) };
                GCBuffer::block_size(buff.len())
            }
            BlockType::DynBuffer => {
                let obj_ptr = hdr.object_ptr::<GCDynBuffer>();
                unsafe { std::ptr::drop_in_place(obj_ptr) };
                GCDynBuffer::block_size()
            }
            BlockType::String => {
                let obj_ptr = hdr.payload_ptr().cast::<GCString>();
                unsafe { std::ptr::drop_in_place(obj_ptr) };
                GCString::block_size()
            }
            BlockType::Task => {
                let obj_ptr = hdr.payload_ptr().cast::<GCTask>();
                unsafe { std::ptr::drop_in_place(obj_ptr) };
                GCTask::block_size()
            }
        };

        self.track_free(memcat, size);
        unsafe { self.pages.free(block, page_ptr) };
    }

    /// Forward barrier: when a black object `parent` receives a reference to white object `child`.
    /// Either marks the child (during mark phases) or whitens the parent (during sweep).
    pub fn barrier_forward(&mut self, mut parent: GCPtr, child: GCPtr) {
        let parent_hdr = parent.hdr_mut();
        let child_hdr = child.hdr();

        // Only trigger if black -> white
        if !(parent_hdr.is_black() && child_hdr.is_white()) {
            return;
        }

        debug_assert!(self.is_alive(parent_hdr) && self.is_alive(child_hdr));
        debug_assert!(self.gc_state != GCState::Pause);

        match self.gc_state {
            GCState::Propagate | GCState::PropagateAgain | GCState::Atomic => {
                // Keep invariant during mark phases, by marking the child
                self.mark_object(child)
            }
            _ => {
                // During sweep: just make parent white to avoid repeated barriers
                parent_hdr.mark = (parent_hdr.mark & MASK_MARKS) | self.current_white;
            }
        }
    }

    /// Backward barrier: when a black object is modified, turn it gray and add to grayagain.
    /// Used for container objects like buffers.
    pub fn barrier_back(&mut self, mut obj: GCPtr) {
        let hdr = obj.hdr_mut();

        if !hdr.is_black() {
            return;
        }

        debug_assert!(self.is_alive(hdr));
        debug_assert!(self.gc_state != GCState::Pause);

        hdr.black_to_gray();

        match hdr.tt {
            BlockType::Buffer => obj.as_mut::<GCBuffer>().gc_list = self.gray.replace(obj),
            BlockType::DynBuffer => obj.as_mut::<GCDynBuffer>().gc_list = self.gray.replace(obj),
            BlockType::Task => obj.as_mut::<GCTask>().gc_list = self.gray.replace(obj),
            BlockType::Free | BlockType::String => unreachable!("back bariier on non-container"),
        }
    }

    /// Check if GC needs to run
    #[inline]
    pub fn needs_gc(&self) -> bool {
        self.pages.total_bytes() >= self.gc_threshold
    }

    /// Run a GC step if needed. Call this after allocations.
    /// Returns work done (0 if no GC was needed).
    pub fn check_gc(&mut self, roots: &[Value]) -> usize {
        if self.needs_gc() { self.step(roots) } else { 0 }
    }

    /// Run one GC step.
    /// Returns work done.
    pub fn step(&mut self, roots: &[Value]) -> usize {
        let limit = GC_STEP_SIZE * GC_STEP_MUL / 100;
        let debt = self.pages.total_bytes().saturating_sub(self.gc_threshold);

        let work = self.gc_step(roots, limit);
        let actual_step_size = work * 100 / GC_STEP_MUL;

        if self.gc_state == GCState::Pause {
            // Cycle just finished - set threshold for next cycle
            let live_bytes = self.pages.total_bytes();
            let heap_goal = (live_bytes / 100) * GC_GOAL;

            // Start next cycle when we've allocated halfway to the goal
            self.gc_threshold = live_bytes + (heap_goal - live_bytes) / 2;
        } else {
            // Mid-cycle, allow some allocation before next step
            self.gc_threshold = self.pages.total_bytes() + actual_step_size;
            self.gc_threshold = self.gc_threshold.saturating_sub(debt);
        }

        actual_step_size
    }

    /// Run a full GC cycle (non-incremental).
    pub fn full_gc(&mut self, roots: &[Value]) {
        // If in the middle of a cycle, finish it
        while self.gc_state != GCState::Pause {
            self.gc_step(roots, usize::MAX);
        }

        // Run a complete new cycle
        self.gc_step(roots, usize::MAX);

        while self.gc_state != GCState::Pause {
            self.gc_step(roots, usize::MAX);
        }

        // Set threshold based on live size
        let live_bytes = self.pages.total_bytes();
        let heap_goal = (live_bytes / 100) * GC_GOAL;
        self.gc_threshold = live_bytes + (heap_goal - live_bytes) / 2;
    }

    /// Reset the heap to initial state, freeing all objects.
    /// The heap can be reused after this call.
    ///
    /// **⚠️ Note:** All pointers to objects in this heap become invalid
    pub fn reset(&mut self) {
        let mut sweep_page = self.pages.gco_pages();

        while let Some(page_ptr) = sweep_page {
            let page = unsafe { page_ptr.as_ref() };

            sweep_page = page.pagelist_next();

            for block in page.blocks() {
                let hdr = unsafe { block.as_ref() };

                if hdr.is_free() {
                    continue;
                }

                match hdr.tt {
                    BlockType::Free | BlockType::Buffer => {}
                    BlockType::DynBuffer => {
                        let obj_ptr = hdr.object_ptr::<GCDynBuffer>();
                        unsafe { std::ptr::drop_in_place(obj_ptr) };
                    }
                    BlockType::String => {
                        let obj_ptr = hdr.object_ptr::<GCString>();
                        unsafe { std::ptr::drop_in_place(obj_ptr) };
                    }
                    BlockType::Task => {
                        let obj_ptr = hdr.object_ptr::<GCTask>();
                        unsafe { std::ptr::drop_in_place(obj_ptr) };
                    }
                }
            }
        }

        self.pages.reset();
        self.gray = None;
        self.gray_again = None;
        self.gc_state = GCState::Pause;
        self.current_white = WHITE0_BIT;
        self.sweep_page = None;
        self.gc_threshold = GC_THRESHOLD_DEFAULT;

        for stats in self.memcat_stats.iter_mut() {
            stats.bytes = 0;
            stats.objects = 0;
        }
    }
}

// In heap.rs

impl<A: PageAllocator> Drop for Heap<A> {
    fn drop(&mut self) {
        let mut page_opt = self.pages.gco_pages();
        while let Some(page_ptr) = page_opt {
            let page = unsafe { page_ptr.as_ref() };
            page_opt = page.pagelist_next();

            for block in page.blocks() {
                let hdr = unsafe { block.as_ref() };

                if hdr.is_free() {
                    continue;
                }

                match hdr.tt {
                    BlockType::Free | BlockType::Buffer => {}
                    BlockType::DynBuffer => {
                        let obj_ptr = hdr.payload_ptr().cast::<GCDynBuffer>();
                        unsafe { std::ptr::drop_in_place(obj_ptr) };
                    }
                    BlockType::String => {
                        let obj_ptr = hdr.payload_ptr().cast::<GCString>();
                        unsafe { std::ptr::drop_in_place(obj_ptr) };
                    }
                    BlockType::Task => {
                        let obj_ptr = hdr.payload_ptr().cast::<GCTask>();
                        unsafe { std::ptr::drop_in_place(obj_ptr) };
                    }
                }
            }
        }
    }
}
