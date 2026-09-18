use crate::InstructionSet;
use alloc::vec::Vec;
use wasmparser::{FuncType, ValType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Snippet {
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64RotL,
    I64RotR,
    /// Shared unsigned 64-bit divide-and-remainder core. Never emitted for a wasm opcode
    /// directly; the four div/rem snippets call into it (see [`Snippet::dependencies`]).
    UDivMod64,
}

#[derive(Debug)]
struct SnippetDefinition {
    pub emitter: fn(&mut InstructionSet),
    /// Emitter for the snippets-off mode, where the body is spliced at the call site and
    /// therefore must not contain unresolved snippet-to-snippet calls. Identical to `emitter`
    /// for self-contained snippets.
    pub inline_emitter: fn(&mut InstructionSet),
    pub max_stack_height: u32,
    pub orig_params: &'static [ValType],
    pub orig_results: &'static [ValType],
    /// Snippets this snippet's body calls via `CallInternal`; resolved by `emit_snippets`.
    pub dependencies: &'static [Snippet],
}

macro_rules! define_snippet {
    ($emitter:ident, $max_stack_height:ident, $params:expr, $results:expr) => {
        define_snippet!(
            $emitter,
            $emitter,
            $max_stack_height,
            $params,
            $results,
            &[]
        )
    };
    ($emitter:ident, $inline_emitter:ident, $max_stack_height:ident, $params:expr, $results:expr, $dependencies:expr) => {{
        static DEF: SnippetDefinition = SnippetDefinition {
            emitter: InstructionSet::$emitter,
            inline_emitter: InstructionSet::$inline_emitter,
            max_stack_height: InstructionSet::$max_stack_height,
            orig_params: $params,
            orig_results: $results,
            dependencies: $dependencies,
        };
        &DEF
    }};
}

impl Snippet {
    /// Every snippet, in declaration order.
    pub const ALL: [Snippet; 23] = [
        Snippet::I64Eq,
        Snippet::I64Ne,
        Snippet::I64LtS,
        Snippet::I64LtU,
        Snippet::I64GtS,
        Snippet::I64GtU,
        Snippet::I64LeS,
        Snippet::I64LeU,
        Snippet::I64GeS,
        Snippet::I64GeU,
        Snippet::I64Add,
        Snippet::I64Sub,
        Snippet::I64Mul,
        Snippet::I64DivS,
        Snippet::I64DivU,
        Snippet::I64RemS,
        Snippet::I64RemU,
        Snippet::I64Shl,
        Snippet::I64ShrS,
        Snippet::I64ShrU,
        Snippet::I64RotL,
        Snippet::I64RotR,
        Snippet::UDivMod64,
    ];

    /// The largest `StackCheck` any snippet reserves on top of its operands, i.e. the deepest
    /// frame a single `i64` operator can hide behind a `CallInternal`. A wrapper's peak already
    /// includes the snippets it calls (`MSH_I64_DIV_S` covers `UDivMod64`), so this is the whole
    /// hidden frame. The runtime value stack keeps this much room above `N_MAX_STACK_SIZE`
    /// (`N_STACK_TRAMPOLINE_HEADROOM`); `max_stack_height_is_pinned` keeps the list complete.
    pub const MAX_STACK_HEIGHT: u32 = {
        let peaks = [
            InstructionSet::MSH_I64_EQ,
            InstructionSet::MSH_I64_NE,
            InstructionSet::MSH_I64_LT_S,
            InstructionSet::MSH_I64_LT_U,
            InstructionSet::MSH_I64_GT_S,
            InstructionSet::MSH_I64_GT_U,
            InstructionSet::MSH_I64_LE_S,
            InstructionSet::MSH_I64_LE_U,
            InstructionSet::MSH_I64_GE_S,
            InstructionSet::MSH_I64_GE_U,
            InstructionSet::MSH_I64_ADD,
            InstructionSet::MSH_I64_SUB,
            InstructionSet::MSH_I64_MUL,
            InstructionSet::MSH_I64_DIV_S,
            InstructionSet::MSH_I64_DIV_U,
            InstructionSet::MSH_I64_REM_S,
            InstructionSet::MSH_I64_REM_U,
            InstructionSet::MSH_I64_SHL,
            InstructionSet::MSH_I64_SHR_S,
            InstructionSet::MSH_I64_SHR_U,
            InstructionSet::MSH_I64_ROTL,
            InstructionSet::MSH_I64_ROTR,
            InstructionSet::MSH_UDIVMOD64,
        ];
        let mut max = 0;
        let mut i = 0;
        while i < peaks.len() {
            if peaks[i] > max {
                max = peaks[i];
            }
            i += 1;
        }
        max
    };

    fn definition(&self) -> &'static SnippetDefinition {
        use wasmparser::ValType::*;
        use Snippet::*;
        match self {
            I64Eq => define_snippet!(op_i64_eq, MSH_I64_EQ, &[I64, I64], &[I32]),
            I64Ne => define_snippet!(op_i64_ne, MSH_I64_NE, &[I64, I64], &[I32]),
            I64LtS => define_snippet!(op_i64_lt_s, MSH_I64_LT_S, &[I64, I64], &[I32]),
            I64LtU => define_snippet!(op_i64_lt_u, MSH_I64_LT_U, &[I64, I64], &[I32]),
            I64GtS => define_snippet!(op_i64_gt_s, MSH_I64_GT_S, &[I64, I64], &[I32]),
            I64GtU => define_snippet!(op_i64_gt_u, MSH_I64_GT_U, &[I64, I64], &[I32]),
            I64LeS => define_snippet!(op_i64_le_s, MSH_I64_LE_S, &[I64, I64], &[I32]),
            I64LeU => define_snippet!(op_i64_le_u, MSH_I64_LE_U, &[I64, I64], &[I32]),
            I64GeS => define_snippet!(op_i64_ge_s, MSH_I64_GE_S, &[I64, I64], &[I32]),
            I64GeU => define_snippet!(op_i64_ge_u, MSH_I64_GE_U, &[I64, I64], &[I32]),
            I64Add => define_snippet!(op_i64_add, MSH_I64_ADD, &[I64, I64], &[I64]),
            I64Sub => define_snippet!(op_i64_sub, MSH_I64_SUB, &[I64, I64], &[I64]),
            I64Mul => define_snippet!(op_i64_mul, MSH_I64_MUL, &[I64, I64], &[I64]),
            I64DivS => define_snippet!(
                op_i64_div_s,
                op_i64_div_s_inline,
                MSH_I64_DIV_S,
                &[I64, I64],
                &[I64],
                &[UDivMod64]
            ),
            I64DivU => define_snippet!(
                op_i64_div_u,
                op_i64_div_u_inline,
                MSH_I64_DIV_U,
                &[I64, I64],
                &[I64],
                &[UDivMod64]
            ),
            I64RemS => define_snippet!(
                op_i64_rem_s,
                op_i64_rem_s_inline,
                MSH_I64_REM_S,
                &[I64, I64],
                &[I64],
                &[UDivMod64]
            ),
            I64RemU => define_snippet!(
                op_i64_rem_u,
                op_i64_rem_u_inline,
                MSH_I64_REM_U,
                &[I64, I64],
                &[I64],
                &[UDivMod64]
            ),
            I64Shl => define_snippet!(op_i64_shl, MSH_I64_SHL, &[I64, I64], &[I64]),
            I64ShrS => define_snippet!(op_i64_shr_s, MSH_I64_SHR_S, &[I64, I64], &[I64]),
            I64ShrU => define_snippet!(op_i64_shr_u, MSH_I64_SHR_U, &[I64, I64], &[I64]),
            I64RotL => define_snippet!(op_i64_rotl, MSH_I64_ROTL, &[I64, I64], &[I64]),
            I64RotR => define_snippet!(op_i64_rotr, MSH_I64_ROTR, &[I64, I64], &[I64]),
            UDivMod64 => define_snippet!(op_udivmod64, MSH_UDIVMOD64, &[I64, I64], &[I64, I64]),
        }
    }

    pub fn emitter(&self) -> fn(&mut InstructionSet) {
        self.definition().emitter
    }

    /// Snippets this snippet's body calls via unresolved `CallInternal` placeholders.
    pub fn dependencies(&self) -> &'static [Snippet] {
        self.definition().dependencies
    }

    pub fn emit(&self, instruction_set: &mut InstructionSet) {
        (self.definition().emitter)(instruction_set);
    }

    /// Emits the snippets-off body: self-contained, so it never leaves an unresolved
    /// snippet-to-snippet `CallInternal` behind at the splice site.
    pub fn emit_inline(&self, instruction_set: &mut InstructionSet) {
        (self.definition().inline_emitter)(instruction_set);
    }

    pub fn max_stack_height(&self) -> u32 {
        self.definition().max_stack_height
    }

    pub fn orig_func_type(&self) -> FuncType {
        let params = self.definition().orig_params.to_vec();
        let result = self.definition().orig_results.to_vec();
        FuncType::new(params, result)
    }

    pub fn func_type(&self) -> FuncType {
        let params = expand_i64_to_i32(self.definition().orig_params);
        let result = expand_i64_to_i32(self.definition().orig_results);
        FuncType::new(params, result)
    }
}

fn expand_i64_to_i32(params: &[ValType]) -> Vec<ValType> {
    let mut expanded = Vec::new();
    for &t in params {
        match t {
            ValType::I64 | ValType::F64 => {
                expanded.push(ValType::I32);
                expanded.push(ValType::I32);
            }
            _ => expanded.push(t),
        }
    }
    expanded
}

#[derive(Debug, Clone)]
pub struct SnippetCall {
    pub snippet: Snippet,
    pub loc: u32, // call instruction index
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::N_STACK_TRAMPOLINE_HEADROOM;

    /// `MAX_STACK_HEIGHT` is computed from a literal list of peaks, so a new snippet has to be
    /// added there and to `ALL`: this pins the list to the definitions and to the value-stack
    /// headroom that makes a snippet call at the caller's peak fit (audit 2026-09-18).
    #[test]
    fn max_stack_height_is_pinned() {
        let peak = Snippet::ALL
            .iter()
            .map(Snippet::max_stack_height)
            .max()
            .unwrap();
        assert_eq!(peak, Snippet::MAX_STACK_HEIGHT);
        assert!(Snippet::MAX_STACK_HEIGHT as usize <= N_STACK_TRAMPOLINE_HEADROOM);
        // every variant is listed once
        let mut listed = Snippet::ALL.to_vec();
        listed.sort();
        listed.dedup();
        assert_eq!(listed.len(), Snippet::ALL.len());
        for snippet in Snippet::ALL {
            // a wrapper's peak covers the snippets it calls into
            for dependency in snippet.dependencies() {
                assert!(dependency.max_stack_height() <= snippet.max_stack_height());
            }
        }
    }
}
