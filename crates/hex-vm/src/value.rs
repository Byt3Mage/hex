#[allow(non_camel_case_types)]
pub type word = u64;

pub trait AsWord: Copy {
    fn from_word(w: word) -> Self;
    fn into_word(self) -> word;
}

impl AsWord for u64 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u64
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for i64 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as i64
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for usize {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as usize
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for f64 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        f64::from_bits(w)
    }
    #[inline(always)]
    fn into_word(self) -> word {
        f64::to_bits(self)
    }
}

impl AsWord for bool {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w != 0
    }

    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for u8 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u8
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for u16 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u16
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for u32 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as u32
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

impl AsWord for i32 {
    #[inline(always)]
    fn from_word(w: word) -> Self {
        w as i32
    }
    #[inline(always)]
    fn into_word(self) -> word {
        self as word
    }
}

#[macro_export]
macro_rules! args {
    ($($arg:expr),* $(,)?) => { &[$($crate::AsWord::into_word($arg)),*] };
}
