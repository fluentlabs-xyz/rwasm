use core::fmt::{Debug, Display};
use downcast_rs::{impl_downcast, DowncastSync};

/// Trait that allows the host to return a custom error.
///
/// It should be useful for representing custom traps,
/// troubles at instantiation time or other host-specific conditions.
///
/// Types that implement this trait can automatically be converted to `wasmi::Error` and
/// `wasmi::Trap` and will be represented as a boxed `HostError`. You can then use the various
/// methods on `wasmi::Error` to get your custom error type back
pub trait HostError: 'static + Display + Debug + DowncastSync {}
impl_downcast!(HostError);
