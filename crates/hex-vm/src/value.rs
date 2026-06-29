use std::ops::Deref;

use crate::Reg;

#[allow(non_camel_case_types)]
pub type word = u64;

pub trait AsWord: Copy {
    fn from_word(w: word) -> Self;
    fn into_word(self) -> word;
}

impl AsWord for u64 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u64
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for i64 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as i64
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for usize {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as usize
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for f64 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        f64::from_bits(w)
    }
    #[inline(always)]
    fn into_word(self) -> word {
        f64::to_bits(self)
    }
}

impl AsWord for bool {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w != 0
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for u8 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u8
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for u16 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u16
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for u32 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u32
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct Args<'a>(&'a [word]);

impl<'a> Args<'a> {
    #[inline]
    pub fn new(args: &'a [word]) -> Option<Self> {
        if args.len() <= (Reg::MAX as usize) { Some(Self(args)) } else { None }
    }

    #[inline(always)]
    pub fn count(&self) -> Reg {
        self.len() as Reg
    }
}

impl<'a> Deref for Args<'a> {
    type Target = [word];

    fn deref(&self) -> &Self::Target {
        self.0
    }
}
