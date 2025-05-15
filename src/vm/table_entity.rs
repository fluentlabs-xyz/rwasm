use crate::types::{RwasmError, UntypedValue, N_MAX_TABLE_SIZE};
use alloc::vec::Vec;

/// A Wasm table entity.
#[derive(Debug)]
pub struct TableEntity {
    pub(crate) elements: Vec<UntypedValue>,
}

impl TableEntity {
    /// Creates a new table entity with the given resizable limits.
    ///
    /// # Errors
    ///
    /// If `init` does not match the [`TableType`] element type.
    pub fn new() -> Self {
        let elements = Vec::with_capacity(N_MAX_TABLE_SIZE);
        Self { elements }
    }

    /// Returns the current size of the [`Table`].
    pub fn size(&self) -> u32 {
        self.elements.len() as u32
    }

    /// Grows the table by the given number of elements.
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
    pub fn grow_untyped(&mut self, delta: u32, init: UntypedValue) -> u32 {
        let current = self.size();
        let Some(desired) = current.checked_add(delta) else {
            return u32::MAX;
        };
        if desired as usize > self.elements.capacity() {
            return u32::MAX;
        }
        self.elements.resize(desired as usize, init);
        current
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

    /// Initializes a segment of the `elements` table with values from a specified `element` array.
    ///
    /// # Parameters
    ///
    /// - `&mut self`: A mutable reference to the current instance, allowing modification of the
    ///   `elements` table.
    /// - `dst_index`: The starting index in the `elements` table where the initialization will
    ///   begin.
    /// - `element`: A slice of `UntypedValue` containing the values to copy into the `elements`
    ///   table.
    /// - `src_index`: The starting index in the `element` slice from which to copy values.
    /// - `len`: The number of values to copy from `element` to `elements`.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the segment was successfully initialized.
    /// - `Err(RwasmError::TableOutOfBounds)` if the source or destination ranges are out of bounds.
    ///
    /// # Behavior
    ///
    /// 1. Convert `dst_index`, `src_index`, and `len` to `usize` for indexing.
    /// 2. Performs bounds-checking for both the `elements` table and the `element` slice:
    ///    - Ensures enough elements are available starting from `dst_index` and `src_index` for the
    ///      specified `len`.
    ///    - If the bounds are exceeded, it returns `Err(RwasmError::TableOutOfBounds)`.
    /// 3. If `len` is 0, the method returns early with `Ok(())` after performing the necessary
    ///    bounds check.
    /// 4. Copies the specified `len` values from `element[src_index..]` into
    ///    `elements[dst_index..]`.
    ///
    /// # Notes
    ///
    /// - The bound check is always performed, even if `len` is 0, to adhere to WebAssembly
    ///   specifications.
    /// - This function works with untyped values (`UntypedValue`), allowing generic initialization
    ///   of table segments.
    pub fn init_untyped(
        &mut self,
        dst_index: u32,
        element: &[UntypedValue],
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
