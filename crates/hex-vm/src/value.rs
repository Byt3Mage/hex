use std::ops::Deref;

use crate::{Reg, word};

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Value(word);

impl Value {
    pub const ZERO: Self = Self(0);

    pub const fn from_bits(bits: word) -> Self {
        Self(bits)
    }

    pub const fn to_bits(self) -> word {
        self.0
    }

    pub const fn copy_from_slice(bytes: &[u8]) -> Self {
        let mut b = [0; 8];
        b.copy_from_slice(bytes);
        Self(u64::from_le_bytes(b))
    }

    pub const fn to_le_bytes(self) -> [u8; 8] {
        self.0.to_le_bytes()
    }

    #[inline(always)]
    pub fn get<T: IsValue>(self) -> T {
        T::from_value(self)
    }

    #[inline(always)]
    pub fn set<T: IsValue>(&mut self, v: T) {
        *self = v.into_value();
    }
}

pub trait IsValue: Copy {
    fn from_value(v: Value) -> Self;
    fn into_value(self) -> Value;
}

impl IsValue for usize {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as usize
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as word)
    }
}

impl IsValue for u64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self)
    }
}

impl IsValue for i64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as i64
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as word)
    }
}

impl IsValue for f64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        f64::from_bits(v.0)
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self.to_bits())
    }
}

impl IsValue for bool {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 != 0
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as word)
    }
}

impl IsValue for u8 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u8
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as word)
    }
}

impl IsValue for u16 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u16
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as word)
    }
}

impl IsValue for u32 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u32
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as word)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Args<'a>(&'a [Value]);

impl<'a> Args<'a> {
    #[inline]
    pub fn new(args: &'a [Value]) -> Option<Self> {
        if args.len() <= (Reg::MAX as usize) { Some(Self(args)) } else { None }
    }

    #[inline(always)]
    pub fn count(&self) -> Reg {
        self.len() as Reg
    }
}

impl<'a> Deref for Args<'a> {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        self.0
    }
}
