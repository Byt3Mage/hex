macro_rules! bug {
    ($($arg:tt)*) => {
        panic!("TALKSCRIPT ICE: {}", format!($($arg)*))
    };
}

pub(crate) use bug;
