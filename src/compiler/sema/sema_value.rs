use std::rc::Rc;

use ahash::AHashMap;

use crate::{
    arena::{Arena, Ident, StrSymbol},
    compiler::{ast::DeclId, sema::sema_type::SemaTypeId},
};

slotmap::new_key_type! {
    pub struct SemaValueId;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signedness {
    Signed,
    Unsigned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComptimeInt {
    sign: Signedness,
    value: u64,
}

impl ComptimeInt {
    pub fn from_lit(value: u64) -> Self {
        Self {
            sign: Signedness::Unsigned,
            value,
        }
    }

    pub fn negate(&self) -> Self {
        if self.value == 0 {
            return *self;
        }

        Self {
            sign: match self.sign {
                Signedness::Signed => Signedness::Unsigned,
                Signedness::Unsigned => Signedness::Signed,
            },
            value: self.value,
        }
    }

    pub fn get_unsigned(&self) -> Option<u64> {
        (self.sign == Signedness::Unsigned).then_some(self.value)
    }

    pub fn get_signed(&self) -> Option<i64> {
        const MAX: u64 = i64::MAX as u64;
        const MIN: u64 = i64::MAX as u64 + 1;

        match self.sign {
            Signedness::Signed if self.value > MAX => Some(self.value as i64),
            Signedness::Unsigned if self.value > MIN => Some(-(self.value as i64)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SemaValue {
    ComptimeInt(ComptimeInt),
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
        Self::ComptimeInt(ComptimeInt::from_lit(value))
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
