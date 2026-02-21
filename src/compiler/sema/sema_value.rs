use std::rc::Rc;

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident, StrSymbol},
    compiler::{ast::DeclId, sema::sema_type::SemaTypeId},
};

slotmap::new_key_type! {
    pub struct SemaValueId;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComptimeInt {
    sign: bool, // false = positive, true = negative
    abs: u64,
}

impl ComptimeInt {
    pub fn unsigned(value: u64) -> Self {
        Self {
            sign: false,
            abs: value,
        }
    }

    pub fn signed(value: i64) -> Self {
        Self {
            sign: true,
            abs: value.unsigned_abs(),
        }
    }

    pub fn is_neg(&self) -> bool {
        self.sign
    }

    pub fn negate(&self) -> Self {
        Self {
            sign: !self.sign,
            abs: self.abs,
        }
    }

    pub fn get_unsigned(&self) -> Option<u64> {
        (!self.sign).then_some(self.abs)
    }

    pub fn get_signed(&self) -> Option<i64> {
        const ABS_MAX: u64 = i64::MAX as u64;
        const ABS_MIN: u64 = i64::MAX as u64 + 1;

        match (self.sign, self.abs) {
            (false, v) if v <= ABS_MAX => Some(v as i64),
            (true, v) if v < ABS_MIN => Some(-(v as i64)),
            (true, v) if v == ABS_MIN => Some(i64::MIN),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SemaValue {
    Cint(ComptimeInt),
    Int(i64),
    Uint(u64),
    Bool(bool),
    Float(f64),
    Char(char),
    Str(StrSymbol),
    Null,
    Void,

    Array(Rc<[SemaValueId]>),
    Tuple(Rc<[SemaValueId]>),
    Struct(SemaTypeId, AHashMap<Ident, SemaValueId>),
    Union(SemaTypeId, u64, SemaValueId),
    Variant(SemaTypeId, SemaValueId),
    Function(DeclId),
}

impl SemaValue {
    pub fn from_int_lit(value: u64) -> Self {
        Self::Cint(ComptimeInt::unsigned(value))
    }
}

pub struct ValueArena {
    values: Arena<SemaValueId, SemaValue>,
}

impl ValueArena {
    pub fn new() -> Self {
        Self {
            values: Arena::with_key(),
        }
    }

    pub fn insert(&mut self, val: SemaValue) -> SemaValueId {
        self.values.insert(val)
    }

    pub fn get(&self, id: SemaValueId) -> &SemaValue {
        &self.values[id]
    }

    pub fn get_mut(&mut self, id: SemaValueId) -> &mut SemaValue {
        &mut self.values[id]
    }
}
