/// A reference to an instruction of the partially
/// constructed function body of the [`InstructionsBuilder`].
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct InstrLoc(u32);

impl InstrLoc {
    /// Creates an [`rwasm_legacy::engine::Instr`] from the given `usize` value.
    ///
    /// # Note
    ///
    /// This intentionally is an API intended for test purposes only.
    ///
    /// # Panics
    ///
    /// If the `value` exceeds limitations for [`rwasm_legacy::engine::Instr`].
    pub fn from_usize(value: usize) -> Self {
        let value = value.try_into().unwrap_or_else(|error| {
            panic!("invalid index {value} for instruction reference: {error}")
        });
        Self(value)
    }

    /// Returns a ` usize ` representation of the instruction index.
    pub fn into_usize(self) -> usize {
        self.0 as usize
    }

    /// Creates an [`rwasm_legacy::engine::Instr`] form the given `u32` value.
    pub fn from_u32(value: u32) -> Self {
        Self(value)
    }

    /// Returns an `u32` representation of the instruction index.
    pub fn into_u32(self) -> u32 {
        self.0
    }
}
