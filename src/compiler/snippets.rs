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
}

#[derive(Debug)]
struct SnippetDefinition {
    pub emitter: fn(&mut InstructionSet),
    pub max_stack_height: u32,
    pub orig_params: &'static [ValType],
    pub orig_results: &'static [ValType],
}

macro_rules! define_snippet {
    ($emitter:ident, $max_stack_height:ident, $params:expr, $results:expr) => {{
        static DEF: SnippetDefinition = SnippetDefinition {
            emitter: InstructionSet::$emitter,
            max_stack_height: InstructionSet::$max_stack_height,
            orig_params: $params,
            orig_results: $results,
        };
        &DEF
    }};
}

impl Snippet {
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
            I64DivS => define_snippet!(op_i64_div_s, MSH_I64_DIV_S, &[I64, I64], &[I64]),
            I64DivU => define_snippet!(op_i64_div_u, MSH_I64_DIV_U, &[I64, I64], &[I64]),
            I64RemS => define_snippet!(op_i64_rem_s, MSH_I64_REM_S, &[I64, I64], &[I64]),
            I64RemU => define_snippet!(op_i64_rem_u, MSH_I64_REM_U, &[I64, I64], &[I64]),
            I64Shl => define_snippet!(op_i64_shl, MSH_I64_SHL, &[I64, I64], &[I64]),
            I64ShrS => define_snippet!(op_i64_shr_s, MSH_I64_SHR_S, &[I64, I64], &[I64]),
            I64ShrU => define_snippet!(op_i64_shr_u, MSH_I64_SHR_U, &[I64, I64], &[I64]),
            I64RotL => define_snippet!(op_i64_rotl, MSH_I64_ROTL, &[I64, I64], &[I64]),
            I64RotR => define_snippet!(op_i64_rotr, MSH_I64_ROTR, &[I64, I64], &[I64]),
        }
    }

    pub fn emitter(&self) -> fn(&mut InstructionSet) {
        self.definition().emitter
    }

    pub fn emit(&self, instruction_set: &mut InstructionSet) {
        (self.definition().emitter)(instruction_set);
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
