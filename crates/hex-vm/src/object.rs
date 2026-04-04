use std::slice::Iter;

use crate::{
    async_runtime::Task,
    heap::{GCHeader, GCPtr},
    program::FunctionPtr,
};

macro_rules! value {
    ($($field: ident : $ty: path),* $(,)?) => {
        mod private {
            pub trait Sealed {}
        }

        use self::private::Sealed;

        pub trait AsValue: Sealed + Sized {
            const TYPE: ValueType;
            fn from_value(value: Value) -> Self;
            fn into_value(self: Self) -> Value;
            fn try_from_value(value: Value) -> Option<Self>;
        }

        #[repr(u8)]
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub enum ValueType {
            $($field),*
        }

        #[repr(C)]
        #[derive(Clone, Copy)]
        #[allow(non_snake_case)]
        union ValueData {
            $($field: $ty),*
        }

        #[derive(Clone, Copy)]
        pub struct Value {
            data: ValueData,
            ty: ValueType,
        }

        const _: () = assert!(align_of::<Value>() == align_of::<u64>());

        impl Value {
            #[inline(always)]
            pub fn zero() -> Self {
                unsafe { std::mem::zeroed::<Self>() }
            }

            #[inline(always)]
            pub fn get<T: AsValue>(self) -> T {
                T::from_value(self)
            }

            #[inline(always)]
            pub fn try_get<T: AsValue>(self) -> Option<T> {
                T::try_from_value(self)
            }

            #[inline(always)]
            pub fn set(&mut self, v: impl AsValue) {
                *self = v.into_value();
            }

            #[inline(always)]
            pub fn ty(&self) -> ValueType {
                self.ty
            }
        }

        $(
            impl Sealed for $ty {}
            impl AsValue for $ty {
                const TYPE: ValueType = ValueType::$field;

                fn from_value(value: Value) -> Self {
                    unsafe { value.data.$field }
                }

                fn into_value(self: Self) -> Value {
                    Value {
                        data: ValueData { $field: self },
                        ty: Self::TYPE,
                    }
                }

                fn try_from_value(value: Value) -> Option<Self> {
                    (value.ty == Self::TYPE).then_some(unsafe { value.data.$field })
                }
            }
        )*
    };
}

value! {
    Ptr: GCPtr,
    Int: i64,
    Uint: u64,
    Bool: bool,
    Float: f64,
    Char: char,
    //Str: StrSymbol,
    Func: FunctionPtr,

    // OS-specific size types
    Isize: isize,
    Usize: usize,

    // Niche values that can live in a single register
    OptPtr: Option<GCPtr>,
    OptBool: Option<bool>,
}

pub fn try_get_ptr(value: &Value) -> Option<GCPtr> {
    match value.ty {
        ValueType::Ptr => Some(value.get::<GCPtr>()),
        ValueType::OptPtr => value.get::<Option<GCPtr>>(),
        _ => None,
    }
}

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
pub(super) struct GCDynBuffer {
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
    pub(super) fn get_mut(&mut self) -> &mut Vec<Value> {
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
pub(super) struct GCString {
    data: String,
}

impl GCString {
    pub fn new() -> Self {
        Self {
            data: String::new(),
        }
    }

    #[inline(always)]
    pub(super) fn get(&self) -> &String {
        &self.data
    }

    #[inline(always)]
    pub(super) fn get_mut(&mut self) -> &mut String {
        &mut self.data
    }

    pub fn block_size() -> usize {
        size_of::<GCHeader>() + size_of::<Self>()
    }
}

#[repr(C)]
pub(super) struct GCTask {
    pub gc_list: Option<GCPtr>,
    data: Task,
}

impl GCTask {
    pub fn new(data: Task) -> Self {
        Self {
            gc_list: None,
            data,
        }
    }

    #[inline(always)]
    pub(super) fn get(&self) -> &Task {
        &self.data
    }

    #[inline(always)]
    pub(super) fn get_mut(&mut self) -> &mut Task {
        &mut self.data
    }

    pub fn block_size() -> usize {
        size_of::<GCHeader>() + size_of::<Self>()
    }
}
