use std::ops::Deref;

#[repr(transparent)]
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct Name(u64);

pub struct NameTable {
    strings: Vec<Box<str>>,
}

impl NameTable {
    pub fn new() -> Self {
        Self { strings: vec![] }
    }

    pub fn alloc(&mut self, str: impl AsRef<str>) -> Name {
        let id = self.strings.len();
        self.strings.push(str.as_ref().into());
        Name(id as u64)
    }

    pub fn spelling(&self, name: Name) -> Option<&str> {
        self.strings.get(name.0 as usize).map(Deref::deref)
    }
}
