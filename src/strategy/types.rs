use crate::{CompilationError, TrapCode};

pub trait StoreTr<T> {
    fn memory_read(&mut self, offset: usize, buffer: &mut [u8]) -> Result<(), TrapCode>;

    fn memory_write(&mut self, offset: usize, buffer: &[u8]) -> Result<(), TrapCode>;

    fn data_mut(&mut self) -> &mut T;

    fn data(&self) -> &T;

    fn try_consume_fuel(&mut self, delta: u64) -> Result<(), TrapCode>;

    fn remaining_fuel(&self) -> Option<u64>;

    fn reset_fuel(&mut self, new_fuel_limit: u64);
}

pub trait CallerTr<T>: StoreTr<T> {}

#[derive(Debug)]
pub enum StrategyError {
    CompilationError(CompilationError),
    TrapCode(TrapCode),
}

impl From<CompilationError> for StrategyError {
    fn from(err: CompilationError) -> Self {
        StrategyError::CompilationError(err)
    }
}
impl From<TrapCode> for StrategyError {
    fn from(err: TrapCode) -> Self {
        StrategyError::TrapCode(err)
    }
}
