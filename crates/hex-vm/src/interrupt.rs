use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

/// Shared interrupt flag.
/// Multiple owners can hold a handle; raising is thread-safe and signal-safe.
#[derive(Debug, Default, Clone)]
pub struct InterruptFlag(Arc<AtomicU32>);

impl InterruptFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicU32::new(0)))
    }

    /// Raise an interrupt. Wait-free and signal-safe.
    #[inline]
    pub fn raise(&self, vector: u8) {
        debug_assert!(vector < 32);
        self.0.fetch_or(1u32 << vector, Ordering::Relaxed);
    }

    /// Check pending interrupts. Returns the bitmask of reasons.
    #[inline(always)]
    pub fn pending(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }

    /// Clear a specific reason. Other pending reasons remain set.
    #[inline]
    pub fn clear(&self, vector: u8) {
        self.0.fetch_and(!(1u32 << vector), Ordering::Relaxed);
    }

    /// Clear all interrupts. Returns the previous bitmask.
    #[inline]
    pub fn clear_all(&self) -> u32 {
        self.0.swap(0, Ordering::Relaxed)
    }
}
