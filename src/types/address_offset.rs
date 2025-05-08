use bincode::{Decode, Encode};

/// A linear memory access offset.
///
/// # Note
///
/// Used to calculate the effective address of linear memory access.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct AddressOffset(u32);

impl From<u32> for AddressOffset {
    fn from(index: u32) -> Self {
        Self(index)
    }
}

impl AddressOffset {
    /// Returns the inner `u32` index.
    pub fn into_inner(self) -> u32 {
        self.0
    }
}
