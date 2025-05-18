use crate::CompiledExpr;
use wasmparser::GlobalType;

#[derive(Debug)]
pub struct GlobalVariable {
    pub global_type: GlobalType,
    pub init_expr: CompiledExpr,
}
