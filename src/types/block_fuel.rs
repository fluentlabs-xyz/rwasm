use crate::RwasmError;
use bincode::{Decode, Encode};

/// The accumulated fuel to execute a block via [`Instruction::ConsumeFuel`].
///
/// [`Instruction::ConsumeFuel`]: [`super::Instruction::ConsumeFuel`]
#[derive(Debug, Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
#[repr(transparent)]
pub struct BlockFuel(u32);

impl TryFrom<u64> for BlockFuel {
    type Error = RwasmError;

    fn try_from(index: u64) -> Result<Self, Self::Error> {
        match u32::try_from(index) {
            Ok(index) => Ok(Self(index)),
            Err(_) => Err(RwasmError::BlockFuelOutOfBounds),
        }
    }
}

impl From<u32> for BlockFuel {
    fn from(value: u32) -> Self {
        BlockFuel(value)
    }
}

impl BlockFuel {
    /// Bump the fuel by `amount` if possible.
    ///
    /// # Errors
    ///
    /// If the new fuel amount after this operation is out of bounds.
    pub fn bump_by(&mut self, amount: u64) -> Result<(), RwasmError> {
        let new_amount = self
            .to_u64()
            .checked_add(amount)
            .ok_or(RwasmError::BlockFuelOutOfBounds)?;
        self.0 = u32::try_from(new_amount).map_err(|_| RwasmError::BlockFuelOutOfBounds)?;
        Ok(())
    }

    /// Returns the index value as `u64`.
    pub fn to_u64(self) -> u64 {
        u64::from(self.0)
    }
}
