use std::{marker::PhantomData, ptr::NonNull};

use slotmap::SlotMap;
use string_interner::{StringInterner, backend::StringBackend, symbol::SymbolU32};

pub type StrSymbol = SymbolU32;
pub type Ident = SymbolU32;
pub type Arena<K, T> = SlotMap<K, T>;
pub type TypedArena<T> = typed_arena::Arena<T>;

pub struct Interner {
    interner: StringInterner<StringBackend<StrSymbol>>,
    infer_name: Ident,
}

impl Interner {
    pub fn new() -> Self {
        let mut interner = StringInterner::new();
        let infer_name = interner.get_or_intern_static("_");

        Self {
            interner,
            infer_name,
        }
    }

    pub fn infer_name(&self) -> Ident {
        self.infer_name
    }

    pub fn resolve(&self, symbol: StrSymbol) -> Option<&str> {
        self.interner.resolve(symbol)
    }

    pub fn get_or_intern(&mut self, str: &str) -> StrSymbol {
        self.interner.get_or_intern(str)
    }

    pub fn get_or_intern_static(&mut self, str: &'static str) -> StrSymbol {
        self.interner.get_or_intern_static(str)
    }
}

pub struct RefKey<'a, T> {
    inner: NonNull<T>,
    marker: PhantomData<&'a T>,
}

impl<'a, T> RefKey<'a, T> {
    pub fn new(val_ref: &'a T) -> Self {
        Self {
            inner: NonNull::from_ref(val_ref),
            marker: PhantomData,
        }
    }
}
