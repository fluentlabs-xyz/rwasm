use crate::{
    types::{TrapCode, UntypedValue},
    ExternRef, FuncRef, I64ValueSplit, Value, F32, F64, N_DEFAULT_STACK_SIZE, N_MAX_STACK_SIZE,
};
use alloc::vec::Vec;
use core::fmt::Debug;
use smallvec::{smallvec, SmallVec};
use wasmparser::ValType;

/// The value stack used to execute Wasm bytecode.
///
/// # Note
///
/// The [`ValueStack`] implementation heavily relies on the prior
/// validation of the executed Wasm bytecode for correct execution.
#[derive(Clone)]
pub struct ValueStack {
    /// All currently live stack entries.
    entries: SmallVec<[UntypedValue; N_DEFAULT_STACK_SIZE]>,
    /// Index of the first free place in the stack.
    stack_ptr: usize,
    /// The maximum value stack height.
    ///
    /// # Note
    ///
    /// Extending the value stack beyond this limit during execution
    /// will cause a stack overflow trap.
    pub maximum_len: usize,
    /// The maximum stack height
    max_stack_height: usize,
}

impl Debug for ValueStack {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ValueStack")
            .field("stack_ptr", &self.stack_ptr)
            .field("entries", &&self.entries[..self.stack_ptr])
            .finish()
    }
}

impl PartialEq for ValueStack {
    fn eq(&self, other: &Self) -> bool {
        self.stack_ptr == other.stack_ptr
            && self.entries[..self.stack_ptr] == other.entries[..other.stack_ptr]
    }
}

impl Eq for ValueStack {}

impl Extend<UntypedValue> for ValueStack {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = UntypedValue>,
    {
        for item in iter {
            self.push(item)
        }
    }
}

impl Default for ValueStack {
    fn default() -> Self {
        Self::new(N_MAX_STACK_SIZE)
    }
}

impl ValueStack {
    /// Creates an empty [`ValueStack`] that does not allocate heap memor.
    ///
    /// # Note
    ///
    /// This is required for resumable functions to replace their
    /// proper stack with an inexpensive fake one.
    pub fn empty() -> Self {
        Self {
            entries: SmallVec::new(),
            stack_ptr: 0,
            maximum_len: 0,
            max_stack_height: 0,
        }
    }

    pub fn max_stack_height(&self) -> usize {
        self.max_stack_height
    }

    /// Returns the current [`ValueStackPtr`] of `self`.
    ///
    /// The returned [`ValueStackPtr`] points to the top most value on the [`ValueStack`].
    #[inline]
    pub fn stack_ptr(&mut self) -> ValueStackPtr {
        self.base_ptr().into_add(self.stack_ptr)
    }

    /// Calculates the length of the stack from a given stack pointer.
    pub fn stack_len(&mut self, sp: ValueStackPtr) -> usize {
        sp.offset_from(self.base_ptr()) as usize
    }

    /// Checks if the stack has overflowed based on the provided stack pointer.
    pub fn has_stack_overflowed(&mut self, sp: ValueStackPtr) -> bool {
        self.stack_len(sp) > self.maximum_len
    }

    /// Returns a slice of `UntypedValue` starting from the base pointer up to the given
    /// `ValueStackPtr` (exclusive).
    pub fn as_slice(&mut self) -> &mut [UntypedValue] {
        &mut self.entries[0..self.stack_ptr]
    }

    /// Dumps a portion of the value stack into a `Vec<UntypedValue>`.
    pub fn dump_stack(&mut self) -> Vec<UntypedValue> {
        debug_assert!(
            self.stack_ptr <= self.capacity(),
            "stack_ptr={}, capacity={}",
            self.stack_ptr,
            self.capacity()
        );
        unsafe { self.entries.get_unchecked_mut(..self.stack_ptr) }.to_vec()
    }

    /// Returns the base [`ValueStackPtr`] of `self`.
    ///
    /// The returned [`ValueStackPtr`] points to the first value on the [`ValueStack`].
    #[inline]
    fn base_ptr(&mut self) -> ValueStackPtr {
        ValueStackPtr::new(self.entries.as_mut_ptr())
    }

    /// Synchronizes [`ValueStack`] with the new [`ValueStackPtr`].
    #[inline]
    pub fn sync_stack_ptr(&mut self, new_sp: ValueStackPtr) {
        let offset = new_sp.offset_from(self.base_ptr());
        debug_assert!(offset >= 0, "stack underflow: {}", offset);
        self.stack_ptr = offset as usize;
        if self.stack_ptr > self.max_stack_height {
            self.max_stack_height = self.stack_ptr;
        }
    }

    pub(crate) fn check_max_stack_height(&mut self, sp: ValueStackPtr) {
        let offset = sp.offset_from(self.base_ptr());
        debug_assert!(offset >= 0, "stack underflow: {}", offset);
        if offset as usize > self.max_stack_height {
            self.max_stack_height = offset as usize;
        }
    }

    /// Returns `true` if the [`ValueStack`] is empty.
    pub fn is_empty(&self) -> bool {
        self.stack_ptr == 0
    }

    /// Creates a new empty [`ValueStack`].
    pub fn new(maximum_len: usize) -> Self {
        // use maximum_len to prevent reallocations (which cause base_ptr change)
        let entries = smallvec![UntypedValue::default(); maximum_len];
        Self {
            entries,
            stack_ptr: 0,
            maximum_len,
            max_stack_height: 0,
        }
    }

    /// Returns the [`UntypedValue`] at the given `index`.
    ///
    /// # Note
    ///
    /// This is an optimized convenience method that only asserts
    /// that the index is within bounds in `debug` mode.
    ///
    /// # Safety
    ///
    /// This is safe since all rwasm bytecode has been validated
    /// during translation and therefore cannot result in out-of-bounds accesses.
    ///
    /// # Panics (Debug)
    ///
    /// If the `index` is out of bounds.
    #[inline]
    fn get_release_unchecked_mut(&mut self, index: usize) -> &mut UntypedValue {
        debug_assert!(index < self.capacity());
        // Safety: This is safe since all rwasm bytecode has been validated
        //         during translation and therefore cannot result in out of
        //         bounds accesses.
        unsafe { self.entries.get_unchecked_mut(index) }
    }

    /// Drops the last value on the [`ValueStack`].
    #[inline]
    pub fn drop(&mut self, depth: usize) {
        self.stack_ptr -= depth;
    }

    /// Pushes the [`UntypedValue`] to the end of the [`ValueStack`].
    ///
    /// # Note
    ///
    /// - This operation heavily relies on the prior validation of the executed WebAssembly bytecode
    ///   for correctness.
    /// - Especially the stack-depth analysis during compilation with a manual stack extension
    ///   before function call prevents this procedure from panicking.
    #[inline]
    pub fn push(&mut self, entry: UntypedValue) {
        *self.get_release_unchecked_mut(self.stack_ptr) = entry;
        self.stack_ptr += 1;
        if self.stack_ptr > self.max_stack_height {
            self.max_stack_height = self.stack_ptr;
        }
    }

    #[inline]
    pub fn pop(&mut self) -> UntypedValue {
        debug_assert!(self.stack_ptr > 0);
        self.stack_ptr -= 1;
        *self.get_release_unchecked_mut(self.stack_ptr)
    }

    /// Returns the capacity of the [`ValueStack`].
    pub(crate) fn capacity(&self) -> usize {
        self.entries.len()
    }

    /// Returns the current length of the [`ValueStack`].
    pub(crate) fn len(&self) -> usize {
        self.stack_ptr
    }

    /// Reserves enough space for `additional` entries in the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This allows efficiently operating on the [`ValueStack`] through
    /// [`ValueStackPtr`], which requires external resource management.
    ///
    /// Before executing a function, the interpreter calls this function
    /// to guarantee that enough space on the [`ValueStack`] exists for
    /// the correct execution to occur.
    /// For this to be working, we need a stack-depth analysis during Wasm
    /// compilation so that we are aware of all stack-depths for every
    /// function.
    pub fn reserve(&mut self, additional: usize) -> Result<(), TrapCode> {
        let new_len = self
            .len()
            .checked_add(additional)
            .filter(|&new_len| new_len <= self.maximum_len)
            .ok_or_else(|| TrapCode::StackOverflow)?;
        if new_len > self.capacity() {
            // Note: By extending the new length, we effectively double
            // the current value stack length and add the additional flat amount
            // on top. This avoids too many frequent reallocations.
            self.entries
                .extend(core::iter::repeat(UntypedValue::default()).take(new_len));
        }
        Ok(())
    }

    /// Extends the value stack by the `additional` number of zeros.
    ///
    /// # Errors
    ///
    /// If the value stack cannot fit `additional` stack values.
    pub fn extend_zeros(&mut self, additional: usize) {
        let cells = self
            .entries
            .get_mut(self.stack_ptr..)
            .and_then(|slice| slice.get_mut(..additional))
            .unwrap_or_else(|| panic!("did not reserve enough value stack space"));
        cells.fill(UntypedValue::default());
        self.stack_ptr += additional;
        if self.stack_ptr > self.max_stack_height {
            self.max_stack_height = self.stack_ptr;
        }
    }

    /// Drains the remaining value stack.
    ///
    /// # Note
    ///
    /// This API is mostly used when writing results back to the
    /// caller after function execution has finished.
    #[inline]
    pub fn drain(&mut self) -> &[UntypedValue] {
        let len = self.stack_ptr;
        self.stack_ptr = 0;
        &self.entries[0..len]
    }

    /// Returns an exclusive slice to the last `depth` entries in the value stack.
    #[inline]
    pub fn peek_as_slice_mut(&mut self, depth: usize) -> &mut [UntypedValue] {
        let start = self.stack_ptr - depth;
        let end = self.stack_ptr;
        &mut self.entries[start..end]
    }

    /// Clears the [`ValueStack`] entirely.
    ///
    /// # Note
    ///
    /// This is required since sometimes execution can halt in the middle of
    /// function execution which leaves the [`ValueStack`] in an unspecified
    /// state.
    /// Therefore, the [`ValueStack`] is required to be reset before
    /// function execution happens.
    pub fn reset(&mut self) {
        self.stack_ptr = 0;
        self.max_stack_height = 0;
    }
}

/// A pointer on the [`ValueStack`].
///
/// Allows for efficient mutable access to the values of the [`ValueStack`].
///
/// [`ValueStack`]: super::ValueStack
#[derive(Debug, Copy, Clone)]
pub struct ValueStackPtr {
    src: *mut UntypedValue,
    ptr: *mut UntypedValue,
}

unsafe impl Send for ValueStackPtr {}

impl From<*mut UntypedValue> for ValueStackPtr {
    #[inline]
    fn from(ptr: *mut UntypedValue) -> Self {
        Self { src: ptr, ptr }
    }
}

impl ValueStackPtr {
    pub fn new(ptr: *mut UntypedValue) -> ValueStackPtr {
        Self { ptr, src: ptr }
    }

    /// Calculates the distance between two [`ValueStackPtr] in units of [`UntypedValue`].
    #[inline]
    pub fn offset_from(self, other: Self) -> isize {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        unsafe { self.ptr.offset_from(other.ptr) }
    }

    /// Returns the [`UntypedValue`] at the current stack pointer.
    #[must_use]
    #[inline]
    fn get(self) -> UntypedValue {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        unsafe { *self.ptr }
    }

    /// Writes `value` to the cell pointed at by [`ValueStackPtr`].
    #[inline]
    fn set(self, value: UntypedValue) {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        *unsafe { &mut *self.ptr } = value;
    }

    /// Returns a [`ValueStackPtr`] with a pointer value increased by `delta`.
    ///
    /// # Note
    ///
    /// The amount of `delta` is in the number of bytes per [`UntypedValue`].
    #[must_use]
    #[inline]
    pub fn into_add(mut self, delta: usize) -> Self {
        self.inc_by(delta);
        self
    }

    /// Returns a [`ValueStackPtr`] with a pointer value decreased by `delta`.
    ///
    /// # Note
    ///
    /// The amount of `delta` is in the number of bytes per [`UntypedValue`].
    #[must_use]
    #[inline]
    pub fn into_sub(mut self, delta: usize) -> Self {
        self.dec_by(delta);
        self
    }

    /// Returns the last [`UntypedValue`] on the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This has the same effect as [`ValueStackPtr::nth_back`]`(1)`.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    #[must_use]
    pub fn last(self) -> UntypedValue {
        self.nth_back(1)
    }

    /// Peeks the entry at the given depth from the last entry.
    ///
    /// # Note
    ///
    /// Given a `depth` of 1 has the same effect as [`ValueStackPtr::last`].
    ///
    /// A `depth` of 0 is invalid and undefined.
    #[inline]
    #[must_use]
    pub fn nth_back(self, depth: usize) -> UntypedValue {
        self.into_sub(depth).get()
    }

    /// Writes `value` to the n-th [`UntypedValue`] from the back.
    ///
    /// # Note
    ///
    /// Given a `depth` of 1 has the same effect as mutating [`ValueStackPtr::last`].
    ///
    /// A `depth` of 0 is invalid and undefined.
    #[inline]
    pub fn set_nth_back(self, depth: usize, value: UntypedValue) {
        self.into_sub(depth).set(value)
    }

    /// Bumps the [`ValueStackPtr`] of `self` by one.
    #[inline]
    fn inc_by(&mut self, delta: usize) {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        self.ptr = unsafe { self.ptr.add(delta) };
        debug_assert!(self.ptr >= self.src, "stack underflow: {}", delta);
    }

    /// Decreases the [`ValueStackPtr`] of `self` by one.
    #[inline]
    fn dec_by(&mut self, delta: usize) {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        self.ptr = unsafe { self.ptr.sub(delta) };
        debug_assert!(self.ptr >= self.src, "stack underflow");
    }

    /// convert stack pointer to the address number
    #[cfg(feature = "tracing")]
    pub fn to_relative_address(&self) -> u32 {
        crate::mem_index::SP_START - (self.ptr as u32 - self.src as u32)
    }

    /// Pushes the `T` to the end of the [`ValueStack`].
    ///
    /// # Note
    ///
    /// - This operation heavily relies on the prior validation of the executed WebAssembly bytecode
    ///   for correctness.
    /// - Especially the stack-depth analysis during compilation with a manual stack extension
    ///   before function call prevents this procedure from panicking.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn push_as<T>(&mut self, value: T)
    where
        T: Into<UntypedValue>,
    {
        self.push(value.into())
    }

    /// Pushes the [`UntypedValue`] to the end of the [`ValueStack`].
    ///
    /// # Note
    ///
    /// - This operation heavily relies on the prior validation of the executed WebAssembly bytecode
    ///   for correctness.
    /// - Especially the stack-depth analysis during compilation with a manual stack extension
    ///   before function call prevents this procedure from panicking.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn push(&mut self, value: UntypedValue) {
        self.set(value);
        self.inc_by(1);
    }

    /// Drops the last [`UntypedValue`] from the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This operation heavily relies on the prior validation of
    /// the executed WebAssembly bytecode for correctness.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn drop(&mut self) {
        self.dec_by(1);
    }

    #[inline]
    pub fn drop_n(&mut self, n: usize) {
        self.dec_by(n);
    }

    /// Pops the last [`UntypedValue`] from the [`ValueStack`] as `T`.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn pop_as<T>(&mut self) -> T
    where
        T: From<UntypedValue>,
    {
        T::from(self.pop())
    }

    /// Pops the last [`UntypedValue`] from the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This operation heavily relies on the prior validation of
    /// the executed WebAssembly bytecode for correctness.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn pop(&mut self) -> UntypedValue {
        self.dec_by(1);
        self.get()
    }

    /// Pops the last pair of [`UntypedValue`] from the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This operation heavily relies on the prior validation of
    /// the executed WebAssembly bytecode for correctness.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn pop2(&mut self) -> (UntypedValue, UntypedValue) {
        let rhs = self.pop();
        let lhs = self.pop();
        (lhs, rhs)
    }

    /// Pops the last triple of [`UntypedValue`] from the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This operation heavily relies on the prior validation of
    /// the executed WebAssembly bytecode for correctness.
    ///
    /// [`ValueStack`]: super::ValueStack
    #[inline]
    pub fn pop3(&mut self) -> (UntypedValue, UntypedValue, UntypedValue) {
        let (snd, trd) = self.pop2();
        let fst = self.pop();
        (fst, snd, trd)
    }

    /// Evaluates the given closure `f` for the top most stack value.
    #[inline]
    pub fn eval_top<F>(&mut self, f: F)
    where
        F: FnOnce(UntypedValue) -> UntypedValue,
    {
        let last = self.into_sub(1);
        last.set(f(last.get()))
    }

    /// Evaluates the given closure `f` for the 2 top most stack values.
    #[inline]
    pub fn eval_top2<F>(&mut self, f: F)
    where
        F: FnOnce(UntypedValue, UntypedValue) -> UntypedValue,
    {
        let rhs = self.pop();
        let last = self.into_sub(1);
        let lhs = last.get();
        last.set(f(lhs, rhs));
    }

    /// Evaluates the given closure `f` for the 3 top most stack values.
    #[inline]
    pub fn eval_top3<F>(&mut self, f: F)
    where
        F: FnOnce(UntypedValue, UntypedValue, UntypedValue) -> UntypedValue,
    {
        let (e2, e3) = self.pop2();
        let last = self.into_sub(1);
        let e1 = last.get();
        last.set(f(e1, e2, e3));
    }

    /// Evaluates the given fallible closure `f` for the top most stack value.
    ///
    /// # Errors
    ///
    /// If the closure execution fails.
    #[inline]
    pub fn try_eval_top<F>(&mut self, f: F) -> Result<(), TrapCode>
    where
        F: FnOnce(UntypedValue) -> Result<UntypedValue, TrapCode>,
    {
        let last = self.into_sub(1);
        last.set(f(last.get())?);
        Ok(())
    }

    /// Evaluates the given fallible closure `f` for the 2 top most stack values.
    ///
    /// # Errors
    ///
    /// If the closure execution fails.
    #[inline]
    pub fn try_eval_top2<F>(&mut self, f: F) -> Result<(), TrapCode>
    where
        F: FnOnce(UntypedValue, UntypedValue) -> Result<UntypedValue, TrapCode>,
    {
        let rhs = self.pop();
        let last = self.into_sub(1);
        let lhs = last.get();
        last.set(f(lhs, rhs)?);
        Ok(())
    }

    pub fn push_f32(&mut self, value: F32) {
        self.push(value.into());
    }

    pub fn pop_f32(&mut self) -> F32 {
        self.pop().as_f32()
    }

    pub fn push_f64(&mut self, value: F64) {
        let bits = value.to_bits();
        let lo = bits as i32;
        self.push(lo.into());
        let hi = (bits >> 32) as i32;
        self.push(hi.into());
    }

    pub fn pop_f64(&mut self) -> F64 {
        let (lo, hi) = self.pop2();
        F64::from_bits(((hi.as_u64()) << 32) | (lo.as_u64()))
    }

    pub fn push_value(&mut self, value: &Value) {
        match value {
            Value::I32(value) => self.push_i32(*value),
            Value::I64(value) => self.push_i64(*value),
            Value::F32(value) => self.push_f32(*value),
            Value::F64(value) => self.push_f64(*value),
            Value::FuncRef(value) => self.push_i32(value.0 as i32),
            Value::ExternRef(value) => self.push_i32(value.0 as i32),
        }
    }

    pub fn push_i32(&mut self, value: i32) {
        self.push(value.into());
    }

    pub fn pop_value(&mut self, value_type: ValType) -> Value {
        match value_type {
            ValType::I32 => Value::I32(self.pop_i32()),
            ValType::I64 => Value::I64(self.pop_i64()),
            ValType::F32 => Value::F32(self.pop_f32()),
            ValType::F64 => Value::F64(self.pop_f64()),
            ValType::V128 => unreachable!("can't invoke syscall with v128"),
            ValType::FuncRef => Value::FuncRef(FuncRef::new(self.pop_i32() as u32)),
            ValType::ExternRef => Value::ExternRef(ExternRef::new(self.pop_i32() as u32)),
        }
    }

    pub fn pop_i32(&mut self) -> i32 {
        self.pop().as_i32()
    }

    pub fn push_i64(&mut self, value: i64) {
        let (lo, hi) = value.split_into_i32_tuple();
        self.push(lo.into());
        self.push(hi.into());
    }

    pub fn pop_i64(&mut self) -> i64 {
        let (lo, hi) = self.pop2();
        (hi.as_i64() << 32) | lo.as_i64()
    }
}
