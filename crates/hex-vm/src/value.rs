#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Value(u64);

impl Value {
    pub const ZERO: Self = Self(0);

    #[inline(always)]
    pub fn get<T: IsValue>(self) -> T {
        T::from_value(self)
    }

    #[inline(always)]
    pub fn set<T: IsValue>(&mut self, v: T) {
        *self = v.into_value();
    }
}

pub trait IsValue: Copy {
    fn from_value(v: Value) -> Self;
    fn into_value(self) -> Value;
}

impl IsValue for usize {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as usize
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for u64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self)
    }
}

impl IsValue for i64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as i64
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for f64 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        f64::from_bits(v.0)
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self.to_bits())
    }
}

impl IsValue for bool {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 != 0
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for u32 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u32
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}

impl IsValue for u16 {
    #[inline(always)]
    fn from_value(v: Value) -> Self {
        v.0 as u16
    }
    #[inline(always)]
    fn into_value(self) -> Value {
        Value(self as u64)
    }
}
