use std::rc::Rc;

use crate::{name::Name, object::Value, program::NativeFunc, vm::VMResult};

/// A type that can be constructed from a slice of Values
pub trait FromValues: Sized + 'static {
    /// Number of Value registers needed
    const NREG: u8;

    /// Construct from a slice of Values
    fn from_values(values: &[Value]) -> VMResult<Self>;
}

/// A type that can be written into a slice of Values
pub trait IntoValues: 'static {
    /// Number of Value registers needed
    const NREG: u8;

    /// Write into a slice of Values
    fn into_values(self, values: &mut [Value]) -> VMResult<()>;
}

/// A trait for types that can be used as native functions in the VM
pub trait IntoNativeFunc {
    fn into_native_func(self, name: Name) -> NativeFunc;
}

impl<Ret: IntoValues> IntoNativeFunc for fn() -> VMResult<Ret> {
    fn into_native_func(self, name: Name) -> NativeFunc {
        NativeFunc {
            name,
            func: Rc::new(move |_: &[Value], result: &mut [Value]| {
                self()?.into_values(result)?;
                Ok(())
            }),
            narg: 0,
            nret: Ret::NREG,
        }
    }
}

macro_rules! impl_into_native_func {
    ($($T:ident),+) => {
        impl<$($T,)+ Ret> IntoNativeFunc for fn($($T),+) -> VMResult<Ret>
        where
            $($T: FromValues,)+
            Ret: IntoValues,
        {
            #[allow(unused_assignments)]
            fn into_native_func(self, name: Name) -> NativeFunc {
                NativeFunc {
                    name,
                    func: Rc::new(move |args: &[Value], result: &mut [Value]| {
                        let mut offset = 0usize;
                        $(
                            #[allow(non_snake_case)]
                            let $T = {
                                let val = $T::from_values(&args[offset..offset + $T::NREG as usize])?;
                                offset += $T::NREG as usize;
                                val
                            };
                        )+
                        self($($T),+)?.into_values(result)?;
                        Ok(())
                    }),
                    narg: 0 $(+ $T::NREG)+,
                    nret: Ret::NREG,
                }
            }
        }
    };
}

impl_into_native_func!(A);
impl_into_native_func!(A, B);
impl_into_native_func!(A, B, C);
impl_into_native_func!(A, B, C, D);
impl_into_native_func!(A, B, C, D, E);
impl_into_native_func!(A, B, C, D, E, F);
