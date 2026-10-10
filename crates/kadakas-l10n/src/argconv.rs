//! How `tr!` turns an argument into a message argument, without a type annotation.
//!
//! A number stays a number, so plural rules work (`[one]`, `[other]`). Anything else that
//! can be shown (`Display`: text, paths, errors, JSON values) becomes text. The choice is made
//! at compile time by "autoref specialization": the method of `ViaNum` is found first, and
//! the method of `ViaDisplay` is the fallback.

use std::fmt::Display;

use crate::Arg;

/// Holds a reference to the argument.
pub struct Wrap<'a, T: ?Sized>(pub &'a T);

/// A number type.
pub trait NumArg {
    fn num(&self) -> f64;
}

macro_rules! num_arg {
    ($($t:ty),*) => {$(
        impl NumArg for $t {
            fn num(&self) -> f64 {
                *self as f64
            }
        }
    )*};
}
num_arg!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

impl<T: NumArg + ?Sized> NumArg for &T {
    fn num(&self) -> f64 {
        (**self).num()
    }
}

pub trait ViaNum {
    fn to_arg(&self) -> Arg;
}

impl<T: NumArg + ?Sized> ViaNum for Wrap<'_, T> {
    fn to_arg(&self) -> Arg {
        Arg::Num(self.0.num())
    }
}

pub trait ViaDisplay {
    fn to_arg(&self) -> Arg;
}

impl<T: Display + ?Sized> ViaDisplay for &Wrap<'_, T> {
    fn to_arg(&self) -> Arg {
        Arg::Text(self.0.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_stay_numbers_and_the_rest_is_text() {
        use crate::tr;
        assert_eq!(tr!("common-delete-n", count = 1), "Delete 1 object");
        let n = 2usize;
        assert_eq!(tr!("common-delete-n", count = &n), "Delete 2 objects");
        assert_eq!(tr!("common-delete-n", count = n as f64), "Delete 2 objects");
        let name = String::from("Mari");
        assert_eq!(tr!("common-test-hello", name = name), "Hello Mari");
        assert_eq!(tr!("common-test-hello", name = &name), "Hello Mari");
        assert_eq!(tr!("common-test-hello", name = "Jaan"), "Hello Jaan");
        assert_eq!(
            tr!(
                "common-test-hello",
                name = std::path::Path::new("/a").display()
            ),
            "Hello /a"
        );
        assert_eq!(Wrap(&1.5f32).to_arg(), Arg::Num(1.5));
    }
}
