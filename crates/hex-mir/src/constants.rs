use std::collections::HashMap;

use crate::{ConstVal, vm};

pub struct ConstantPool {
    values: Vec<vm::Value>,
    dedup: HashMap<u64, vm::InstType>,
}

impl ConstantPool {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            dedup: HashMap::new(),
        }
    }

    pub fn insert(&mut self, val: &ConstVal) -> vm::InstType {
        let bits = match val {
            ConstVal::Int(i) => *i as u64,
            ConstVal::Uint(u) => *u as u64,
            ConstVal::Bool(b) => *b as u64,
            ConstVal::Float(f) => f.to_bits(),
        };

        if let Some(&idx) = self.dedup.get(&bits) {
            return idx;
        }
        let idx = self.values.len() as vm::InstType;
        self.dedup.insert(bits, idx);
        self.values.push(vm::Value::from_bits(bits));
        idx
    }

    pub fn into_values(self) -> Vec<vm::Value> {
        self.values
    }
}

impl From<ConstantPool> for Box<[vm::Value]> {
    fn from(pool: ConstantPool) -> Self {
        pool.into_values().into_boxed_slice()
    }
}
