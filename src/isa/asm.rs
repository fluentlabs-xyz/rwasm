//! A tiny structured assembler for hand-written snippet bodies.
//!
//! Snippet bodies are flat rwasm opcode sequences that address values by stack depth
//! (`LocalGet`/`LocalSet` take a depth relative to the current stack top) and branch by relative
//! instruction offsets. Writing such code by hand means recomputing every depth after every push
//! and recounting every branch distance after every edit, which is how the machine-traced bodies
//! ended up unmaintainable.
//!
//! [`SnippetAsm`] removes both failure modes:
//!
//! - **Named slots.** The assembler tracks the virtual stack height as opcodes are emitted (every
//!   opcode has a known arity) and lets the body name stack slots. `get`/`set`/`tee` translate a
//!   slot name into the correct depth for the current height.
//! - **Labels.** Branches target named labels; relative offsets are resolved in [`finish`].
//!   Each label pins the stack height, and every branch to it asserts the same height, so an
//!   unbalanced path panics at emission time (i.e., in tests) instead of corrupting the stack at
//!   run time.
//!
//! [`finish`]: SnippetAsm::finish

use crate::{InstructionSet, Opcode, TrapCode, SNIPPET_FUNC_IDX_UNRESOLVED};
use alloc::{collections::BTreeMap, vec::Vec};

pub(crate) struct SnippetAsm<'a> {
    is: &'a mut InstructionSet,
    /// Current number of stack slots in the snippet frame (inputs + everything pushed above).
    height: u32,
    /// Highest value `height` has reached so far.
    max_height: u32,
    /// Names for the bottom `slots.len()` slots of the frame (bottom → top). Slots above the
    /// last named one are anonymous temporaries.
    slots: Vec<&'static str>,
    /// Resolved labels: name → (instruction index, stack height).
    labels: BTreeMap<&'static str, (u32, u32)>,
    /// Emitted branches waiting for their label: (branch instruction index, label name).
    fixups: Vec<(u32, &'static str)>,
    /// Stack heights promised to not-yet-defined labels by branches targeting them.
    pending_heights: BTreeMap<&'static str, u32>,
}

macro_rules! impl_binary_alu {
    ($( fn $name:ident => $op:ident; )*) => {
        $(
            pub fn $name(&mut self) {
                paste::paste! { self.is.[< op_ $op >](); }
                self.shrink(1);
            }
        )*
    };
}

impl<'a> SnippetAsm<'a> {
    /// Starts a snippet body whose frame begins with the given named input slots.
    pub fn new(is: &'a mut InstructionSet, inputs: &[&'static str]) -> Self {
        Self {
            is,
            height: inputs.len() as u32,
            max_height: inputs.len() as u32,
            slots: inputs.to_vec(),
            labels: BTreeMap::new(),
            fixups: Vec::new(),
            pending_heights: BTreeMap::new(),
        }
    }

    fn grow(&mut self, n: u32) {
        self.height += n;
        self.max_height = self.max_height.max(self.height);
    }

    fn shrink(&mut self, n: u32) {
        assert!(self.height >= n, "snippet stack underflow");
        self.height -= n;
        self.slots.truncate(self.height as usize);
    }

    fn slot_index(&self, name: &'static str) -> u32 {
        self.slots
            .iter()
            .position(|slot| *slot == name)
            .unwrap_or_else(|| panic!("unknown snippet slot `{name}`")) as u32
    }

    /// Names the anonymous temporary currently on top of the stack.
    pub fn def(&mut self, name: &'static str) {
        assert_eq!(
            self.height as usize,
            self.slots.len() + 1,
            "`def` expects exactly one anonymous temporary on top of the stack"
        );
        assert!(!self.slots.contains(&name), "slot `{name}` already defined");
        self.slots.push(name);
    }

    /// Renames a named slot (e.g. after a call repurposes input slots as results).
    pub fn rename(&mut self, from: &'static str, to: &'static str) {
        assert!(!self.slots.contains(&to), "slot `{to}` already defined");
        let index = self.slot_index(from) as usize;
        self.slots[index] = to;
    }

    /// Pushes a copy of the named slot.
    pub fn get(&mut self, name: &'static str) {
        let depth = self.height - self.slot_index(name);
        self.is.op_local_get(depth);
        self.grow(1);
    }

    /// Pops the top of the stack into the named slot.
    pub fn set(&mut self, name: &'static str) {
        let index = self.slot_index(name);
        assert!(index < self.height - 1, "`set` would pop the target slot");
        self.is.op_local_set(self.height - 1 - index);
        self.shrink(1);
    }

    /// Copies the top of the stack into the named slot without popping.
    pub fn tee(&mut self, name: &'static str) {
        let index = self.slot_index(name);
        assert!(index < self.height, "`tee` target must be below the top");
        self.is.op_local_tee(self.height - index);
    }

    pub fn i32_const(&mut self, value: i32) {
        self.is.op_i32_const(value);
        self.grow(1);
    }

    pub fn drop(&mut self) {
        self.is.op_drop();
        self.shrink(1);
    }

    pub fn trap(&mut self, trap_code: TrapCode) {
        self.is.op_trap(trap_code);
    }

    pub fn i32_eqz(&mut self) {
        self.is.op_i32_eqz();
    }

    impl_binary_alu! {
        fn i32_add => i32_add;
        fn i32_sub => i32_sub;
        fn i32_and => i32_and;
        fn i32_or => i32_or;
        fn i32_xor => i32_xor;
        fn i32_shl => i32_shl;
        fn i32_shr_u => i32_shr_u;
        fn i32_eq => i32_eq;
        fn i32_ne => i32_ne;
        fn i32_lt_u => i32_lt_u;
        fn i32_gt_u => i32_gt_u;
        fn i32_ge_u => i32_ge_u;
        fn i32_gt_s => i32_gt_s;
        fn i32_div_u => i32_div_u;
        fn i32_rem_u => i32_rem_u;
    }

    /// Emits a `CallInternal` to another snippet, to be resolved by `emit_snippets`. The callee
    /// must replace the slots it consumes one-for-one, so height and slot names stay unchanged;
    /// callers typically `rename` the result slots afterwards.
    pub fn call_snippet_unresolved(&mut self) {
        self.is.op_call_internal(SNIPPET_FUNC_IDX_UNRESOLVED);
    }

    /// Splices raw instructions emitted by `body` with a net-zero stack effect (slots replaced
    /// one-for-one, names stay valid) and a declared transient growth of at most `max_growth`
    /// slots above the current height.
    pub fn splice<R>(&mut self, max_growth: u32, body: impl FnOnce(&mut InstructionSet) -> R) -> R {
        let result = body(self.is);
        self.max_height = self.max_height.max(self.height + max_growth);
        result
    }

    /// Defines `name` at the current instruction. The stack height here must match the height
    /// at every branch targeting this label.
    pub fn label(&mut self, name: &'static str) {
        assert!(
            !self.labels.contains_key(name),
            "label `{name}` already defined"
        );
        if let Some(expected) = self.pending_heights.remove(name) {
            let fallthrough_is_dead = matches!(
                self.is.last(),
                Some(Opcode::Br(_) | Opcode::Trap(_) | Opcode::Return)
            );
            if fallthrough_is_dead {
                self.height = expected;
                self.slots.truncate(self.height as usize);
            } else {
                assert_eq!(
                    expected, self.height,
                    "label `{name}` reached with mismatched stack heights"
                );
            }
        }
        self.labels
            .insert(name, (self.is.len() as u32, self.height));
    }

    fn expect_label_height(&mut self, name: &'static str) {
        if let Some((_, height)) = self.labels.get(name) {
            assert_eq!(
                *height, self.height,
                "branch to `{name}` with mismatched stack height"
            );
        } else if let Some(height) = self.pending_heights.get(name) {
            assert_eq!(
                *height, self.height,
                "branch to `{name}` with mismatched stack height"
            );
        } else {
            self.pending_heights.insert(name, self.height);
        }
    }

    pub fn br(&mut self, name: &'static str) {
        self.fixups.push((self.is.len() as u32, name));
        self.is.op_br(0);
        self.expect_label_height(name);
    }

    pub fn br_if_nez(&mut self, name: &'static str) {
        self.fixups.push((self.is.len() as u32, name));
        self.is.op_br_if_nez(0);
        self.shrink(1);
        self.expect_label_height(name);
    }

    pub fn br_if_eqz(&mut self, name: &'static str) {
        self.fixups.push((self.is.len() as u32, name));
        self.is.op_br_if_eqz(0);
        self.shrink(1);
        self.expect_label_height(name);
    }

    /// Resolves all branches and returns the maximum stack height the body can reach.
    ///
    /// # Panics
    ///
    /// - If any branch targets a label that was never defined.
    /// - If a recorded fixup site no longer holds a branch opcode.
    pub fn finish(self) -> u32 {
        assert!(
            self.pending_heights.is_empty(),
            "undefined snippet labels: {:?}",
            self.pending_heights.keys().collect::<Vec<_>>()
        );
        for (loc, name) in self.fixups {
            let (target, _) = self.labels[name];
            let offset = target as i64 - loc as i64;
            match &mut self.is.instr[loc as usize] {
                op @ (Opcode::Br(_) | Opcode::BrIfEqz(_) | Opcode::BrIfNez(_)) => {
                    op.update_branch_offset(offset as i32);
                }
                op => panic!("fixup at {loc} is not a branch: {op:?}"),
            }
        }
        self.max_height
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Slots resolve to depths relative to the *current* height, so the same name must yield a
    /// different `LocalGet` depth once more values sit above it.
    #[test]
    fn slot_depth_tracks_stack_growth() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a", "b"]);
        asm.get("a");
        asm.i32_const(7);
        asm.get("a");
        asm.finish();
        assert_eq!(
            is.instr,
            vec![
                Opcode::LocalGet(2),
                Opcode::I32Const(7.into()),
                Opcode::LocalGet(4),
            ]
        );
    }

    /// A backward branch gets a negative offset, a forward branch a positive one, both relative
    /// to the branch instruction itself.
    #[test]
    fn labels_resolve_to_relative_offsets() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a"]);
        asm.label("top");
        asm.get("a");
        asm.br_if_nez("top");
        asm.get("a");
        asm.br("done");
        asm.i32_const(0);
        asm.drop();
        asm.label("done");
        let max_height = asm.finish();
        assert_eq!(is.instr[1], Opcode::BrIfNez((-1i32).into()));
        assert_eq!(is.instr[3], Opcode::Br(3i32.into()));
        // Peak height, reached by the `i32_const` that the `drop` then pops.
        assert_eq!(max_height, 3);
    }

    #[test]
    fn max_height_is_the_peak_not_the_final_height() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a"]);
        asm.i32_const(1);
        asm.i32_const(2);
        asm.i32_add();
        asm.drop();
        assert_eq!(asm.finish(), 3);
    }

    #[test]
    #[should_panic(expected = "unknown snippet slot `nope`")]
    fn unknown_slot_panics() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a"]);
        asm.get("nope");
    }

    #[test]
    #[should_panic(expected = "undefined snippet labels")]
    fn undefined_label_panics() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a"]);
        asm.get("a");
        asm.br_if_nez("never_defined");
        asm.finish();
    }

    /// A live fall-through into a label must agree with the height promised by branches to it;
    /// disagreement means one path leaves the stack unbalanced.
    #[test]
    #[should_panic(expected = "mismatched stack heights")]
    fn mismatched_label_height_panics() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a"]);
        asm.get("a");
        asm.br_if_nez("join");
        asm.i32_const(1);
        asm.label("join");
        asm.finish();
    }

    /// The fixup sites recorded by the assembler must still hold branch opcodes; rewriting the
    /// buffer underneath it is a programming error, not a silently-wrong encoding.
    #[test]
    #[should_panic(expected = "is not a branch")]
    fn fixup_over_non_branch_panics() {
        let mut is = InstructionSet::new();
        let mut asm = SnippetAsm::new(&mut is, &["a"]);
        asm.get("a");
        asm.br_if_nez("done");
        asm.label("done");
        asm.is.instr[1] = Opcode::Drop;
        asm.finish();
    }
}
