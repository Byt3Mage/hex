use crate::{Error, IsValue, Reg, VM, Value};

pub type Syscode = u8;

pub trait Host {
    fn syscall(&mut self, code: Syscode, ctx: HostCtx) -> Result<Flow, Error>;
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Flow {
    /// Continue VM execution to next instruction.
    Continue,
    /// Suspend VM, reason recorded in host state. [VM::run] returns Suspended.
    Suspend,
}

pub struct HostCtx<'v> {
    pub(crate) vm: &'v mut VM,
    pub(crate) base: usize,
    pub(crate) narg: Reg,
    pub(crate) nret: Reg,
}

impl<'v> HostCtx<'v> {
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
    pub fn args(&self) -> &[Value] {
        &self.vm.registers[self.base..self.base + self.narg as usize]
    }

    #[inline(always)]
    pub fn arg_raw(&self, i: Reg) -> Result<Value, Error> {
        if i >= self.narg {
            return Err(Error::ArgOutOfBounds { index: i, narg: self.narg });
        }
        Ok(self.vm.registers[self.base + i as usize])
    }

    #[inline(always)]
    pub fn arg<T: IsValue>(&self, i: Reg) -> Result<T, Error> {
        if i >= self.narg {
            return Err(Error::ArgOutOfBounds { index: i, narg: self.narg });
        }
        Ok(self.vm.registers[self.base + i as usize].get())
    }

    #[inline(always)]
    pub fn ret_raw(&mut self, i: Reg, v: Value) -> Result<(), Error> {
        if i >= self.nret {
            return Err(Error::RetOutOfBounds { index: i, nret: self.nret });
        }
        self.vm.registers[self.base + i as usize] = v;
        Ok(())
    }

    #[inline(always)]
    pub fn ret<T: IsValue>(&mut self, i: Reg, v: T) -> Result<(), Error> {
        self.ret_raw(i, v.into_value())
    }

    /// Write a result slice into the return window (truncated to nret).
    #[inline]
    pub fn ret_all(&mut self, values: &[Value]) {
        let nret = self.nret as usize;
        let rets = &mut self.vm.registers[self.base..self.base + nret];
        rets.copy_from_slice(&values[..nret]);
    }
}
