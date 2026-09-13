use crate::types::Opcode;

/// The instruction pointer to the instruction of a function on the call stack.
///
/// # Note
///
/// The pointer carries the `[src, end)` window of the code section it was created for. Every
/// movement is bounds-checked against that window in **all** build profiles: a displacement that
/// leaves the window neither moves the pointer nor reads memory — the pointer is parked on the
/// code base and [`InstructionPtr::is_out_of_bounds`] starts reporting `true`, which the
/// interpreter turns into a [`crate::TrapCode::UnreachableCodeReached`] trap before the next
/// instruction runs.
///
/// The previous representation was a bare `*const Opcode` moved with `offset`/`add` and
/// dereferenced unconditionally. Bytecode is a trusted artifact of the compiler, but the module
/// builder and the decode entry points are safe public API, and a `Br`/`BrTable`/`CallInternal`
/// immediate that was not produced by the compiler used to fetch instructions from outside the
/// code section — an out-of-bounds read that aborts the process.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct InstructionPtr {
    /// First instruction of the code section.
    pub(crate) src: *const Opcode,
    /// The instruction currently pointed at.
    pub(crate) ptr: *const Opcode,
    /// One past the last instruction of the code section.
    pub(crate) end: *const Opcode,
    /// Sticky flag raised once a displacement left the code section.
    pub(crate) out_of_bounds: bool,
}

/// It is safe to send an [`rwasm::engine::code_map::InstructionPtr`] to another thread.
///
/// The access to the pointed-to [`Opcode`] is read-only and
/// [`Opcode`] itself is [`Send`].
///
/// However, it is not safe to share an [`rwasm::engine::code_map::InstructionPtr`] between threads
/// due to their [`rwasm::engine::code_map::InstructionPtr::offset`] method which relinks the
/// internal pointer and is not synchronized.
unsafe impl Send for InstructionPtr {}

impl InstructionPtr {
    /// Creates an [`InstructionPtr`] for a code section of `len` instructions starting at `base`.
    #[inline]
    pub fn new(base: *const Opcode, len: usize) -> Self {
        Self {
            src: base,
            ptr: base,
            end: base.wrapping_add(len),
            out_of_bounds: false,
        }
    }

    /// Returns `true` if some displacement left the code section.
    ///
    /// # Note
    ///
    /// The offending move was suppressed, so this only reports that the executed bytecode is
    /// invalid and that execution has to be aborted with
    /// [`crate::TrapCode::UnreachableCodeReached`].
    #[inline]
    pub fn is_out_of_bounds(self) -> bool {
        self.out_of_bounds
    }

    /// Returns the instruction offset of the current position inside the code section.
    #[inline]
    pub(crate) fn position(self) -> usize {
        (self.ptr as usize).wrapping_sub(self.src as usize) / size_of::<Opcode>()
    }

    /// Number of instructions the code section holds.
    #[inline]
    fn capacity(self) -> usize {
        (self.end as usize).wrapping_sub(self.src as usize) / size_of::<Opcode>()
    }

    /// Records an out-of-bounds move and parks the pointer on the code base.
    ///
    /// # Note
    ///
    /// Parking keeps every follow-up operation harmless until the interpreter observes the flag
    /// and traps.
    #[cold]
    #[inline]
    fn mark_out_of_bounds(&mut self) {
        self.out_of_bounds = true;
        self.ptr = self.src;
    }

    /// Offset the [`InstructionPtr`] by the given value.
    ///
    /// A displacement that leaves the code section is refused and records an out-of-bounds move
    /// instead of relinking the pointer ([`InstructionPtr::is_out_of_bounds`]).
    #[inline(always)]
    pub fn offset(&mut self, by: isize) {
        if by >= 0 {
            self.add(by as usize);
        } else {
            self.sub(by.unsigned_abs());
        }
    }

    #[inline(always)]
    pub fn add(&mut self, delta: usize) {
        match self.position().checked_add(delta).filter(|&it| it < self.capacity()) {
            Some(position) => self.ptr = unsafe { self.src.add(position) },
            None => self.mark_out_of_bounds(),
        }
    }

    #[inline(always)]
    fn sub(&mut self, delta: usize) {
        match self.position().checked_sub(delta) {
            Some(position) => self.ptr = unsafe { self.src.add(position) },
            None => self.mark_out_of_bounds(),
        }
    }

    /// Returns the currently pointed at [`Opcode`].
    ///
    /// # Note
    ///
    /// A pointer that does not address an instruction of its code section reads no memory: the
    /// call records the out-of-bounds state and yields [`Opcode::Unreachable`] so that the
    /// interpreter traps before executing anything built from foreign bytes.
    #[inline(always)]
    pub fn get(&mut self) -> Opcode {
        if self.ptr >= self.end || self.end.is_null() {
            self.mark_out_of_bounds();
            return Opcode::Unreachable;
        }
        // SAFETY: the check above proves that `ptr` addresses an instruction inside `[src, end)`,
        // the code section this pointer was created for.
        unsafe { *self.ptr }
    }
}
