use std::slice::SliceIndex;

use crate::Value;

pub trait Buffer {
    fn buf(&self) -> &[u8];
    fn buf_mut(&mut self) -> &mut [u8];
}

impl Buffer for &mut [u8] {
    fn buf(&self) -> &[u8] {
        self
    }

    fn buf_mut(&mut self) -> &mut [u8] {
        self
    }
}

impl<const N: usize> Buffer for [u8; N] {
    fn buf(&self) -> &[u8] {
        self
    }

    fn buf_mut(&mut self) -> &mut [u8] {
        self
    }
}

impl Buffer for Vec<u8> {
    fn buf(&self) -> &[u8] {
        self
    }

    fn buf_mut(&mut self) -> &mut [u8] {
        self
    }
}

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum MemoryError {
    #[error("out of bounds memory access")]
    OutOfBounds,
}

pub struct Memory<B: Buffer> {
    buf: B,
}

impl<B: Buffer> Memory<B> {
    pub fn new(buf: B) -> Self {
        Self { buf }
    }

    #[inline(always)]
    pub fn buffer(&self) -> &[u8] {
        self.buf.buf()
    }

    #[inline(always)]
    pub fn buffer_mut(&mut self) -> &mut [u8] {
        self.buf.buf_mut()
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.buf.buf().len()
    }

    #[inline(always)]
    pub fn reset(&mut self) {
        self.buffer_mut().fill(0);
    }

    #[inline(always)]
    fn get<I: SliceIndex<[u8]>>(&self, addr: I) -> Result<&I::Output, MemoryError> {
        match self.buffer().get(addr) {
            Some(s) => Ok(s),
            None => Err(MemoryError::OutOfBounds),
        }
    }

    #[inline(always)]
    fn get_mut<I: SliceIndex<[u8]>>(&mut self, addr: I) -> Result<&mut I::Output, MemoryError> {
        match self.buffer_mut().get_mut(addr) {
            Some(s) => Ok(s),
            None => Err(MemoryError::OutOfBounds),
        }
    }

    #[inline]
    pub fn load_u8(&self, addr: usize) -> Result<u8, MemoryError> {
        self.get(addr).copied()
    }

    #[inline]
    pub fn store_u8(&mut self, addr: usize, v: u8) -> Result<(), MemoryError> {
        Ok(*self.get_mut(addr)? = v)
    }

    #[inline]
    pub fn load_value(&self, addr: usize) -> Result<Value, MemoryError> {
        let mut b = [0u8; 8];
        b.copy_from_slice(self.get(addr..addr + 8)?);
        Ok(Value::from_le_bytes(b))
    }

    #[inline]
    pub fn store_value(&mut self, ptr: usize, v: Value) -> Result<(), MemoryError> {
        let mem = self.get_mut(ptr..ptr + 8)?;
        mem.copy_from_slice(&v.to_le_bytes());
        Ok(())
    }
}
