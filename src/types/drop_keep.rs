use bincode::{Decode, Encode};
use core::{fmt, fmt::Display};

/// Defines how many stack values are going to be dropped and kept after branching.
#[derive(Copy, Clone, PartialEq, Eq, Default, Hash, PartialOrd, Ord, Encode, Decode)]
pub struct DropKeep {
    pub drop: u16,
    pub keep: u16,
}

impl fmt::Debug for DropKeep {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("DropKeep")
            .field("drop", &self.drop())
            .field("keep", &self.keep())
            .finish()
    }
}

/// An error that may occur upon operating on [`DropKeep`].
#[derive(Debug, Copy, Clone)]
pub enum DropKeepError {
    /// The number of kept elements exceeds the engine's limits.
    KeepOutOfBounds,
    /// The number of dropped elements exceeds the engine's limits.
    DropOutOfBounds,
}

impl Display for DropKeepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DropKeepError::KeepOutOfBounds => {
                write!(f, "number of kept elements exceeds engine limits")
            }
            DropKeepError::DropOutOfBounds => {
                write!(f, "number of dropped elements exceeds engine limits")
            }
        }
    }
}

impl DropKeep {
    pub fn none() -> Self {
        Self { drop: 0, keep: 0 }
    }

    /// Returns the number of stack values to keep.
    pub fn keep(self) -> u16 {
        self.keep
    }

    pub fn add_keep(&mut self, delta: u16) {
        self.keep += delta;
    }

    /// Returns the number of stack values to drop.
    pub fn drop(self) -> u16 {
        self.drop
    }

    /// Returns `true` if the [`DropKeep`] does nothing.
    pub fn is_noop(self) -> bool {
        self.drop == 0
    }

    /// Creates a new [`DropKeep`] with the given amounts to drop and keep.
    ///
    /// # Errors
    ///
    /// - If `keep` is larger than `drop`.
    /// - If `keep` is out of bounds. (max 4095)
    /// - If `drop` is out of bounds. (delta to keep max 4095)
    pub fn new(drop: usize, keep: usize) -> Result<Self, DropKeepError> {
        let keep = u16::try_from(keep).map_err(|_| DropKeepError::KeepOutOfBounds)?;
        let drop = u16::try_from(drop).map_err(|_| DropKeepError::KeepOutOfBounds)?;
        // Now we can cast `drop` and `keep` to `u16` values safely.
        Ok(Self { drop, keep })
    }
}
