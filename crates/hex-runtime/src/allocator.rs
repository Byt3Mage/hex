/// Bump allocator over a VM Memory buffer.
///
/// The single authority for carving raw regions out of Memory. Every
/// subsystem that wants VM-memory-backed storage (the GC heap, the
/// scheduler, channels, etc.) requests regions through here, so that
/// no two subsystems hand out overlapping offsets.
///
/// Allocation is forward-only: the cursor never moves back. Reclamation
/// and reuse are each client's own responsibility (e.g. the GC reuses
/// its own swept pages rather than returning them here).
pub struct Allocator {
    /// Offset of the next unallocated byte in the buffer.
    cursor: usize,
}

impl Allocator {
    /// Create an allocator. `reserved` bytes at the start of the buffer
    /// are left untouched (use 0 if the whole buffer is available).
    pub fn new(reserved: usize) -> Self {
        Self { cursor: reserved }
    }

    /// Current cursor position — the offset of the next free byte.
    #[inline]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Bytes still available in `memory` beyond the cursor.
    #[inline]
    pub fn remaining(&self, mem_size: usize) -> usize {
        mem_size.saturating_sub(self.cursor)
    }

    /// Allocate `size` bytes aligned to `align` (which must be a power of two).
    /// Returns the offset of the region's start.
    ///
    /// The region's contents retain buffer state from previous allocation.
    /// Callers that need zeroed memory must clear it themselves.
    pub fn alloc(&mut self, mem_size: usize, size: usize, align: usize) -> Option<hex_vm::word> {
        assert!(align.is_power_of_two(), "align must be a power of two");
        let ptr = (self.cursor + align - 1) & !(align - 1);
        self.cursor = ptr.checked_add(size).filter(|&e| e <= mem_size)?;
        Some(ptr as hex_vm::word)
    }

    /// Reset the allocator to a fresh state, reserving `reserved` bytes.
    /// Does not touch buffer contents. Callers are responsible for any
    /// invariants tied to previously-handed-out regions.
    pub fn reset(&mut self, reserved: usize) {
        self.cursor = reserved;
    }
}
