use crate::{core::ValueType, module::ImportName};
use hashbrown::HashMap;
use crate::engine::bytecode::Instruction;

#[derive(Debug, Default, Clone)]
pub struct ImportLinker {
    func_by_name: HashMap<ImportName, ImportLinkerEntity>,
}

#[derive(Debug, Clone)]
pub struct ImportLinkerEntity {
    pub func_idx: u32,
    pub fuel_procedure: &'static [Instruction],
    pub params: &'static [ValueType],
    pub result: &'static [ValueType],
}

impl<I> From<I> for ImportLinker
where
    I: IntoIterator<Item = (ImportName, ImportLinkerEntity)>,
{
    fn from(iter: I) -> Self {
        Self {
            func_by_name: HashMap::from_iter(iter),
        }
    }
}

impl ImportLinker {
    pub fn insert_function(
        &mut self,
        import_name: ImportName,
        func_idx: u32,
        fuel_procedure: &'static [Instruction],
        params: &'static [ValueType],
        result: &'static [ValueType],
    ) {
        let last_value = self.func_by_name.insert(
            import_name,
            ImportLinkerEntity {
                func_idx,
                fuel_procedure,
                params,
                result,
            },
        );
        assert!(last_value.is_none(), "rwasm: import linker name collision");
    }

    pub fn resolve_by_import_name(&self, import_name: &ImportName) -> Option<&ImportLinkerEntity> {
        self.func_by_name.get(import_name)
    }
}
