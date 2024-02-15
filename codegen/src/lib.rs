#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
#[macro_use]
extern crate alloc;
extern crate core;
#[cfg(feature = "std")]
extern crate std as alloc;

pub mod binary_format;
mod compiler;
mod instruction_set;
mod platform;
mod reduced_module;
#[cfg(test)]
mod tests;

pub use self::{binary_format::*, instruction_set::*, platform::*, reduced_module::*};
pub use rwasm;
