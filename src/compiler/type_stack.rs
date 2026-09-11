use alloc::vec::Vec;
use core::ops::Index;
use wasmparser::ValType;

/// The emulated Wasm operand-type stack of the function being translated.
///
/// rwasm is a 32-bit slot machine: an `i64`/`f64` operand occupies two value-stack slots, every
/// other type one. Translating `local.get`/`local.set`/`local.tee` needs the slot depth of a
/// suffix of this stack, so next to the types it keeps an inclusive prefix sum of slot counts and
/// answers that query in O(1). Summing the suffix on every access would make each local access
/// linear in the current stack height and a function with many locals quadratic to compile, and
/// compilation is not fuel-metered.
#[derive(Debug, Default)]
pub struct TypeStack {
    types: Vec<ValType>,
    /// `slots[i]` is the number of value-stack slots occupied by `types[..=i]`.
    slots: Vec<u32>,
}

impl TypeStack {
    /// Returns the number of value-stack slots a value of type `ty` occupies.
    #[inline]
    pub(crate) fn slots_of(ty: ValType) -> u32 {
        match ty {
            ValType::I64 | ValType::F64 => 2,
            _ => 1,
        }
    }

    /// Returns the number of types on the stack.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        self.types.len()
    }

    /// Returns `true` if the stack holds no types.
    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    /// Removes every type from the stack.
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.types.clear();
        self.slots.clear();
    }

    /// Pushes `ty` on top of the stack.
    #[inline]
    pub(crate) fn push(&mut self, ty: ValType) {
        let height = self.slot_height() + Self::slots_of(ty);
        self.types.push(ty);
        self.slots.push(height);
    }

    /// Pushes every type of `types` in order.
    pub(crate) fn extend<'a>(&mut self, types: impl IntoIterator<Item = &'a ValType>) {
        for ty in types {
            self.push(*ty);
        }
    }

    /// Pops the top type, or `None` if the stack is empty.
    #[inline]
    pub(crate) fn pop(&mut self) -> Option<ValType> {
        self.slots.pop();
        self.types.pop()
    }

    /// Returns the top type without removing it.
    #[inline]
    pub(crate) fn last(&self) -> Option<&ValType> {
        self.types.last()
    }

    /// Returns the number of value-stack slots occupied by the whole stack.
    #[inline]
    pub(crate) fn slot_height(&self) -> u32 {
        self.slots.last().copied().unwrap_or(0)
    }

    /// Returns the number of value-stack slots occupied by the top `depth` types.
    ///
    /// `depth` must not exceed [`TypeStack::len`]; a larger depth is clamped to the whole stack.
    #[inline]
    pub(crate) fn slot_depth(&self, depth: u32) -> u32 {
        let below = self.types.len().saturating_sub(depth as usize);
        let slots_below = below
            .checked_sub(1)
            .map(|index| self.slots[index])
            .unwrap_or(0);
        self.slot_height() - slots_below
    }
}

impl Index<usize> for TypeStack {
    type Output = ValType;

    #[inline]
    fn index(&self, index: usize) -> &Self::Output {
        &self.types[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The definition `slot_depth` must agree with: the sum over the top `depth` types.
    fn naive_slot_depth(types: &[ValType], depth: u32) -> u32 {
        types
            .iter()
            .rev()
            .take(depth as usize)
            .map(|ty| TypeStack::slots_of(*ty))
            .sum()
    }

    #[test]
    fn slot_depth_matches_the_suffix_sum_under_pushes_and_pops() {
        let mut stack = TypeStack::default();
        let mut reference = Vec::new();
        // a fixed pseudo-random sequence of pushes and pops over every value type
        let mut state = 0x9e37_79b9_u32;
        for _ in 0..2_000 {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            if state.is_multiple_of(3) && !reference.is_empty() {
                assert_eq!(stack.pop(), reference.pop());
            } else {
                let ty = match state % 7 {
                    0 => ValType::I32,
                    1 => ValType::I64,
                    2 => ValType::F32,
                    3 => ValType::F64,
                    4 => ValType::FuncRef,
                    5 => ValType::ExternRef,
                    _ => ValType::I64,
                };
                stack.push(ty);
                reference.push(ty);
            }
            assert_eq!(stack.len(), reference.len());
            assert_eq!(stack.last(), reference.last());
            assert_eq!(
                stack.slot_height(),
                naive_slot_depth(&reference, reference.len() as u32)
            );
            for depth in 0..=reference.len() as u32 {
                assert_eq!(
                    stack.slot_depth(depth),
                    naive_slot_depth(&reference, depth),
                    "depth {depth} of {reference:?}"
                );
            }
        }
        stack.clear();
        assert!(stack.is_empty());
        assert_eq!(stack.slot_height(), 0);
        assert_eq!(stack.slot_depth(0), 0);
    }

    #[test]
    fn extend_and_index_follow_the_pushed_order() {
        let mut stack = TypeStack::default();
        stack.extend(&[ValType::I32, ValType::I64, ValType::F32]);
        assert_eq!(stack[0], ValType::I32);
        assert_eq!(stack[1], ValType::I64);
        assert_eq!(stack[2], ValType::F32);
        assert_eq!(stack.slot_height(), 4);
        assert_eq!(stack.slot_depth(1), 1);
        assert_eq!(stack.slot_depth(2), 3);
        assert_eq!(stack.slot_depth(3), 4);
        // a depth beyond the stack is clamped to the whole stack
        assert_eq!(stack.slot_depth(10), 4);
    }
}
