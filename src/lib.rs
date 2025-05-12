#![cfg_attr(not(feature = "std"), no_std)]
#![allow(dead_code)]
#![warn(unused_crate_dependencies)]

mod types;
mod vm;

extern crate alloc;
extern crate core;

pub use types::*;
pub use vm::*;

pub mod legacy {
    pub use rwasm_legacy::*;
}

use libm as _;
