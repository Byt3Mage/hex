use crate::{AsWord, Error, Fault, Reg, word};

pub type Syscode = u8;

pub trait Host {
    fn syscall(&mut self, code: Syscode, ctx: HostCtx) -> Result<Flow, Error>;
}

impl Host for () {
    fn syscall(&mut self, _: Syscode, _: HostCtx) -> Result<Flow, Error> {
        unimplemented!("hex_vm: host has no implementation")
    }
}

#[derive(Debug, Copy, Clone)]
pub enum Flow {
    /// Continue VM execution to next instruction.
    Continue,
    /// Suspend VM, reason recorded in host state. [VM::run] returns Suspended.
    Suspend,
    /// Trap VM, VM handles the fault and may unwind.
    Trap(Fault),
}

pub struct HostCtx<'v> {
    pub(crate) regs: &'v mut [word],
    pub(crate) base: usize,
    pub(crate) narg: Reg,
    pub(crate) nret: Reg,
}

impl<'v> HostCtx<'v> {
    #[inline(always)]
    pub fn new(regs: &'v mut [word], base: usize, narg: Reg, nret: Reg) -> Self {
        Self { regs, base, narg, nret }
    }

    #[inline(always)]
    pub fn nargs(&self) -> Reg {
        self.narg
    }

    #[inline(always)]
    pub fn nrets(&self) -> Reg {
        self.nret
    }

    pub fn arg_base(&self) -> usize {
        self.base
    }

    #[inline(always)]
    pub fn args(&self) -> &[word] {
        &self.regs[self.base..self.base + self.narg as usize]
    }

    #[inline(always)]
    pub fn arg_raw(&self, i: Reg) -> Result<word, Error> {
        if i >= self.narg {
            return Err(Error::ArgOutOfBounds { index: i, narg: self.narg });
        }
        Ok(self.regs[self.base + i as usize])
    }

    #[inline(always)]
    pub fn arg<T: AsWord>(&self, i: Reg) -> Result<T, Error> {
        if i >= self.narg {
            return Err(Error::ArgOutOfBounds { index: i, narg: self.narg });
        }
        Ok(T::from_word(self.regs[self.base + i as usize]))
    }

    #[inline(always)]
    pub fn ret_raw(&mut self, i: Reg, v: word) -> Result<(), Error> {
        if i >= self.nret {
            return Err(Error::RetOutOfBounds { index: i, nret: self.nret });
        }
        self.regs[self.base + i as usize] = v;
        Ok(())
    }

    #[inline(always)]
    pub fn ret<T: AsWord>(&mut self, i: Reg, v: T) -> Result<(), Error> {
        self.ret_raw(i, v.into_word())
    }

    /// Write a result slice into the return window (truncated to nret).
    #[inline]
    pub fn rets(&mut self) -> &mut [word] {
        &mut self.regs[self.base..self.base + self.nret as usize]
    }
}
