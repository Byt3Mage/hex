use crate::{Fault, word};

pub struct NativeCtx<'a> {
    pub window: &'a mut [word],
    pub memory: &'a mut [u8],
}

pub enum NativeOutcome {
    Continue,
    Trap(Fault),
}

pub type NativeFn = fn(NativeCtx<'_>) -> NativeOutcome;
