use crate::{compiler::value_stack::ValueStackHeight, CompilationError, InstructionSet};
use bincode::{Decode, Encode};
use core::fmt;

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
    pub fn new(drop: usize, keep: usize) -> Result<Self, CompilationError> {
        let keep = u16::try_from(keep).map_err(|_| CompilationError::DropKeepOutOfBounds)?;
        let drop = u16::try_from(drop).map_err(|_| CompilationError::DropKeepOutOfBounds)?;
        // Now we can cast `drop` and `keep` to `u16` values safely.
        Ok(Self { drop, keep })
    }
}

pub fn translate_drop_keep(
    instr_builder: &mut InstructionSet,
    drop_keep: DropKeep,
    height: &mut ValueStackHeight,
) -> usize {
    let (drop, keep) = (drop_keep.drop(), drop_keep.keep());
    if drop == 0 {
        return 0;
    }
    let mut opcode_count = 0;
    if drop >= keep {
        (0..keep).for_each(|_| {
            instr_builder.op_local_set(drop as u32);
            opcode_count += 1;
        });
        (0..(drop - keep)).for_each(|_| {
            instr_builder.op_drop();
            opcode_count += 1;
        });
    } else {
        height.push1();
        height.pop1();
        (0..keep).for_each(|i| {
            instr_builder.op_local_get(keep as u32 - i as u32);
            instr_builder.op_local_set(keep as u32 + drop as u32 - i as u32);
            opcode_count += 2;
        });
        (0..drop).for_each(|_| {
            instr_builder.op_drop();
            opcode_count += 1;
        });
    }
    opcode_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Opcode, OpcodeData};

    #[test]
    fn test_drop_keep_translation() {
        macro_rules! drop_keep {
            ($drop:literal, $keep:literal) => {
                DropKeep::new($drop, $keep).unwrap()
            };
        }
        let tests = vec![
            (vec![100, 20, 120], vec![120], drop_keep!(2, 1)),
            (vec![1, 2], vec![1, 2], drop_keep!(0, 0)),
            (vec![1, 2, 3], vec![1, 2, 3], drop_keep!(0, 3)),
            (vec![1, 2, 3, 4], vec![3, 4], drop_keep!(2, 2)),
            (vec![2, 3, 7], vec![3, 7], drop_keep!(1, 2)),
            (vec![1, 2, 3, 4, 5, 6], vec![3, 4, 5, 6], drop_keep!(2, 4)),
            (vec![7, 100, 20, 3], vec![7], drop_keep!(3, 0)),
            (vec![100, 20, 120], vec![120], drop_keep!(2, 1)),
            (vec![1, 2, 3, 4, 5], vec![5], drop_keep!(4, 1)),
        ];
        for (input, output, drop_keep) in tests.iter() {
            let mut instr_builder = InstructionSet::new();
            let mut stack_height = ValueStackHeight::default();
            translate_drop_keep(&mut instr_builder, *drop_keep, &mut stack_height);
            let mut stack = input.clone();
            for instr in instr_builder.instr {
                match instr {
                    (Opcode::LocalSet, OpcodeData::LocalDepth(index)) => {
                        let last = stack.last().unwrap();
                        let len = stack.len();
                        *stack.get_mut(len - 1 - index as usize).unwrap() = *last;
                        stack.pop();
                    }
                    (Opcode::LocalGet, OpcodeData::LocalDepth(index)) => {
                        let len = stack.len();
                        let item = *stack.get(len - index as usize).unwrap();
                        stack.push(item);
                    }
                    (Opcode::Drop, _) => {
                        stack.pop();
                    }
                    _ => unreachable!("unknown opcode: {:?}", instr),
                }
            }
            assert_eq!(stack, *output);
        }
    }
}
