use hex_vm::{Value, instruction::InstType};
use std::collections::HashMap;

pub struct ConstantPool {
    values: Vec<Value>,
    dedup: HashMap<u64, InstType>,
}

impl ConstantPool {
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            dedup: HashMap::new(),
        }
    }

    pub fn insert(&mut self, bits: u64) -> InstType {
        if let Some(&idx) = self.dedup.get(&bits) {
            return idx;
        }
        let idx = self.values.len() as InstType;
        self.dedup.insert(bits, idx);
        self.values.push(Value::from_bits(bits));
        idx
    }

    pub fn into_values(self) -> Vec<Value> {
        self.values.into()
    }
}
