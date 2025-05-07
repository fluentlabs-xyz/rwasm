use crate::{
    executor::element_entity::ElementSegmentEntity,
    types::{RwasmError, UntypedValue, N_MAX_TABLE_SIZE},
};

/// A Wasm table entity.
#[derive(Debug)]
pub struct TableEntity {
    elements: Vec<UntypedValue>,
}

impl TableEntity {
    /// Creates a new table entity with the given resizable limits.
    ///
    /// # Errors
    ///
    /// If `init` does not match the [`TableType`] element type.
    pub fn new(init: UntypedValue, default_size: usize) -> Self {
        let mut elements = Vec::with_capacity(N_MAX_TABLE_SIZE);
        elements.resize(default_size, init);
        Self { elements }
    }

    /// Returns the current size of the [`Table`].
    pub fn size(&self) -> u32 {
        self.elements.len() as u32
    }

    /// Grows the table by the given amount of elements.
    ///
    /// Returns the old size of the [`Table`] upon success.
    ///
    /// # Note
    ///
    /// This is an internal API that exists for efficiency purposes.
    ///
    /// The newly added elements are initialized to the `init` [`Value`].
    ///
    /// # Errors
    ///
    /// If the table is grown beyond its maximum limits.
    pub fn grow_untyped(&mut self, delta: u32, init: UntypedValue) -> Result<u32, RwasmError> {
        // ResourceLimiter gets the first look at the request.
        let current = self.size();
        let Some(desired) = current.checked_add(delta) else {
            return Ok(u32::MAX);
        };
        if desired as usize > self.elements.capacity() {
            return Ok(u32::MAX);
        }
        self.elements.resize(desired as usize, init);
        Ok(current)
    }

    /// Returns the untyped [`Table`] element value at `index`.
    ///
    /// Returns `None` if `index` is out of bounds.
    ///
    /// # Note
    ///
    /// This is a more efficient version of [`Table::get`] for
    /// internal use only.
    pub fn get_untyped(&self, index: u32) -> Option<UntypedValue> {
        self.elements.get(index as usize).copied()
    }

    /// Returns the [`UntypedValue`] of the [`Table`] at `index`.
    ///
    /// # Errors
    ///
    /// If `index` is out of bounds.
    pub fn set_untyped(&mut self, index: u32, value: UntypedValue) -> Result<(), RwasmError> {
        let untyped = self
            .elements
            .get_mut(index as usize)
            .ok_or(RwasmError::TableOutOfBounds)?;
        *untyped = value;
        Ok(())
    }

    pub fn init_untyped(
        &mut self,
        dst_index: u32,
        element: &ElementSegmentEntity,
        src_index: u32,
        len: u32,
    ) -> Result<(), RwasmError> {
        // Convert parameters to indices.
        let dst_index = dst_index as usize;
        let src_index = src_index as usize;
        let len = len as usize;
        // Perform bound check before anything else.
        let dst_items = self
            .elements
            .get_mut(dst_index..)
            .and_then(|items| items.get_mut(..len))
            .ok_or(RwasmError::TableOutOfBounds)?;
        let src_items = element
            .items()
            .get(src_index..)
            .and_then(|items| items.get(..len))
            .ok_or(RwasmError::TableOutOfBounds)?;
        if len == 0 {
            // Bail out early if nothing needs to be initialized.
            // The Wasm spec demands to still perform the bound check
            // so we cannot bail out earlier.
            return Ok(());
        }
        // Perform the actual table initialization.
        dst_items.iter_mut().zip(src_items).for_each(|(dst, src)| {
            *dst = *src;
        });
        Ok(())
    }

    /// Copy `len` elements from `src_table[src_index..]` into
    /// `dst_table[dst_index..]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of either the source or
    /// destination tables.
    pub fn copy(
        dst_table: &mut Self,
        dst_index: u32,
        src_table: &Self,
        src_index: u32,
        len: u32,
    ) -> Result<(), RwasmError> {
        // Turn parameters into proper slice indices.
        let src_index = src_index as usize;
        let dst_index = dst_index as usize;
        let len = len as usize;
        // Perform bound check before anything else.
        let dst_items = dst_table
            .elements
            .get_mut(dst_index..)
            .and_then(|items| items.get_mut(..len))
            .ok_or(RwasmError::TableOutOfBounds)?;
        let src_items = src_table
            .elements
            .get(src_index..)
            .and_then(|items| items.get(..len))
            .ok_or(RwasmError::TableOutOfBounds)?;
        // Finally, copy elements in-place for the table.
        dst_items.copy_from_slice(src_items);
        Ok(())
    }

    /// Copy `len` elements from `self[src_index..]` into `self[dst_index..]`.
    ///
    /// # Errors
    ///
    /// Returns an error if the range is out of bounds of the table.
    pub fn copy_within(
        &mut self,
        dst_index: u32,
        src_index: u32,
        len: u32,
    ) -> Result<(), RwasmError> {
        // These accesses just perform the bound checks required by the Wasm spec.
        let max_offset = core::cmp::max(dst_index, src_index);
        max_offset
            .checked_add(len)
            .filter(|&offset| offset <= self.size())
            .ok_or(RwasmError::TableOutOfBounds)?;
        // Turn parameters into proper indices.
        let src_index = src_index as usize;
        let dst_index = dst_index as usize;
        let len = len as usize;
        // Finally, copy elements in-place for the table.
        self.elements
            .copy_within(src_index..src_index.wrapping_add(len), dst_index);
        Ok(())
    }

    /// Fill `table[dst..(dst + len)]` with the given value.
    ///
    /// # Note
    ///
    /// This is an API for internal use only and exists for efficiency reasons.
    ///
    /// # Errors
    ///
    /// - If the region to be filled is out of bounds for the [`Table`].
    ///
    /// # Panics
    ///
    /// If `ctx` does not own `dst_table` or `src_table`.
    ///
    /// [`Store`]: [`crate::Store`]
    pub fn fill_untyped(
        &mut self,
        dst: u32,
        val: UntypedValue,
        len: u32,
    ) -> Result<(), RwasmError> {
        let dst_index = dst as usize;
        let len = len as usize;
        let dst = self
            .elements
            .get_mut(dst_index..)
            .and_then(|elements| elements.get_mut(..len))
            .ok_or(RwasmError::TableOutOfBounds)?;
        dst.fill(val);
        Ok(())
    }
}
