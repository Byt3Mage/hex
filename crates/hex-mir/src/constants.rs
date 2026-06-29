use std::collections::HashMap;

use crate::{ConstVal, vm};
use vm::AsWord;
pub struct ConstantPool {
    values: Vec<vm::word>,
    dedup: HashMap<u64, vm::Instruction>,
}

impl ConstantPool {
    pub fn new() -> Self {
        Self { values: Vec::new(), dedup: HashMap::new() }
    }

    pub fn insert(&mut self, val: &ConstVal) -> vm::Instruction {
        let word = match val {
            ConstVal::Sint(i) => i64::into_word(*i),
            ConstVal::Uint(u) => u64::into_word(*u),
            ConstVal::Bool(b) => bool::into_word(*b),
            ConstVal::Float(f) => f64::into_word(*f),
        };

        if let Some(&idx) = self.dedup.get(&word) {
            return idx;
        }

        let idx = self.values.len() as vm::Instruction;
        self.dedup.insert(word, idx);
        self.values.push(word);
        idx
    }

    pub fn into_values(self) -> Vec<vm::word> {
        self.values
    }
}

impl From<ConstantPool> for Box<[vm::word]> {
    fn from(pool: ConstantPool) -> Self {
        pool.into_values().into_boxed_slice()
    }
}
