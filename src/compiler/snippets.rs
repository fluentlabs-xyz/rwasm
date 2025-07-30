use crate::InstructionSet;
use wasmparser::ValType;

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
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64RotL,
    I64RotR,
}

#[derive(Debug)]
pub struct SnippetDefinition {
    pub emitter: fn(&mut InstructionSet),
    pub max_stack_height: u32,
    pub params: &'static [ValType],
    pub results: &'static [ValType],
    pub orig_params: &'static [ValType],
    pub orig_results: &'static [ValType],
}

macro_rules! define_i64_snippet {
    ($emitter:ident, $max_stack_height:ident) => {{
        static DEF: SnippetDefinition = SnippetDefinition {
            emitter: InstructionSet::$emitter,
            max_stack_height: InstructionSet::$max_stack_height,
            params: &[ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            results: &[ValType::I32, ValType::I32],
            orig_params: &[ValType::I64, ValType::I64],
            orig_results: &[ValType::I64],
        };
        &DEF
    }};
}

impl Snippet {
    pub fn definition(&self) -> &'static SnippetDefinition {
        use Snippet::*;
        match self {
            I64Eq => define_i64_snippet!(op_i64_eq, MSH_I64_EQ),
            I64Ne => define_i64_snippet!(op_i64_ne, MSH_I64_NE),
            I64LtS => define_i64_snippet!(op_i64_lt_s, MSH_I64_LT_S),
            I64LtU => define_i64_snippet!(op_i64_lt_u, MSH_I64_LT_U),
            I64GtS => define_i64_snippet!(op_i64_gt_s, MSH_I64_GT_S),
            I64GtU => define_i64_snippet!(op_i64_gt_u, MSH_I64_GT_U),
            I64LeS => define_i64_snippet!(op_i64_le_s, MSH_I64_LE_S),
            I64LeU => define_i64_snippet!(op_i64_le_u, MSH_I64_LE_U),
            I64GeS => define_i64_snippet!(op_i64_ge_s, MSH_I64_GE_S),
            I64GeU => define_i64_snippet!(op_i64_ge_u, MSH_I64_GE_U),
            I64Add => define_i64_snippet!(op_i64_add, MSH_I64_ADD),
            I64Sub => define_i64_snippet!(op_i64_sub, MSH_I64_SUB),
            I64Mul => define_i64_snippet!(op_i64_mul, MSH_I64_MUL),
            I64DivS => define_i64_snippet!(op_i64_div_s, MSH_I64_DIV_S),
            I64DivU => define_i64_snippet!(op_i64_div_u, MSH_I64_DIV_U),
            I64RemS => define_i64_snippet!(op_i64_rem_s, MSH_I64_REM_S),
            I64RemU => define_i64_snippet!(op_i64_rem_u, MSH_I64_REM_U),
            I64And => define_i64_snippet!(op_i64_and, MSH_I64_AND),
            I64Or => define_i64_snippet!(op_i64_or, MSH_I64_OR),
            I64Xor => define_i64_snippet!(op_i64_xor, MSH_I64_XOR),
            I64Shl => define_i64_snippet!(op_i64_shl, MSH_I64_SHL),
            I64ShrS => define_i64_snippet!(op_i64_shr_s, MSH_I64_SHR_S),
            I64ShrU => define_i64_snippet!(op_i64_shr_u, MSH_I64_SHR_U),
            I64RotL => define_i64_snippet!(op_i64_rotl, MSH_I64_ROTL),
            I64RotR => define_i64_snippet!(op_i64_rotr, MSH_I64_ROTR),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SnippetCall {
    pub snippet: Snippet,
    pub loc: u32, // call instruction index
}
