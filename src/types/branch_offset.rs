use bincode::{Decode, Encode};
use serde::{Deserialize, Serialize};

/// A signed offset for branch instructions.
///
/// This defines how much the instruction pointer is offset
/// upon taking the respective branch.
#[cfg_attr(feature = "tracing", derive(Serialize, Deserialize))]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
pub struct BranchOffset(i32);

impl From<i32> for BranchOffset {
    fn from(index: i32) -> Self {
        Self(index)
    }
}

impl BranchOffset {
    /// Creates an uninitialized [`BranchOffset`].
    pub fn uninit() -> Self {
        Self(0)
    }

    /// Creates an initialized [`BranchOffset`] from `src` to `dst`.
    ///
    /// # Errors
    ///
    /// If the resulting [`BranchOffset`] is out of bounds.
    ///
    /// # Panics
    ///
    /// If the resulting [`BranchOffset`] is uninitialized, aka equal to 0.
    pub fn from_src_to_dst(src: u32, dst: u32) -> Option<Self> {
        let src = i64::from(src);
        let dst = i64::from(dst);
        let offset = dst.checked_sub(src)?;
        let offset = i32::try_from(offset).ok()?;
        Some(Self(offset))
    }

    /// Returns the `i32` representation of the [`BranchOffset`].
    pub fn to_i32(self) -> i32 {
        self.0
    }
}
