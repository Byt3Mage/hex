use std::slice::Iter;

use hex_vm::Value;

use crate::heap::{GCHeader, GCPtr};

#[repr(C)]
pub struct GCBuffer {
    pub gc_list: Option<GCPtr>,
    len: usize,
    data: [Value; 0],
}

impl GCBuffer {
    const _ASSERT: () = assert!(align_of::<Self>() == align_of::<Value>());

    pub fn new(len: usize) -> Self {
        Self {
            gc_list: None,
            len,
            data: [],
        }
    }

    pub const fn block_size(len: usize) -> usize {
        size_of::<GCHeader>() + size_of::<Self>() + (len * size_of::<Value>())
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline(always)]
    pub fn data(&self) -> *mut Value {
        self.data.as_ptr().cast_mut()
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[Value] {
        unsafe { std::slice::from_raw_parts(self.data(), self.len as usize) }
    }

    #[inline(always)]
    pub fn as_slice_mut(&mut self) -> &mut [Value] {
        unsafe { std::slice::from_raw_parts_mut(self.data(), self.len) }
    }

    pub fn iter(&'_ self) -> Iter<'_, Value> {
        self.as_slice().iter()
    }

    #[inline(always)]
    pub fn get(&self, offset: usize) -> Value {
        unsafe { self.data().add(offset).read() }
    }

    #[inline(always)]
    pub fn set(&mut self, offset: usize, value: Value) {
        unsafe { self.data().add(offset).write(value) }
    }
}

#[repr(C)]
pub struct GCDynBuffer {
    pub gc_list: Option<GCPtr>,
    data: Vec<Value>,
}

impl GCDynBuffer {
    pub fn new() -> Self {
        Self {
            gc_list: None,
            data: vec![],
        }
    }

    #[inline(always)]
    pub(super) fn get(&self) -> &Vec<Value> {
        &self.data
    }

    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut Vec<Value> {
        &mut self.data
    }

    #[inline(always)]
    pub fn iter(&self) -> Iter<'_, Value> {
        self.data.iter()
    }

    pub fn block_size() -> usize {
        size_of::<GCHeader>() + size_of::<Self>()
    }
}

#[repr(C)]
pub struct GCString {
    data: String,
}

impl GCString {
    pub fn new() -> Self {
        Self {
            data: String::new(),
        }
    }

    #[inline(always)]
    pub fn get(&self) -> &String {
        &self.data
    }

    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut String {
        &mut self.data
    }

    pub fn block_size() -> usize {
        size_of::<GCHeader>() + size_of::<Self>()
    }
}
