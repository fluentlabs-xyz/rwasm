use crate::types::{
    DropKeep,
    RwasmError,
    UntypedValue,
    DEFAULT_MAX_VALUE_STACK_HEIGHT,
    DEFAULT_MIN_VALUE_STACK_HEIGHT,
};
use alloc::{vec, vec::Vec};
use core::fmt::Debug;

/// The value stack used to execute Wasm bytecode.
///
/// # Note
///
/// The [`ValueStack`] implementation heavily relies on the prior
/// validation of the executed Wasm bytecode for correct execution.
#[derive(Clone)]
pub struct ValueStack {
    /// All currently live stack entries.
    entries: Vec<UntypedValue>,
    /// Index of the first free place in the stack.
    stack_ptr: usize,
    /// The maximum value stack height.
    ///
    /// # Note
    ///
    /// Extending the value stack beyond this limit during execution
    /// will cause a stack overflow trap.
    maximum_len: usize,
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

impl Default for ValueStack {
    fn default() -> Self {
        let register_len = size_of::<UntypedValue>();
        Self::new(
            DEFAULT_MIN_VALUE_STACK_HEIGHT / register_len,
            DEFAULT_MAX_VALUE_STACK_HEIGHT / register_len,
        )
    }
}

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

impl ValueStack {
    /// Creates an empty [`ValueStack`] that does not allocate heap memor.
    ///
    /// # Note
    ///
    /// This is required for resumable functions in order to replace their
    /// proper stack with a cheap dummy one.
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
            stack_ptr: 0,
            maximum_len: 0,
        }
    }

    /// Returns the current [`ValueStackPtr`] of `self`.
    ///
    /// The returned [`ValueStackPtr`] points to the top most value on the [`ValueStack`].
    #[inline]
    pub fn stack_ptr(&mut self) -> ValueStackPtr {
        self.base_ptr().into_add(self.stack_ptr)
    }

    pub fn stack_len(&mut self, sp: ValueStackPtr) -> usize {
        let base = self.base_ptr();
        sp.offset_from(base) as usize
    }

    pub fn has_stack_overflowed(&mut self, sp: ValueStackPtr) -> bool {
        self.stack_len(sp) > self.maximum_len
    }

    pub fn dump_stack(&mut self, sp: ValueStackPtr) -> Vec<UntypedValue> {
        let size = self.stack_len(sp);
        self.entries[0..size.min(self.entries.len())].to_vec()
    }

    /// Returns the base [`ValueStackPtr`] of `self`.
    ///
    /// The returned [`ValueStackPtr`] points to the first value on the [`ValueStack`].
    #[inline]
    fn base_ptr(&mut self) -> ValueStackPtr {
        ValueStackPtr::new(self.entries.as_mut_ptr(), self.entries.len())
    }

    /// Synchronizes [`ValueStack`] with the new [`ValueStackPtr`].
    #[inline]
    pub fn sync_stack_ptr(&mut self, new_sp: ValueStackPtr) {
        let offset = new_sp.offset_from(self.base_ptr());
        self.stack_ptr = offset as usize;
    }

    /// Returns `true` if the [`ValueStack`] is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.capacity() == 0
    }

    /// Creates a new empty [`ValueStack`].
    ///
    /// # Panics
    ///
    /// - If the `initial_len` is zero.
    /// - If the `initial_len` is greater than `maximum_len`.
    pub fn new(initial_len: usize, maximum_len: usize) -> Self {
        assert!(
            initial_len > 0,
            "cannot initialize the value stack with zero length",
        );
        assert!(
            initial_len <= maximum_len,
            "initial value stack length is greater than maximum value stack length",
        );
        let entries = vec![UntypedValue::default(); initial_len];
        Self {
            entries,
            stack_ptr: 0,
            maximum_len,
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
    /// during translation and therefore cannot result in out of
    /// bounds accesses.
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

    /// Extends the value stack by the `additional` amount of zeros.
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
    }

    /// Prepares the [`ValueStack`] for execution of the given Wasm function.
    pub fn prepare_wasm_call(
        &mut self,
        max_stack_height: usize,
        len_locals: usize,
    ) -> Result<(), RwasmError> {
        self.reserve(max_stack_height)?;
        self.extend_zeros(len_locals);
        Ok(())
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
    }

    /// Returns the capacity of the [`ValueStack`].
    fn capacity(&self) -> usize {
        self.entries.len()
    }

    /// Returns the current length of the [`ValueStack`].
    fn len(&self) -> usize {
        self.stack_ptr
    }

    /// Reserves enough space for `additional` entries in the [`ValueStack`].
    ///
    /// # Note
    ///
    /// This allows to efficiently operate on the [`ValueStack`] through
    /// [`ValueStackPtr`] which requires external resource management.
    ///
    /// Before executing a function the interpreter calls this function
    /// to guarantee that enough space on the [`ValueStack`] exists for
    /// correct execution to occur.
    /// For this to be working we need a stack-depth analysis during Wasm
    /// compilation so that we are aware of all stack-depths for every
    /// functions.
    pub fn reserve(&mut self, additional: usize) -> Result<(), RwasmError> {
        let new_len = self
            .len()
            .checked_add(additional)
            .filter(|&new_len| new_len <= self.maximum_len)
            .ok_or_else(|| RwasmError::StackOverflow)?;
        if new_len > self.capacity() {
            // Note: By extending the new length, we effectively double
            // the current value stack length and add the additional flat amount
            // on top. This avoids too many frequent reallocations.
            self.entries
                .extend(core::iter::repeat(UntypedValue::default()).take(new_len));
        }
        Ok(())
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
    /// state. Therefore the [`ValueStack`] is required to be reset before
    /// function execution happens.
    pub fn reset(&mut self) {
        self.stack_ptr = 0;
    }
}

/// A pointer on the [`ValueStack`].
///
/// Allows for efficient mutable access to the values of the [`ValueStack`].
///
/// [`ValueStack`]: super::ValueStack
#[derive(Debug, Copy, Clone)]
// #[repr(transparent)]
pub struct ValueStackPtr {
    src: *mut UntypedValue,
    ptr: *mut UntypedValue,
    // len: usize,
}

impl From<*mut UntypedValue> for ValueStackPtr {
    #[inline]
    fn from(ptr: *mut UntypedValue) -> Self {
        Self {
            src: ptr,
            ptr,
            // len: usize::MAX,
        }
    }
}

impl ValueStackPtr {
    pub fn new(ptr: *mut UntypedValue, _len: usize) -> ValueStackPtr {
        // Self { src: ptr, ptr, len }
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
    pub (crate) fn  get(self) -> UntypedValue {
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
    /// The amount of `delta` is in number of bytes per [`UntypedValue`].
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
    /// The amount of `delta` is in number of bytes per [`UntypedValue`].
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
        debug_assert!(self.ptr >= self.src, "stack underflow");
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
    pub fn try_eval_top<F>(&mut self, f: F) -> Result<(), RwasmError>
    where
        F: FnOnce(UntypedValue) -> Result<UntypedValue, RwasmError>,
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
    pub fn try_eval_top2<F>(&mut self, f: F) -> Result<(), RwasmError>
    where
        F: FnOnce(UntypedValue, UntypedValue) -> Result<UntypedValue, RwasmError>,
    {
        let rhs = self.pop();
        let last = self.into_sub(1);
        let lhs = last.get();
        last.set(f(lhs, rhs)?);
        Ok(())
    }

    /// Drops some amount of entries and keeps some amount of them at the new top.
    ///
    /// # Note
    ///
    /// For an amount of entries to keep `k` and an amount of entries to drop `d`
    /// this has the following effect on stack `s` and stack pointer `sp`.
    ///
    /// 1) Copy `k` elements from indices starting at `sp - k` to `sp - k - d`.
    /// 2) Adjust stack pointer: `sp -= d`
    ///
    /// After this operation the value stack will have `d` fewer entries and the
    /// top `k` entries are the top `k` entries before this operation.
    ///
    /// Note that `k + d` cannot be greater than the stack length.
    pub fn drop_keep(&mut self, drop_keep: DropKeep) {
        fn drop_keep_impl(this: ValueStackPtr, drop_keep: DropKeep) {
            let keep = drop_keep.keep;
            if keep == 0 {
                // Case: no values need to be kept.
                return;
            }
            let keep = keep as usize;
            let src = this.into_sub(keep);
            let dst = this.into_sub(keep + drop_keep.drop as usize);
            if keep == 1 {
                // Case: only one value needs to be kept.
                dst.set(src.get());
                return;
            }
            // Case: many values need to be kept and moved on the stack.
            for i in 0..keep {
                dst.into_add(i).set(src.into_add(i).get());
            }
        }

        let drop = drop_keep.drop;
        if drop == 0 {
            // Nothing to do in this case.
            return;
        }
        drop_keep_impl(*self, drop_keep);
        self.dec_by(drop as usize);
    }
     #[inline]
    pub fn to_position(&self)->u32{
       unsafe {
            (*self.src).as_u32()
       }
    }
}
