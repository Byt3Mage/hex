/// A fixed-capacity backing store for a VM stack region.
///
/// The store's length *is* the maximum stack size: the VM never grows it,
/// it only bounds-checks against it and traps on overflow. The blanket impl
/// below covers `Vec<T>`, `[T; N]`, `&mut [T]`, `heapless::Vec`, `ArrayVec`,
/// `smallvec`, and anything else that is `AsRef<[T]> + AsMut<[T]>`.
pub trait Slab<T> {
    fn len(&self) -> usize;
    fn slots(&self) -> &[T];
    fn slots_mut(&mut self) -> &mut [T];
}

impl<T, S: AsRef<[T]> + AsMut<[T]> + ?Sized> Slab<T> for S {
    #[inline(always)]
    fn len(&self) -> usize {
        self.as_ref().len()
    }

    #[inline(always)]
    fn slots(&self) -> &[T] {
        self.as_ref()
    }

    #[inline(always)]
    fn slots_mut(&mut self) -> &mut [T] {
        self.as_mut()
    }
}

/// Conservative upper bound on call-frame depth for a register capacity.
///
/// Each non-tail call advances `base` by at least 1, so depth can never
/// exceed `reg_cap`. Real programs need far fewer; size the frame slab
/// smaller if you know your program's maximum nesting depth.
///
/// Usable in const position, e.g. `[Frame::default(); max_frames(1024)]`.
pub const fn max_frames(reg_cap: usize) -> usize {
    reg_cap
}
