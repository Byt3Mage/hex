use std::marker::PhantomData;

use string_interner::{StringInterner, backend::StringBackend, symbol::SymbolU32};

pub type StrSymbol = SymbolU32;
pub type Ident = SymbolU32;

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(u32);

        impl From<u32> for $name {
            fn from(v: u32) -> Self {
                Self(v)
            }
        }

        impl From<$name> for u32 {
            fn from(v: $name) -> u32 {
                v.0
            }
        }
    };
}

pub(crate) use define_id;

#[derive(Debug)]
pub struct Arena<I, T> {
    items: Vec<T>,
    _marker: PhantomData<I>,
}

impl<I, T> Arena<I, T>
where
    I: From<u32> + Into<u32> + Copy,
{
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            _marker: PhantomData,
        }
    }

    pub fn insert(&mut self, item: T) -> I {
        let id = self.items.len();
        self.items.push(item);
        I::from(id as u32)
    }

    pub fn get(&self, id: I) -> &T {
        &self.items[id.into() as usize]
    }

    pub fn get_mut(&mut self, id: I) -> &mut T {
        &mut self.items[id.into() as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.items
            .iter()
            .enumerate()
            .map(|(i, t)| (I::from(i as u32), t))
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl<I, T> std::ops::Index<I> for Arena<I, T>
where
    I: Into<usize> + Copy,
{
    type Output = T;
    fn index(&self, id: I) -> &T {
        &self.items[id.into()]
    }
}

impl<I, T> std::ops::IndexMut<I> for Arena<I, T>
where
    I: Into<usize> + Copy,
{
    fn index_mut(&mut self, id: I) -> &mut T {
        &mut self.items[id.into()]
    }
}

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
