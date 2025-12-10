use crate::{CompilationError, FuncTypeIdx, SignatureIdx};
use alloc::vec::Vec;
use wasmparser::{BlockType, FuncType, ValType};

#[derive(Default, Debug)]
pub struct FuncTypeRegistry {
    original_func_types: Vec<FuncType>,
    func_types: Vec<FuncType>,
    original_signatures: Vec<SignatureIdx>,
}

impl FuncTypeRegistry {
    pub fn new(func_types: Vec<FuncType>) -> Result<Self, CompilationError> {
        let mut result = Self::default();
        for func_type in func_types {
            result.alloc_func_type(func_type)?;
        }
        Ok(result)
    }

    pub fn resolve_func_type(&self, func_type_idx: FuncTypeIdx) -> &FuncType {
        &self.func_types[func_type_idx as usize]
    }

    pub fn resolve_original_func_type(&self, func_type_idx: FuncTypeIdx) -> &FuncType {
        &self.original_func_types[func_type_idx as usize]
    }

    pub fn alloc_func_type(
        &mut self,
        func_type: FuncType,
    ) -> Result<FuncTypeIdx, CompilationError> {
        let mut adjusted_params = Vec::new();
        let mut adjusted_result = Vec::new();
        for x in func_type.params() {
            match x {
                ValType::I64 | ValType::F64 => {
                    adjusted_params.push(ValType::I32);
                    adjusted_params.push(ValType::I32);
                }
                ValType::V128 => {
                    return Err(CompilationError::NotSupportedFuncType);
                }
                _ => adjusted_params.push(*x),
            }
        }
        for x in func_type.results() {
            match x {
                ValType::I64 | ValType::F64 => {
                    adjusted_result.push(ValType::I32);
                    adjusted_result.push(ValType::I32);
                }
                ValType::V128 => {
                    return Err(CompilationError::NotSupportedFuncType);
                }
                _ => adjusted_result.push(*x),
            }
        }
        let dedup_type_position = self
            .original_func_types
            .iter()
            .position(|v| v == &func_type);
        let next_func_type_index = self.original_func_types.len();
        self.original_func_types.push(func_type);
        let adjusted_func_type = FuncType::new(adjusted_params, adjusted_result);
        self.func_types.push(adjusted_func_type);
        let dedup_type_position = dedup_type_position.unwrap_or(next_func_type_index);
        self.original_signatures
            .push(dedup_type_position as SignatureIdx);
        Ok(next_func_type_index as FuncTypeIdx)
    }

    pub fn resolve_func_params_len_type_by_block(&self, block_type: BlockType) -> usize {
        let func_type_index = match block_type {
            BlockType::FuncType(func_type_index) => func_type_index,
            BlockType::Empty | BlockType::Type(_) => return 0,
        };
        self.resolve_func_type_ref(func_type_index, |func_type| func_type.params().len())
    }

    pub fn resolve_func_results_len_type_by_block(&self, block_type: BlockType) -> usize {
        let func_type_index = match block_type {
            BlockType::FuncType(func_type_index) => func_type_index,
            BlockType::Type(ty) => {
                return match ty {
                    ValType::I64 | ValType::F64 => 2,
                    _ => 1,
                }
            }
            BlockType::Empty => return 0,
        };
        self.resolve_func_type_ref(func_type_index, |func_type| func_type.results().len())
    }

    pub fn resolve_func_type_ref<R, F: FnOnce(&FuncType) -> R>(
        &self,
        func_type_idx: FuncTypeIdx,
        f: F,
    ) -> R {
        let func_type = self.func_types.get(func_type_idx as usize).unwrap();
        f(func_type)
    }

    pub fn resolve_func_type_signature(&self, func_type_idx: FuncTypeIdx) -> SignatureIdx {
        self.original_signatures[func_type_idx as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    pub use wasmparser::ValType::*;

    #[test]
    fn resolves_unique_signatures_correctly() {
        let registry = FuncTypeRegistry::new(vec![
            FuncType::new([I32], [I32]),
            FuncType::new([I64], [I64]),
            FuncType::new([F32], [F32]),
        ])
        .unwrap();

        assert_eq!(registry.resolve_func_type_signature(0), 0);
        assert_eq!(registry.resolve_func_type_signature(1), 1);
        assert_eq!(registry.resolve_func_type_signature(2), 2);
    }

    #[test]
    fn deduplicates_matching_signatures() {
        let registry = FuncTypeRegistry::new(vec![
            FuncType::new([I32], [I32]),
            FuncType::new([I64], [I64]),
            FuncType::new([I32], [I32]), // duplicate
            FuncType::new([I64], [I64]), // duplicate
        ])
        .unwrap();

        assert_eq!(registry.resolve_func_type_signature(0), 0);
        assert_eq!(registry.resolve_func_type_signature(1), 1);
        assert_eq!(registry.resolve_func_type_signature(2), 0); // deduped
        assert_eq!(registry.resolve_func_type_signature(3), 1);
    }

    #[test]
    fn index_lookup_is_stable() {
        let registry = FuncTypeRegistry::new(vec![
            FuncType::new([I32], []),
            FuncType::new([I32], []),
            FuncType::new([I32], []),
        ])
        .unwrap();

        // All should resolve to canonical index 0
        assert_eq!(registry.resolve_func_type_signature(0), 0);
        assert_eq!(registry.resolve_func_type_signature(1), 0);
        assert_eq!(registry.resolve_func_type_signature(2), 0);

        assert_eq!(registry.original_func_types.len(), 3);
        assert_eq!(registry.func_types.len(), 3);
        assert_eq!(registry.original_signatures.len(), 3);
    }
}
