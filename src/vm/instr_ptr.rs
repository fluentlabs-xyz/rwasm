use crate::types::{Opcode, OpcodeData};

/// The instruction pointer to the instruction of a function on the call stack.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct InstructionPtr {
    /// The pointer to the instruction.
    pub(crate) ptr: *const (Opcode, OpcodeData),
    pub(crate) src: *const (Opcode, OpcodeData),
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
    pub fn new(ptr: *const (Opcode, OpcodeData)) -> Self {
        Self { ptr, src: ptr }
    }

    #[inline(always)]
    pub fn pc(&self) -> u32 {
        let size = size_of::<(Opcode, OpcodeData)>() as u32;
        let diff = self.ptr as u32 - self.src as u32;
        diff / size
    }

    /// Offset the [`rwasm::engine::code_map::InstructionPtr`] by the given value.
    ///
    /// # Safety
    ///
    /// The caller is responsible for calling this method only with valid
    /// offset values so that the [`rwasm::engine::code_map::InstructionPtr`] never points out of
    /// valid bounds of the instructions of the same compiled Wasm function.
    #[inline(always)]
    pub fn offset(&mut self, by: isize) {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        self.ptr = unsafe { self.ptr.offset(by) };
    }

    #[inline(always)]
    pub fn add(&mut self, delta: usize) {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        self.ptr = unsafe { self.ptr.add(delta) };
    }

    /// Returns a shared reference to the currently pointed at [`Opcode`].
    ///
    /// # Safety
    ///
    /// The caller is responsible for calling this method only when it is
    /// guaranteed that the [`rwasm::engine::code_map::InstructionPtr`] is validly pointing inside
    /// the boundaries of its associated compiled Wasm function.
    #[inline(always)]
    pub fn get(&self) -> Opcode {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        unsafe { &*self.ptr }.0
    }

    #[inline(always)]
    pub fn data(&self) -> &OpcodeData {
        // SAFETY: Within Wasm bytecode execution we are guaranteed by
        //         Wasm validation and `rwasm` codegen to never run out
        //         of valid bounds using this method.
        &unsafe { &*self.ptr }.1
    }
}
