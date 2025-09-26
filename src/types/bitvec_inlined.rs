use bitvec::index::BitIdx;
use bitvec::order::Lsb0;
use bitvec::store::BitStore;
use bitvec::vec::BitVec;
use core::cmp::min;
use std::ops::Range;

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

    fn fill_range(data: &mut [ElemType; INLINE], range: Range<usize>, value: bool) {
        let mut idx = range.start;
        while idx < range.end {
            let (item_idx, item_shift_idx_base) = Self::relative_indexes(idx);
            let item = &mut data[item_idx];
            let mut shift_idx = item_shift_idx_base;
            while shift_idx < Self::INLINE_DATA_BIT_LEN_MAX {
                Self::set_bit(item, shift_idx, value);
                shift_idx += 1;
            }

            idx += shift_idx - item_shift_idx_base;
        }
    }

    #[inline]
    pub fn repeat(bit: bool, len: usize) -> Self {
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

    pub fn len(&self) -> usize {
        self.inline_data_bit_len + self.bit_vec.len()
    }

    pub fn fill(&mut self, value: bool) {
        let fill = if value { ElemType::MAX } else { ElemType::MIN };
        self.inline_data.fill(fill);
        if !self.bit_vec.is_empty() {
            self.bit_vec.fill(value);
        }
    }

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

    pub fn get(&self, index: usize) -> Option<bool> {
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
        if self.inline_data_bit_len < new_len {
            let new_inline_data_bit_len = min(new_len, Self::INLINE_DATA_BIT_LEN_MAX);
            Self::fill_range(
                &mut self.inline_data,
                self.inline_data_bit_len..new_inline_data_bit_len,
                value,
            );
            let len = self.inline_data_bit_len;
            while len < new_inline_data_bit_len {}

            self.inline_data_bit_len = new_inline_data_bit_len;
        }
        if new_len > Self::INLINE_DATA_BIT_LEN_MAX {
            let dynamic_len = new_len - Self::INLINE_DATA_BIT_LEN_MAX;
            self.bit_vec.resize(dynamic_len, value)
        }
    }

    #[inline]
    pub fn set(&mut self, index: usize, value: bool) {
        self.replace(index, value);
    }

    #[inline]
    pub fn set_bit(val: &mut usize, index: usize, value: bool) {
        let mask = 1usize << index;
        if value {
            *val |= mask;
        } else {
            *val &= !mask;
        }
    }

    #[inline]
    pub fn replace(&mut self, index: usize, value: bool) -> bool {
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
    use crate::types::bitvec_inlined::BitVecInlined;

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
}
