use bitvec::{index::BitIdx, order::Lsb0, store::BitStore, vec::BitVec};
use core::{cmp::min, ops::Range};

pub const USIZE_BITS: usize = 0_usize.count_zeros() as usize;

pub type ElemType = usize;

pub struct BitVecInlined<const INLINE: usize> {
    pub inline_data: [ElemType; INLINE],
    pub inline_data_bit_len: usize,
    pub bit_vec: BitVec<ElemType, Lsb0>,
}

impl<const INLINE: usize> BitVecInlined<INLINE> {
    pub const INLINE_DATA_BIT_LEN_MAX: usize = INLINE * USIZE_BITS;

    pub fn new(bit_vec: BitVec) -> Self {
        let static_vec = [ElemType::MIN; INLINE];
        let static_len = if bit_vec.len() > INLINE {
            INLINE
        } else {
            bit_vec.len()
        };
        let bit_vec = bit_vec[static_len..].try_into().unwrap();
        Self {
            inline_data: static_vec,
            inline_data_bit_len: static_len,
            bit_vec,
        }
    }

    pub fn new_empty() -> Self {
        Self {
            inline_data: [usize::MIN; INLINE],
            inline_data_bit_len: 0,
            bit_vec: BitVec::<_, _>::EMPTY,
        }
    }

    pub fn get_inline_count(&self) -> usize {
        INLINE
    }
}

impl<const INLINE: usize> BitVecInlined<INLINE> {
    pub const EMPTY: Self = Self {
        inline_data: [ElemType::MIN; INLINE],
        inline_data_bit_len: 0,
        bit_vec: BitVec::<_, _>::EMPTY,
    };

    #[inline]
    fn fill_range(data: &mut [ElemType; INLINE], range: Range<usize>, value: bool) {
        // println!("BV.fill_range(range={:?},value={})", range, value);
        let mut idx = range.start;
        while idx < range.end {
            let (item_idx, item_shift_idx_base) = Self::relative_indexes(idx);
            let bits_to_set_count = Self::INLINE_DATA_BIT_LEN_MAX - item_shift_idx_base;
            if idx + bits_to_set_count <= range.end {
                if value {
                    let mask = ElemType::MAX.unbounded_shr(item_shift_idx_base as u32);
                    data[item_idx] |= mask;
                } else {
                    let mask = ElemType::MAX.unbounded_shl(bits_to_set_count as u32);
                    data[item_idx] &= mask;
                }
                idx += bits_to_set_count;
                continue;
            }
            let item = &mut data[item_idx];
            for i in item_shift_idx_base..USIZE_BITS {
                Self::set_bit(item, i, value);
            }

            idx += USIZE_BITS - item_shift_idx_base;
        }
    }

    #[inline]
    pub fn repeat(bit: bool, len: usize) -> Self {
        // println!("BV.repeat(bit={},len={})", bit, len);
        let mut inline_data = [ElemType::MIN; INLINE];
        let inline_data_bit_len = min(len, Self::INLINE_DATA_BIT_LEN_MAX);
        Self::fill_range(&mut inline_data, 0..inline_data_bit_len, bit);
        let bit_vec = if len <= Self::INLINE_DATA_BIT_LEN_MAX {
            BitVec::<_, _>::EMPTY
        } else {
            BitVec::repeat(bit, len - Self::INLINE_DATA_BIT_LEN_MAX)
        };

        Self {
            inline_data,
            inline_data_bit_len,
            bit_vec,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.inline_data_bit_len + self.bit_vec.len()
    }

    pub fn fill(&mut self, value: bool) {
        // println!("BV.fill(value={})", value);
        let fill = if value { ElemType::MAX } else { ElemType::MIN };
        self.inline_data.fill(fill);
        if !self.bit_vec.is_empty() {
            self.bit_vec.fill(value);
        }
    }

    #[inline]
    fn item_index(index: usize) -> usize {
        let item_index = index / USIZE_BITS;
        item_index
    }

    #[inline]
    fn relative_indexes(index: usize) -> (usize, usize) {
        let item_index = Self::item_index(index);
        let item_shift_index = index - item_index * USIZE_BITS;
        (item_index, item_shift_index)
    }

    #[inline]
    pub fn get(&self, index: usize) -> Option<bool> {
        // println!("BV.get(index={})", index);
        if index < Self::INLINE_DATA_BIT_LEN_MAX {
            if index >= self.inline_data_bit_len {
                return None;
            }
            let (item_index, item_shift_index) = Self::relative_indexes(index);

            let item = self.inline_data[item_index];
            // TODO replace with manual calculation for performance?
            return Some(item.get_bit::<Lsb0>(BitIdx::new(item_shift_index as u8).unwrap()));
        };
        self.bit_vec
            .get(index - Self::INLINE_DATA_BIT_LEN_MAX)
            .as_deref()
            .copied()
    }

    #[inline]
    pub fn resize(&mut self, new_len: usize, value: bool) {
        // println!("BV.resize(new_len={},value={})", new_len, value);
        if self.inline_data_bit_len < new_len {
            let new_inline_data_bit_len = min(new_len, Self::INLINE_DATA_BIT_LEN_MAX);
            Self::fill_range(
                &mut self.inline_data,
                self.inline_data_bit_len..new_inline_data_bit_len,
                value,
            );

            self.inline_data_bit_len = new_inline_data_bit_len;
        }
        if new_len > Self::INLINE_DATA_BIT_LEN_MAX {
            let dynamic_len = new_len - Self::INLINE_DATA_BIT_LEN_MAX;
            self.bit_vec.resize(dynamic_len, value)
        }
    }

    #[inline]
    pub fn set(&mut self, index: usize, value: bool) {
        // println!("BV.set(index={},value={})", index, value);
        self.replace(index, value);
    }

    #[inline]
    pub fn set_bit(val: &mut usize, index: usize, value: bool) {
        let mask = 1usize.unbounded_shl(index as u32);
        if value {
            *val |= mask;
        } else {
            *val &= !mask;
        }
    }

    #[inline]
    pub fn replace(&mut self, index: usize, value: bool) -> bool {
        // println!("replace(index={},value={})", index, value);
        if index >= Self::INLINE_DATA_BIT_LEN_MAX {
            return self
                .bit_vec
                .replace(index - Self::INLINE_DATA_BIT_LEN_MAX, value);
        }
        self.assert_valid_idx(index);
        let (item_index, item_shift_index) = Self::relative_indexes(index);
        let old_value = self.get(item_index).unwrap();
        let item = &mut self.inline_data[item_index];
        Self::set_bit(item, item_shift_index, value);

        old_value
    }

    #[inline]
    pub fn assert_valid_idx(&self, idx: usize) {
        if idx >= self.len() {
            panic!("index out of bounds")
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{bitvec_inlined::USIZE_BITS, types::bitvec_inlined::BitVecInlined};

    #[test]
    fn tt() {
        let v = 1usize;
        let r = 2usize;
        assert_eq!(v.unbounded_shl(1), r);
    }

    #[test]
    fn bit_vec_inlined() {
        let mut len = 65;
        let mut idx = 1;
        assert!(idx < len);
        let mut value = true;
        let mut bv = BitVecInlined::<1>::repeat(value, len);
        assert_eq!(bv.get(idx), Some(value));
        idx = 64;
        value = false;
        bv.set(idx, value);
        assert_eq!(bv.get(idx), Some(value));
        assert_eq!(bv.get(len), None);
        len += 1;
        value = true;
        bv.resize(len, value);
        assert_eq!(bv.get(len - 1), Some(value));
    }

    #[test]
    fn bit_vec_inlined_filling() {
        const LEN_BASE: usize = 65;
        let mut len = LEN_BASE;
        let mut idx = 1;
        assert!(idx < len);
        let mut value = true;
        let mut bv = BitVecInlined::<{ (LEN_BASE + USIZE_BITS) / USIZE_BITS }>::repeat(value, len);
        assert_eq!(bv.get(idx), Some(value));
        idx = 64;
        value = false;
        bv.set(idx, value);
        assert_eq!(bv.get(idx), Some(value));
        assert_eq!(bv.get(len), None);
        len += 1;
        value = true;
        bv.resize(len, value);
        assert_eq!(bv.get(len - 1), Some(value));
        len += USIZE_BITS;
        value = false;
        bv.resize(len, value);
        assert_eq!(bv.get(len - 1), Some(value));
        assert_eq!(bv.get(63), Some(true));
        assert_eq!(bv.get(64), Some(false));
        assert_eq!(bv.get(65), Some(true));
        assert_eq!(bv.get(66), Some(false));
    }
}
