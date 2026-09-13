use crate::types::Opcode;

/// The instruction pointer to the instruction of a function on the call stack.
///
/// Module construction validates static instruction targets and fallthroughs. The executor checks
/// that result at entry and bounds-checks dynamic indirect-call targets, so pointer movement and
/// fetch remain unchecked here, including for hand-built and decoded modules.
#[derive(Debug, Copy, Clone, PartialEq)]
#[repr(transparent)]
pub(crate) struct InstructionPtr {
    /// The pointer to the instruction.
    pub(crate) ptr: *const Opcode,
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
    /// Creates a new [`rwasm::engine::code_map::InstructionPtr`] for `instr`.
    #[inline]
    pub(crate) fn new(ptr: *const Opcode) -> Self {
        Self { ptr }
    }

    /// Offset the [`rwasm::engine::code_map::InstructionPtr`] by the given value.
    ///
    /// # Safety
    ///
    /// The caller is responsible for calling this method only with valid
    /// offset values so that the [`rwasm::engine::code_map::InstructionPtr`] never points out of
    /// valid bounds of the instructions of the same compiled Wasm function.
    #[inline(always)]
    pub(crate) fn offset(&mut self, by: isize) {
        // SAFETY: Module validation and the indirect-call guards establish in-allocation targets.
        self.ptr = unsafe { self.ptr.offset(by) };
    }

    #[inline(always)]
    pub(crate) fn add(&mut self, delta: usize) {
        // SAFETY: Validated successors/payloads stay within the allocation (or one-past for a
        // tail call that immediately replaces the pointer without fetching from it).
        self.ptr = unsafe { self.ptr.add(delta) };
    }

    /// Returns the currently pointed at [`Opcode`].
    ///
    /// # Safety
    ///
    /// The caller is responsible for calling this method only when it is
    /// guaranteed that the [`rwasm::engine::code_map::InstructionPtr`] is validly pointing inside
    /// the boundaries of its associated compiled Wasm function.
    #[inline(always)]
    pub(crate) fn get(&self) -> Opcode {
        // SAFETY: Entry, static successor and indirect-call checks establish a valid instruction.
        unsafe { *self.ptr }
    }

    #[cfg(feature = "tracing")]
    pub(crate) fn is_valid(self, max: u64) -> bool {
        self.ptr as u64 <= max
    }
}
